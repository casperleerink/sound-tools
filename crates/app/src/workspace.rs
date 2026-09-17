use crate::editing::{self, Edit, Selection};
use gpui::{
    App, ClickEvent, Context, ElementId, FocusHandle, Focusable, Render, ScrollHandle, Window,
    actions, div, prelude::*, px,
};
use sound_core::{Error, Result};
use sound_runtime::session::{Command, Session, Snapshot};
use sound_ui::{
    ActiveTheme,
    components::button::{Button, ButtonSize, ButtonVariant},
    typography,
};
use std::{
    sync::mpsc::{Receiver, TryRecvError},
    time::Duration,
};

actions!(workspace, [TogglePlay, Undo, Redo]);

fn poll_receipt(receipt: &Receiver<Result<()>>) -> Option<Result<()>> {
    match receipt.try_recv() {
        Ok(result) => Some(result),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => Some(Err(Error(
            "Session worker stopped before confirming the command".into(),
        ))),
    }
}

pub(crate) fn poll_pending(pending: &mut Option<Receiver<Result<()>>>) -> Option<Result<()>> {
    let result = poll_receipt(pending.as_ref()?)?;
    *pending = None;
    Some(result)
}

pub struct Workspace {
    pub(crate) session: Session,
    pub(crate) project_root: std::path::PathBuf,
    pub(crate) instruments: crate::instruments::Instruments,
    pub(crate) snap: Snapshot,
    pub(crate) scroll: ScrollHandle,
    pub(crate) selection: Option<Selection>,
    pub(crate) first_beat: u64,
    focus: FocusHandle,
    agent: gpui::Entity<crate::agent_sidebar::AgentSidebar>,
    pending: Option<Receiver<Result<()>>>,
    transport_receipts: Vec<Receiver<Result<()>>>,
    pub(crate) status: String,
}

