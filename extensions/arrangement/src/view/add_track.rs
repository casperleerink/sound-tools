//! The add track button, right under the last track header: `Add track` adds an instrument
//! track, and its chevron offers an instrument track or an audio track. It has the shape of a
//! selected header, its plus where a header has its dot and its words where a name starts. It
//! scrolls with the tracks.
//!
//! It is a view of its own over the timeline, as the recording overlay is: the timeline paints
//! its headers on one canvas, and a notified timeline paints every clip again.

use gpui::{App, Context, Entity, Focusable, IntoElement, Render, Window, div, prelude::*, px};
use sound_core::{Instance, Project, ProjectError};
use sound_ui::Session;
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked,
};
use sound_ui::components::split_button::{SplitButton, SplitChoices};

use super::layout::{ADD_ROW_HEIGHT, HEADER_INSET, HEADER_WIDTH, RULER_HEIGHT};
use super::timeline::{DropTarget, Timeline};
use crate::{ArrangementState, TrackKind};

/// Adds a track of a kind to the arrangement, as one undo step. Whoever registers the view
/// passes it in: a new instrument track gets the default instrument, and this crate knows no
/// instrument.
pub type AddTrack =
    fn(&mut Project, &Instance<ArrangementState>, TrackKind) -> Result<(), ProjectError>;

const INSTRUMENT: &str = "instrument-track";
const AUDIO: &str = "audio-track";
/// The inset of the button from the top and bottom of its row, as a selected header has.
const BUTTON_INSET: f32 = 4.;

/// The kind of track a menu value stands for.
fn kind_of(value: &str) -> Option<TrackKind> {
    match value {
        INSTRUMENT => Some(TrackKind::Instrument),
        AUDIO => Some(TrackKind::Audio),
        _ => None,
    }
}

pub struct AddTrackButton {
    timeline: Entity<Timeline>,
    button: Entity<SplitButton>,
}

impl AddTrackButton {
    pub(super) fn new(
        session: Entity<Session>,
        arrangement: Instance<ArrangementState>,
        timeline: Entity<Timeline>,
        add_track: AddTrack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // A scroll, or a track more or less, moves the button.
        cx.observe(&timeline, |_, _, cx| cx.notify()).detach();
        let button = cx.new(|cx| {
            let items = [
                MenuItem::new(INSTRUMENT, "Instrument track").selectable(false),
                MenuItem::new(AUDIO, "Audio track").selectable(false),
            ];
            let choices = SplitChoices {
                label: "Add track".into(),
                main_value: INSTRUMENT.into(),
                menu_label: "Instrument or audio track".into(),
                entries: vec![MenuEntry::Group(MenuGroup::new().items(items))],
            };
            SplitButton::new("add-track", choices, cx)
                .icon("plus")
                .row(ADD_ROW_HEIGHT - 2. * BUTTON_INSET, cx)
        });
        cx.subscribe(&button, move |_, _, picked: &MenuPicked, cx| {
            let Some(kind) = kind_of(&picked.0) else {
                return;
            };
            // An error shows as the notice of the session.
            session.update(cx, |session, cx| {
                session.edit(cx, |project| add_track(project, &arrangement, kind));
            });
        })
        .detach();
        let menu = button.read(cx).menu().clone();
        cx.observe(&menu, |_, _, cx| cx.notify()).detach();
        // Tab may reach the button while it is scrolled out of the view.
        let focus = button.focus_handle(cx);
        cx.on_focus_in(&focus, window, |this, _, cx| {
            this.timeline
                .update(cx, |timeline, cx| timeline.reveal_add_row(cx));
        })
        .detach();
        Self { timeline, button }
    }

    /// The menu behind the chevron.
    pub(super) fn menu(&self, cx: &App) -> Entity<DropdownMenu> {
        self.button.read(cx).menu().clone()
    }
}

impl Render for AddTrackButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let menu_is_open = self.menu(cx).read(cx).is_open();
        let timeline = self.timeline.read(cx);
        // Files dragged under the last track show the header of the track they would make
        // where the button is.
        let dropping = matches!(timeline.incoming_target(), Some(DropTarget::NewTrack(_)));
        let top = timeline.add_row_top(cx) + BUTTON_INSET;
        // Clipped to the header column under the ruler, as the painted headers are. Not while
        // the menu is open: GPUI clips the menu to where it was made, which is this column.
        div()
            .absolute()
            .top(px(RULER_HEIGHT))
            .bottom_0()
            .left_0()
            .w(px(HEADER_WIDTH - 1.))
            .when(!menu_is_open, |column| column.overflow_hidden())
            .when(!dropping, |column| {
                column.child(
                    div()
                        .absolute()
                        .top(px(top))
                        .left(px(HEADER_INSET))
                        .w(px(HEADER_WIDTH - 2. * HEADER_INSET))
                        .occlude()
                        .child(self.button.clone()),
                )
            })
    }
}
