//! The track panel: the details of one track, in the panel below the timeline where the note
//! editor also shows. It is a rack: device cards from left to right, in the order the sound
//! goes through them. Today a track has one device, its instrument.
//!
//! The panel knows no instrument. It asks the view registry for the view of whatever instance
//! sits in the `instrument` slot of the track and puts it into a card. A slot that is empty,
//! or whose tool has no view, shows a quiet card that says so.
//!
//! How the rack grows. The instrument slot has a fixed name, so its card is made once and only
//! made again when the tool in it changes. Effects will come and go: [`device_slots`] then
//! reads them from the project, and the panel makes its list again on `Created` and `Deleted`
//! inside the track, keeping the device of every slot that stays, so that an open knob drag
//! of another device goes on. The mixer controls of the track (gain, pan, mute) are not
//! devices: they get a fixed section at the right end of the row in `render`, after the rack.

use gpui::{
    AnyView, App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Window, div,
    prelude::*, px,
};
use sound_core::{Instance, InstanceId, InvalidInstanceId, ProjectEvent};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::card::Card;
use sound_ui::{ActiveTheme, Session, Views};

use super::layout::{HEADER_WIDTH, RULER_HEIGHT};
use super::paint::accent;
use crate::{INSTRUMENT, TrackState};

/// What the panel asks of the view that holds it.
pub enum TrackPanelEvent {
    /// The close control.
    Close,
}

/// The devices of a track, in rack order, by the id of their slot. A slot may be empty.
fn device_slots(track: &InstanceId) -> Result<Vec<InstanceId>, InvalidInstanceId> {
    Ok(vec![track.child(INSTRUMENT)?])
}

/// One card of the rack: what is in a slot now, and its view when its tool has one.
struct Device {
    slot: InstanceId,
    /// `None` while the slot is empty.
    tool: Option<&'static str>,
    view: Option<AnyView>,
}

impl Device {
    fn new(session: &Entity<Session>, slot: InstanceId, window: &mut Window, cx: &mut App) -> Self {
        Self {
            tool: session.read(cx).project().tool_of(&slot),
            view: Views::view_of(session, &slot, window, cx),
            slot,
        }
    }
}

pub struct TrackPanel {
    session: Entity<Session>,
    track: Instance<TrackState>,
    devices: Vec<Device>,
    /// Not a tab stop. It tells whether the focus is inside the panel.
    focus_handle: FocusHandle,
    close_focus: FocusHandle,
}

impl EventEmitter<TrackPanelEvent> for TrackPanel {}

impl TrackPanel {
    pub(super) fn new(
        session: Entity<Session>,
        track: Instance<TrackState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe_in(&session, window, |panel, _, event, window, cx| {
            let (ProjectEvent::Created(id) | ProjectEvent::Changed(id) | ProjectEvent::Deleted(id)) =
                event
            else {
                return;
            };
            // The name and the colour. The view that holds the panel closes it with its track.
            if id == panel.track.id() {
                cx.notify();
            }
            // A slot got another tool, lost its record or got one: from a file or an undo.
            // While the tool stays, the view of the device follows its record by itself.
            let Some(index) = panel.devices.iter().position(|device| device.slot == *id) else {
                return;
            };
            let session = panel.session.clone();
            if panel.devices[index].tool != session.read(cx).project().tool_of(id) {
                panel.devices[index] = Device::new(&session, id.clone(), window, cx);
                cx.notify();
            }
        })
        .detach();
        let mut panel = Self {
            session,
            track: track.clone(),
            devices: Vec::new(),
            focus_handle: cx.focus_handle(),
            close_focus: cx.focus_handle().tab_stop(true),
        };
        panel.set_track(track, window, cx);
        panel
    }

    pub fn track(&self) -> &Instance<TrackState> {
        &self.track
    }

    /// The view in each card of the rack, left to right. `None` for a card without one.
    pub fn device_views(&self) -> impl Iterator<Item = Option<&AnyView>> {
        self.devices.iter().map(|device| device.view.as_ref())
    }

    /// Shows another track.
    pub fn set_track(
        &mut self,
        track: Instance<TrackState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let slots = device_slots(track.id()).unwrap_or_else(|error| {
            let session = self.session.clone();
            session.update(cx, |session, cx| session.report(error, cx));
            Vec::new()
        });
        let devices = slots.into_iter();
        self.devices = devices
            .map(|slot| Device::new(&self.session, slot, window, cx))
            .collect();
        self.track = track;
        cx.notify();
    }
}

impl Focusable for TrackPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TrackPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let project = self.session.read(cx).project();
        let theme = cx.theme();
        let (background, hairline, text, muted) = (
            theme.gray_100,
            theme.alpha_at(0.05),
            theme.gray_900,
            theme.gray_700,
        );
        // Gone: the view that holds the panel closes it after the same event.
        let (name, dot) = project.state(&self.track).map_or_else(
            || (String::new(), theme.blue),
            |track| (track.name.clone(), accent(track.colour, theme)),
        );

        let cards = self.devices.iter().map(|device| {
            let card = Card::new().flex_none();
            match (&device.view, device.tool) {
                (Some(view), _) => card.child(view.clone()),
                (None, tool) => {
                    let (title, body) = match tool {
                        Some(tool) => (tool, "This tool has no view."),
                        None => ("No instrument", "This track is silent."),
                    };
                    card.gap(px(4.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(text)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(muted)
                                .child(body),
                        )
                }
            }
        });

        let close = Button::icon_only("close-track-panel", "x")
            // Quiet until it is wanted, as in the note editor.
            .opacity(0.6)
            .variant(ButtonVariant::Ghost)
            .size(ButtonSize::Xs)
            .focus_handle(&self.close_focus)
            .on_click(cx.listener(|_, _, _, cx| cx.emit(TrackPanelEvent::Close)));
        // The header is that of the note editor: the dot and the name of the track in a row
        // as high as a ruler, where the track headers are above, then the close control.
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
                    .left(px(24.))
                    .top(px(RULER_HEIGHT / 2. - 4.))
                    .size(px(8.))
                    .rounded_full()
                    .bg(dot),
            )
            .child(
                div()
                    .absolute()
                    .left(px(44.))
                    .top(px(RULER_HEIGHT / 2. - 10.))
                    .w(px(HEADER_WIDTH - 44. - 40.))
                    .truncate()
                    .line_height(px(20.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(text)
                    .child(name),
            )
            .child(
                div()
                    .absolute()
                    .top(px(4.))
                    .left(px(HEADER_WIDTH - 8. - 24.))
                    .child(close),
            );

        div()
            .size_full()
            .relative()
            .flex()
            .bg(background)
            .track_focus(&self.focus_handle)
            // One hairline above the panel. It is on top and takes no room, as in the note
            // editor, so the header is at the same place in both.
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h(px(1.))
                    .bg(hairline),
            )
            .child(header)
            // The rack. The transport floats over the bottom of the panel, so the cards are at
            // the top, clear of it. It scrolls sideways: a card is as wide as its controls,
            // and a narrow window must not cut the last ones off.
            .child(
                div()
                    .id("rack")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_x_scroll()
                    .flex()
                    .items_start()
                    .gap(px(16.))
                    .p(px(24.))
                    .children(cards),
            )
    }
}
