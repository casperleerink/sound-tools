//! The returns, pinned under the timeline above the master row, where they take no room from
//! the tracks until the composer wants them. Folded, the section is one row that says how many
//! returns there are and their names; open, each return has a row like a track header: its
//! dot, its name and M. A return plays no clips, so its row has no lane, and its panel is the
//! track panel. Whether the section is open is interface state; a new return opens it.
//!
//! Selecting, renaming and deleting go through the timeline, as for a track, so the track
//! panel, the keys and the name field work the same.

use gpui::{
    AnyElement, Context, Entity, EventEmitter, FocusHandle, FontWeight, Hsla, KeyDownEvent,
    Modifiers, MouseButton, MouseDownEvent, SharedString, Window, div, prelude::*, px,
};
use sound_core::{Changes, Instance, ProjectEvent};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::icon::Icon;
use sound_ui::components::toggle::{self, Toggle};
use sound_ui::{ActiveTheme, KeyboardFocus, Session};

use super::AddTrack;
use super::layout::{DOT_LEFT, HEADER_INSET, HEADER_WIDTH, NAME_LEFT};
use super::paint::accent;
use super::timeline::{MUTE_LEFT, RENAME_HEIGHT, RENAME_LEFT, Timeline};
use crate::{ArrangementState, TrackKind, TrackState, returns};

/// The row that folds the section.
pub const RETURNS_HEADER_HEIGHT: f32 = 32.;
/// A return, as tall as the master row.
pub const RETURN_ROW_HEIGHT: f32 = super::MASTER_ROW_HEIGHT;
/// How wide a name gets in the folded row before it is cut.
const FOLDED_NAME_WIDTH: f32 = 120.;

/// What the section asks of the view that holds it.
pub enum ReturnsEvent {
    /// A click on a return, or cmd-down on it: show its panel.
    Open(Instance<TrackState>),
}

/// What a row keeps of itself, in element state under its id.
struct RowState {
    focus_handle: FocusHandle,
    keyboard_focus: KeyboardFocus,
}

pub struct Returns {
    session: Entity<Session>,
    arrangement: Instance<ArrangementState>,
    timeline: Entity<Timeline>,
    add_track: AddTrack,
    expanded: bool,
    header_focus: FocusHandle,
    header_keyboard: KeyboardFocus,
    add_focus: FocusHandle,
}

impl EventEmitter<ReturnsEvent> for Returns {}

impl Returns {
    pub(super) fn new(
        session: Entity<Session>,
        arrangement: Instance<ArrangementState>,
        timeline: Entity<Timeline>,
        add_track: AddTrack,
        cx: &mut Context<Self>,
    ) -> Self {
        // The selection and the name field of a rename are the timeline's.
        cx.observe(&timeline, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&session, |returns, _, event, cx| {
            let (ProjectEvent::Created(id) | ProjectEvent::Changed(id) | ProjectEvent::Deleted(id)) =
                event
            else {
                return;
            };
            if id.parent().as_ref() != Some(returns.arrangement.id()) {
                return;
            }
            if matches!(event, ProjectEvent::Created(_)) {
                let project = returns.session.read(cx).project();
                let track = project.resolve::<TrackState>(id);
                let state = track.as_ref().and_then(|track| project.state(track));
                if state.is_some_and(|state| state.kind == TrackKind::Return) {
                    returns.expanded = true;
                }
            }
            cx.notify();
        })
        .detach();
        Self {
            session,
            arrangement,
            timeline,
            add_track,
            expanded: false,
            header_focus: cx.focus_handle().tab_stop(true),
            header_keyboard: KeyboardFocus::default(),
            add_focus: cx.focus_handle().tab_stop(true),
        }
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    pub fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        if self.expanded != expanded {
            self.expanded = expanded;
            cx.notify();
        }
    }

