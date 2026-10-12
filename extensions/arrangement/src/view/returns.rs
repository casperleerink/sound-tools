//! The returns, pinned under the timeline above the master row, where they take no room from
//! the tracks until the composer wants them. Folded, the section is one row that says how many
//! returns there are and their names; open, each return has a row like a track header: its
//! dot, its name and M. A return plays no clips, so its row has no lane, and its panel is the
//! track panel. Whether the section is open is interface state; a new return opens it.
//!
//! Selecting, renaming and deleting go through the timeline, as for a track, so the track
//! panel, the keys and the name field work the same. A name is edited with enter only: a
//! press opens the panel below, which moves the section up, so a double press would land on
//! the panel.

use std::collections::BTreeMap;

use gpui::{
    AnyElement, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla,
    KeyDownEvent, Modifiers, MouseButton, ScrollHandle, SharedString, Subscription, Window, div,
    prelude::*, px,
};
use sound_core::{Instance, ProjectEvent};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::icon::Icon;
use sound_ui::components::toggle::{self, Toggle};
use sound_ui::{ActiveTheme, KeyboardFocus, Session};

use super::AddTrack;
use super::layout::{DOT_LEFT, HEADER_INSET, HEADER_WIDTH, NAME_LEFT};
use super::paint::{accent, header_shape};
use super::timeline::{MUTE, MUTE_LEFT, RENAME_HEIGHT, RENAME_LEFT, Timeline};
use crate::{ArrangementState, TrackKind, TrackState, returns};

/// The row that folds the section.
const RETURNS_HEADER_HEIGHT: f32 = 32.;
/// A return, as tall as the master row.
pub const RETURN_ROW_HEIGHT: f32 = super::MASTER_ROW_HEIGHT;
/// How many rows the open section shows before it scrolls. The half row says there are more,
/// as the arrangement has no scrollbars.
const SHOWN_ROWS: f32 = 4.5;
/// How wide a name gets in the folded row before it is cut.
const FOLDED_NAME_WIDTH: f32 = 120.;

/// What the section asks of the view that holds it.
pub enum ReturnsEvent {
    /// A click on a return, or cmd-down on it: show its panel.
    Open(Instance<TrackState>),
}

/// What the row of a return keeps of itself.
struct RowState {
    focus_handle: FocusHandle,
    keyboard_focus: KeyboardFocus,
    /// Scrolls the row into view when tab brings it the focus.
    _focus: Subscription,
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
    /// By the folder name of each return. Kept here, so the focus of a row that goes can move
    /// on before the row is gone.
    rows: BTreeMap<String, RowState>,
    rows_scroll: ScrollHandle,
}

impl EventEmitter<ReturnsEvent> for Returns {}