impl Workspace {
    pub fn new(
        session: Session,
        root: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let snap = session.snapshot();
        let focus = cx.focus_handle();
        window.focus(&focus);
        cx.spawn(async move |this, cx| {
            loop {
                gpui::Timer::after(Duration::from_millis(30)).await;
                if this
                    .update(cx, |this: &mut Self, cx| {
                        this.refresh();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        Self {
            session,
            project_root: root.clone().into(),
            instruments: crate::instruments::Instruments::new(cx),
            snap,
            scroll: ScrollHandle::new(),
            selection: None,
            first_beat: 0,
            focus,
            agent: cx.new(|cx| crate::agent_sidebar::AgentSidebar::new(root.into(), cx)),
            pending: None,
            transport_receipts: Vec::new(),
            status: "Ready".into(),
        }
    }

    fn refresh(&mut self) {
        if let Some(result) = poll_pending(&mut self.pending) {
            self.status = match result {
                Ok(()) => "Edit completed".into(),
                Err(error) => error.to_string(),
            };
        }
        self.transport_receipts
            .retain(|receipt| match poll_receipt(receipt) {
                None => true,
                Some(result) => {
                    if let Err(error) = result {
                        self.status = error.to_string();
                    }
                    false
                }
            });
        let snap = self.session.snapshot();
        self.selection =
            editing::reconcile_selection(&self.snap.arrangement, &snap.arrangement, self.selection);
        self.refresh_instruments(&snap);
        self.snap = snap;
    }

    pub(crate) fn busy(&self) -> bool {
        self.pending.is_some() || self.instruments.awaiting_import()
    }

    pub(crate) fn select(&mut self, selection: Selection, cx: &mut Context<Self>) {
        let displayed = self.snap.arrangement.clone();
        self.refresh();
        self.selection =
            editing::reconcile_selection(&displayed, &self.snap.arrangement, Some(selection));
        cx.notify();
    }

    pub(crate) fn current(&mut self, cx: &mut Context<Self>) -> bool {
        let displayed = self.snap.revision;
        self.refresh();
        if self.busy() {
            self.status = "Waiting for the previous edit; this action was not queued".into();
        } else if displayed != self.snap.revision {
            self.status = "Project changed externally. Review the live state and retry.".into();
        } else {
            return true;
        }
        cx.notify();
        false
    }

    pub(crate) fn queue(&mut self, command: Command, cx: &mut Context<Self>) {
        if self.busy() {
            self.status = "Waiting for the previous edit; this action was not queued".into();
        } else {
            match self.session.send(command) {
                Ok(receipt) => {
                    self.pending = Some(receipt);
                    self.status = "Queued; waiting for command receipt".into();
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        cx.notify();
    }

    pub(crate) fn transport(&mut self, command: Command, cx: &mut Context<Self>) {
        self.refresh();
        match self.session.send(command) {
            Ok(receipt) => self.transport_receipts.push(receipt),
            Err(error) => self.status = error.to_string(),
        }
        cx.notify();
    }

    pub(crate) fn edit(&mut self, edit: Edit, cx: &mut Context<Self>) {
        if !self.current(cx) {
            return;
        }
        match editing::apply(&self.snap.arrangement, edit) {
            Ok(arrangement) if arrangement != self.snap.arrangement => {
                self.selection = None;
                self.queue(
                    Command::SetArrangement {
                        arrangement,
                        expected_revision: self.snap.revision,
                    },
                    cx,
                );
            }
            Ok(_) => {
                self.status = "No change".into();
                cx.notify();
            }
            Err(error) => {
                self.status = error.to_string();
                cx.notify();
            }
        }
    }

    fn master(&mut self, delta: f32, cx: &mut Context<Self>) {
        if !self.current(cx) {
            return;
        }
        let gain = (self.snap.master_gain + delta).clamp(0.0, 2.0);
        if gain != self.snap.master_gain {
            self.queue(Command::SetMasterGain(gain), cx);
        }
    }

    fn history(&mut self, command: Command, cx: &mut Context<Self>) {
        if !self.current(cx) {
            return;
        }
        self.selection = None;
        self.queue(command, cx);
    }

    pub(crate) fn edit_button(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<gpui::SharedString>,
        edit: Edit,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id, label)
            .size(ButtonSize::Xs)
            .variant(ButtonVariant::Subtle)
            .disabled(self.busy())
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.edit(edit, cx)))
    }

    fn transport_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let beat = self.snap.frame / sound_daw::arrangement::BEAT;
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .p_3()
            .child(
                Button::new("play", if self.snap.playing { "Pause" } else { "Play" })
                    .size(ButtonSize::Sm)
                    .disabled(!self.snap.audio_available)
                    .on_click(
                        cx.listener(|this, _, _, cx| this.transport(Command::TogglePlay, cx)),
                    ),
            )
            .child(
                Button::new("stop", "Stop")
                    .size(ButtonSize::Sm)
                    .on_click(cx.listener(|this, _, _, cx| this.transport(Command::Stop, cx))),
            )
            .child(
                Button::new("undo", "Undo")
                    .size(ButtonSize::Sm)
                    .disabled(self.busy())
                    .on_click(cx.listener(|this, _, _, cx| this.history(Command::Undo, cx))),
            )
            .child(
                Button::new("redo", "Redo")
                    .size(ButtonSize::Sm)
                    .disabled(self.busy())
                    .on_click(cx.listener(|this, _, _, cx| this.history(Command::Redo, cx))),
            )
            .child(div().font(typography::tabular()).child(format!(
                "{}.{}  |  frame {}",
                beat / 4 + 1,
                beat % 4 + 1,
                self.snap.frame
            )))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .child(self.snap.name.clone()),
            )
    }

    fn selection_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .flex_wrap()
            .gap_2()
            .p_2()
            .child(self.edit_button("add-track", "Add track", Edit::AddTrack, cx))
            .when_some(self.selection, |row, selection| {
                row.child(format!(
                    "Track {}, clip {}",
                    selection.track + 1,
                    selection.clip + 1
                ))
                .child(self.edit_button(
                    "delete-clip",
                    "Delete clip",
                    Edit::DeleteClip(selection),
                    cx,
                ))
                .child(self.edit_button(
                    "duplicate-clip",
                    "Duplicate at end",
                    Edit::DuplicateClip(selection),
                    cx,
                ))
                .child(self.edit_button("move-left", "-1 beat", Edit::MoveClip(selection, -1), cx))
                .child(self.edit_button("move-right", "+1 beat", Edit::MoveClip(selection, 1), cx))
                .child(self.edit_button(
                    "transpose-down",
                    "Notes -1",
                    Edit::Transpose(selection, -1),
                    cx,
                ))
                .child(self.edit_button(
                    "transpose-up",
                    "Notes +1",
                    Edit::Transpose(selection, 1),
                    cx,
                ))
            })
            .when(self.selection.is_none(), |row| {
                row.child("Select a clip to edit its position or notes")
            })
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let peak = self.snap.peak;
        div().id("sidebar").w(px(260.)).flex_none().h_full().overflow_y_scroll().p_3().flex().flex_col().gap_3()
            .border_l_1().border_color(theme.alpha_at(0.1))
            .child("Master")
            .child(div().flex().gap_2().items_center()
                .child(Button::new("master-down", "-").size(ButtonSize::Xs).disabled(self.busy())
                    .on_click(cx.listener(|this, _, _, cx| this.master(-0.1, cx))))
                .child(format!("{:.2} / 2.00", self.snap.master_gain))
                .child(Button::new("master-up", "+").size(ButtonSize::Xs).disabled(self.busy())
                    .on_click(cx.listener(|this, _, _, cx| this.master(0.1, cx)))))
            .child(div().w(px(220.)).h(px(12.)).bg(theme.gray_300)
                .child(div().h_full().w(px(220. * peak.clamp(0.0, 1.0))).bg(if peak > 1.0 { theme.red } else { theme.green })))
            .child(if peak > 0.0 { format!("Peak {:.1} dBFS", 20.0 * peak.log10()) } else { "Peak -inf dBFS".into() })
            .child(if self.snap.audio_available { "Audio output available" } else { "Audio output unavailable" })
            .child(format!("Underrun frames: {}", self.snap.underruns))
            .child(self.instrument_controls(cx))
            .child(self.agent.clone())
            .child(div().text_xs().child("Space: play/pause. Ctrl-Z: undo. Ctrl-Shift-Z: redo."))
            .child(div().text_xs().child("Edits wait for command confirmation. Stale arrangement edits are rejected; review the live state and retry."))
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        div()
            .id("workspace")
            .track_focus(&self.focus)
            .key_context("Workspace")
            .on_action(cx.listener(|this, _: &TogglePlay, window, cx| {
                if this.focus.is_focused(window) {
                    this.transport(Command::TogglePlay, cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &Undo, window, cx| {
                if this.focus.is_focused(window) {
                    this.history(Command::Undo, cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &Redo, window, cx| {
                if this.focus.is_focused(window) {
                    this.history(Command::Redo, cx);
                } else {
                    cx.propagate();
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.gray_100)
            .text_color(theme.gray_950)
            .font(typography::ui_font())
            .text_sm()
            .child(self.transport_bar(cx))
            .child(self.selection_bar(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(self.timeline(cx))
                    .child(self.sidebar(cx)),
            )
            .child(div().flex_none().p_2().child(self.status.clone()))
            .when_some(self.snap.error.clone(), |root, error| {
                root.child(
                    div()
                        .flex_none()
                        .px_2()
                        .pb_2()
                        .text_color(theme.red)
                        .child(error),
                )
            })
    }
}