    fn add_return(&mut self, cx: &mut Context<Self>) {
        let (arrangement, add_track) = (self.arrangement.clone(), self.add_track);
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                add_track(project, &arrangement, TrackKind::Return)
            });
        });
    }

    fn set_mute(&mut self, track: &Instance<TrackState>, mute: bool, cx: &mut Context<Self>) {
        let Some(mut state) = self.session.read(cx).project().state(track).cloned() else {
            return;
        };
        state.mute = mute;
        let label = if mute { "Mute track" } else { "Unmute track" };
        self.session.update(cx, |session, cx| {
            session.edit(cx, |project| {
                let mut changes = Changes::new();
                changes.set(track, state);
                project.commit(label, changes)
            })
        });
    }

    /// A press on a return selects it and shows its panel; a double press edits its name.
    fn press_row(
        &mut self,
        track: &Instance<TrackState>,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let track = track.clone();
        self.timeline.update(cx, |timeline, cx| {
            timeline.select_track(Some(track.id().clone()), cx);
            if event.click_count == 2 {
                timeline.start_rename(track.clone(), window, cx);
            }
        });
        cx.emit(ReturnsEvent::Open(track));
    }

    /// The keys of a return with the focus, as the timeline has them for a track: enter edits
    /// its name, backspace and delete delete it, cmd-down shows its panel.
    fn row_key(
        &mut self,
        track: &Instance<TrackState>,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let modifiers = event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();
        let track = track.clone();
        match (key, modifiers) {
            ("down", modifiers) if modifiers == Modifiers::secondary_key() => {
                self.timeline.update(cx, |timeline, cx| {
                    timeline.select_track(Some(track.id().clone()), cx);
                });
                cx.emit(ReturnsEvent::Open(track));
            }
            ("enter", modifiers) if !modifiers.modified() => {
                self.timeline.update(cx, |timeline, cx| {
                    timeline.start_rename(track, window, cx);
                });
            }
            ("backspace" | "delete", modifiers) if !modifiers.modified() => {
                self.timeline.update(cx, |timeline, cx| {
                    timeline.delete_track(track, window, cx);
                });
            }
            _ => return false,
        }
        true
    }

    /// The row that folds the section: the chevron and the word where a track has its dot and
    /// name, the count, the plus at the right of the header column, and folded, the names.
    fn header(
        &self,
        names: Vec<(SharedString, Hsla)>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (hairline, text, muted, ring) = (
            theme.alpha_at(0.05),
            theme.gray_900,
            theme.gray_700,
            theme.lavender,
        );
        let count = names.len();
        let keyboard_ring = self.header_keyboard.shows_ring(&self.header_focus, window);
        let chevron = match self.expanded {
            true => "chevron-down",
            false => "chevron-right",
        };
        let add = Button::icon_only("add-return", "plus")
            .variant(ButtonVariant::Ghost)
            .size(ButtonSize::Xs)
            .focus_handle(&self.add_focus)
            .on_click(cx.listener(|returns, _, _, cx| returns.add_return(cx)));
        let middle = RETURNS_HEADER_HEIGHT / 2.;
        let column = div()
            .relative()
            .flex_none()
            .w(px(HEADER_WIDTH))
            .h_full()
            .border_r_1()
            .border_color(hairline)
            .child(
                div()
                    .absolute()
                    .left(px(HEADER_INSET))
                    .top(px(4.))
                    .w(px(HEADER_WIDTH - 2. * HEADER_INSET))
                    .h(px(RETURNS_HEADER_HEIGHT - 8.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(match keyboard_ring {
                        true => ring,
                        false => gpui::transparent_black(),
                    }),
            )
            .child(
                Icon::new(chevron)
                    .size(14.)
                    .color(muted)
                    .absolute()
                    .left(px(DOT_LEFT - 3.))
                    .top(px(middle - 7.)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(NAME_LEFT))
                    .top(px(middle - 8.))
                    .flex()
                    .gap(px(6.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .child(div().text_color(text).child("Returns"))
                    .child(div().text_color(muted).child(count.to_string())),
            )
            .child(
                div()
                    .debug_selector(|| "add-return".to_string())
                    .absolute()
                    .left(px(HEADER_WIDTH - HEADER_INSET - 4. - 24.))
                    .top(px(middle - 12.))
                    .child(add),
            );
        let folded = (!self.expanded).then(|| {
            names.into_iter().map(|(name, dot)| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .max_w(px(FOLDED_NAME_WIDTH))
                    .child(div().flex_none().size(px(6.)).rounded_full().bg(dot))
                    .child(div().min_w_0().truncate().child(name))
            })
        });
        div()
            .id("returns-header")
            .debug_selector(|| "returns-header".to_string())
            .track_focus(&self.header_focus)
            .flex_none()
            .h(px(RETURNS_HEADER_HEIGHT))
            .flex()
            .border_t_1()
            .border_color(hairline)
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|returns, _, _, cx| returns.header_keyboard.pressed(cx)),
            )
            .on_click(cx.listener(|returns, _, _, cx| {
                let expanded = !returns.expanded;
                returns.set_expanded(expanded, cx);
            }))
            .child(column)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .pl(px(16.))
                    .overflow_hidden()
                    .text_size(px(12.))
                    .text_color(muted)
                    .children(folded.into_iter().flatten()),
            )
            .into_any_element()
    }

    /// The row of a return: the fill of a selected header, the dot, the name and M. The name
    /// field of a rename takes the place of the name.
    fn row(
        &self,
        track: &Instance<TrackState>,
        state: &TrackState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = track.id().name().to_string();
        let row_state = window.use_keyed_state(
            SharedString::from(format!("return-row-{name}")),
            cx,
            |_, cx| RowState {
                focus_handle: cx.focus_handle().tab_stop(true),
                keyboard_focus: KeyboardFocus::default(),
            },
        );
        let focus_handle = row_state.read(cx).focus_handle.clone();
        let keyboard_ring = row_state
            .read(cx)
            .keyboard_focus
            .shows_ring(&focus_handle, window);
        let timeline = self.timeline.read(cx);
        let selected = timeline.selected_track() == Some(track.id());
        let rename = timeline
            .name_field()
            .filter(|(renamed, _)| renamed.id() == track.id())
            .map(|(_, input)| input.clone());
        let theme = cx.theme();
        let (selected_fill, text, ring, peach) = (
            theme.alpha_at(0.05),
            theme.gray_950,
            theme.lavender,
            theme.peach,
        );
        let hairline = theme.alpha_at(0.05);
        let dot = accent(state.colour, theme);
        let opacity = if state.mute { 0.4 } else { 1. };
        let middle = RETURN_ROW_HEIGHT / 2.;
        let mute = Toggle::new(SharedString::from(format!("mute-{name}")), "M", state.mute)
            .color(peach)
            .tab_stop(false)
            .on_change({
                let (returns, track) = (cx.weak_entity(), track.clone());
                move |mute, _, cx| {
                    returns
                        .update(cx, |returns, cx| returns.set_mute(&track, mute, cx))
                        .ok();
                }
            });
        let name_line = match rename {
            Some(input) => div()
                .absolute()
                .left(px(RENAME_LEFT))
                .top(px(middle - RENAME_HEIGHT / 2.))
                .w(px(MUTE_LEFT - RENAME_LEFT - 4.))
                .occlude()
                .child(input),
            None => div()
                .absolute()
                .left(px(NAME_LEFT))
                .top(px(middle - 10.))
                .w(px(MUTE_LEFT - NAME_LEFT - 8.))
                .truncate()
                .line_height(px(20.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(text)
                .opacity(opacity)
                .child(state.name.clone()),
        };
        let header = div()
            .relative()
            .flex_none()
            .w(px(HEADER_WIDTH))
            .h_full()
            .border_r_1()
            .border_color(hairline)
            .child(
                div()
                    .absolute()
                    .left(px(HEADER_INSET))
                    .top(px(4.))
                    .w(px(HEADER_WIDTH - 2. * HEADER_INSET))
                    .h(px(RETURN_ROW_HEIGHT - 8.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(match keyboard_ring {
                        true => ring,
                        false => gpui::transparent_black(),
                    })
                    .when(selected, |fill| fill.bg(selected_fill)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(DOT_LEFT))
                    .top(px(middle - 4.))
                    .size(px(8.))
                    .rounded_full()
                    .bg(dot)
                    .opacity(opacity),
            )
            .child(name_line)
            .child(
                div()
                    .absolute()
                    .left(px(MUTE_LEFT))
                    .top(px(middle - toggle::HEIGHT / 2.))
                    .occlude()
                    .child(mute),
            );
        let (pressed, keyed) = (track.clone(), track.clone());
        div()
            .id(SharedString::from(format!("return-{name}")))
            .debug_selector(move || format!("return-{name}"))
            .track_focus(&focus_handle)
            .flex_none()
            .h(px(RETURN_ROW_HEIGHT))
            .flex()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, {
                let focus_handle = focus_handle.clone();
                cx.listener(move |returns, event: &MouseDownEvent, window, cx| {
                    row_state.update(cx, |row, cx| row.keyboard_focus.pressed(cx));
                    window.focus(&focus_handle, cx);
                    returns.press_row(&pressed, event, window, cx);
                })
            })
            .on_key_down(
                cx.listener(move |returns, event: &KeyDownEvent, window, cx| {
                    if focus_handle.is_focused(window) && returns.row_key(&keyed, event, window, cx)
                    {
                        cx.stop_propagation();
                    }
                }),
            )
            .child(header)
            .into_any_element()
    }
}

impl Render for Returns {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let project = self.session.read(cx).project();
        let shown: Vec<(Instance<TrackState>, TrackState)> =
            returns(project, self.arrangement.id())
                .into_iter()
                .map(|(track, state)| (track, state.clone()))
                .collect();
        // No returns, no section: the add track menu makes the first.
        if shown.is_empty() {
            return div().into_any_element();
        }
        let theme = cx.theme();
        let names = shown
            .iter()
            .map(|(_, state)| (state.name.clone().into(), accent(state.colour, theme)))
            .collect();
        let header = self.header(names, window, cx);
        let rows: Vec<_> = match self.expanded {
            true => shown
                .iter()
                .map(|(track, state)| self.row(track, state, window, cx))
                .collect(),
            false => Vec::new(),
        };
        div()
            .flex_none()
            .flex()
            .flex_col()
            .child(header)
            .children(rows)
            .into_any_element()
    }
}