impl Returns {
    pub(super) fn new(
        session: Entity<Session>,
        arrangement: Instance<ArrangementState>,
        timeline: Entity<Timeline>,
        add_track: AddTrack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // The selection and the name field of a rename are the timeline's.
        cx.observe(&timeline, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(&session, window, |returns, _, event, window, cx| {
            let (ProjectEvent::Created(id) | ProjectEvent::Changed(id) | ProjectEvent::Deleted(id)) =
                event
            else {
                return;
            };
            if id.parent().as_ref() != Some(returns.arrangement.id()) {
                return;
            }
            match event {
                ProjectEvent::Created(_) => {
                    let project = returns.session.read(cx).project();
                    let track = project.resolve::<TrackState>(id);
                    let state = track.as_ref().and_then(|track| project.state(track));
                    if state.is_some_and(|state| state.kind == TrackKind::Return) {
                        returns.expanded = true;
                    }
                }
                // Else the focus would fall out of the arrangement, and its keys with it.
                ProjectEvent::Deleted(_) => {
                    if let Some(row) = returns.rows.remove(id.name())
                        && row.focus_handle.contains_focused(window, cx)
                    {
                        window.focus(&returns.timeline.focus_handle(cx), cx);
                    }
                }
                _ => {}
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
            rows: BTreeMap::new(),
            rows_scroll: ScrollHandle::new(),
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

    /// A press on a return selects it and shows its panel.
    fn press_row(&mut self, track: &Instance<TrackState>, cx: &mut Context<Self>) {
        let track = track.clone();
        self.timeline.update(cx, |timeline, cx| {
            timeline.select_track(Some(track.id().clone()), cx);
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
                self.press_row(&track, cx);
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

    /// The row of each return that has none yet.
    fn add_rows(&mut self, names: &[String], window: &mut Window, cx: &mut Context<Self>) {
        for name in names {
            if self.rows.contains_key(name) {
                continue;
            }
            let focus_handle = cx.focus_handle().tab_stop(true);
            let focus = cx.on_focus(&focus_handle, window, {
                let name = name.clone();
                move |returns, _, cx| returns.scroll_to(&name, cx)
            });
            let row = RowState {
                focus_handle,
                keyboard_focus: KeyboardFocus::default(),
                _focus: focus,
            };
            self.rows.insert(name.clone(), row);
        }
    }

    fn scroll_to(&self, name: &str, cx: &mut Context<Self>) {
        let project = self.session.read(cx).project();
        let shown = returns(project, self.arrangement.id());
        if let Some(index) = shown
            .iter()
            .position(|(track, _)| track.id().name() == name)
        {
            self.rows_scroll.scroll_to_item(index);
            cx.notify();
        }
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
        let (hairline, text, muted) = (theme.alpha_at(0.05), theme.gray_900, theme.gray_700);
        let count = names.len();
        let keyboard_ring = self.header_keyboard.shows_ring(&self.header_focus, window);
        let shape = header_shape(RETURNS_HEADER_HEIGHT, false, keyboard_ring, theme);
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
            .child(shape)
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

    /// The row of a return: the shape of a selected header, the dot, the name and M. The name
    /// field of a rename takes the place of the name.
    fn row(
        &self,
        track: &Instance<TrackState>,
        state: &TrackState,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let name = track.id().name().to_string();
        let row = self.rows.get(&name)?;
        let focus_handle = row.focus_handle.clone();
        let keyboard_ring = row.keyboard_focus.shows_ring(&focus_handle, window);
        let timeline = self.timeline.read(cx);
        let selected = timeline.selected_track() == Some(track.id());
        let rename = timeline
            .name_field()
            .filter(|(renamed, _)| renamed.id() == track.id())
            .map(|(_, input)| input.clone());
        let theme = cx.theme();
        let (text, peach, hairline) = (theme.gray_950, theme.peach, theme.alpha_at(0.05));
        let shape = header_shape(RETURN_ROW_HEIGHT, selected, keyboard_ring, theme);
        let dot = accent(state.colour, theme);
        let opacity = if state.mute { 0.4 } else { 1. };
        let middle = RETURN_ROW_HEIGHT / 2.;
        let mute = Toggle::new(SharedString::from(format!("mute-{name}")), "M", state.mute)
            .color(peach)
            .tab_stop(false)
            .on_change({
                let (timeline, track) = (self.timeline.downgrade(), track.clone());
                move |mute, _, cx| {
                    timeline
                        .update(cx, |timeline, cx| timeline.switch(&track, MUTE, mute, cx))
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
            .child(shape)
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
        let element = div()
            .id(SharedString::from(format!("return-{name}")))
            .debug_selector({
                let name = name.clone();
                move || format!("return-{name}")
            })
            .track_focus(&focus_handle)
            .flex_none()
            .h(px(RETURN_ROW_HEIGHT))
            .flex()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, {
                let focus_handle = focus_handle.clone();
                cx.listener(move |returns, _, window, cx| {
                    if let Some(row) = returns.rows.get(&name) {
                        row.keyboard_focus.pressed(cx);
                    }
                    window.focus(&focus_handle, cx);
                    returns.press_row(&pressed, cx);
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
            .child(header);
        Some(element.into_any_element())
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
        let rows = self.expanded.then(|| {
            let folders: Vec<String> = (shown.iter())
                .map(|(track, _)| track.id().name().to_string())
                .collect();
            self.add_rows(&folders, window, cx);
            let rows: Vec<AnyElement> = (shown.iter())
                .filter_map(|(track, state)| self.row(track, state, window, cx))
                .collect();
            div()
                .id("return-rows")
                .track_scroll(&self.rows_scroll)
                .max_h(px(SHOWN_ROWS * RETURN_ROW_HEIGHT))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .children(rows)
        });
        div()
            .flex_none()
            .flex()
            .flex_col()
            .child(header)
            .children(rows)
            .into_any_element()
    }
}
