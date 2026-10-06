//! The sidechain picker of an effect card: which track keys the effect, and where on that track
//! the sound is taken. The rack makes it, because the tracks and the slot record are its own,
//! and hands it to the view of the effect in the frame of the card.
//!
//! The source select says `Off`, or lists the tracks by name, the track of the effect too: its
//! sound before its effects keys them without a loop. The tap select shows once a track is
//! picked. Both are controlled: they read the slot when the card draws, and a pick is one undo
//! step of the panel.

use gpui::{App, Entity, SharedString, WeakEntity};
use sound_core::{Instance, InstanceId};
use sound_ui::Session;
use sound_ui::components::cell::{CELL_WIDTH, Cell};
use sound_ui::components::device_card::Column;
use sound_ui::components::dropdown_menu::{MenuEntry, MenuGroup, MenuItem};
use sound_ui::components::select::Select;

use super::track_panel::TrackPanel;
use crate::{Tap, TrackState};

/// The row of no sidechain. A folder name is lowercase, so no track has this value.
const OFF: &str = "Off";

/// Each tap, its value in the select and what the select says.
const TAPS: [(Tap, &str, &str); 3] = [
    (Tap::PreFx, "pre_fx", "Pre FX"),
    (Tap::PostFx, "post_fx", "Post FX"),
    (Tap::PostMixer, "post_mixer", "Post mixer"),
];

/// Two cells wide, for the name of a track.
const SPAN: usize = 2;
const TRIGGER_WIDTH: f32 = SPAN as f32 * CELL_WIDTH - 8.;
const MENU_WIDTH: f32 = 200.;

/// The column of the picker of the effect in `slot` on `track`, made when the card draws.
pub(super) fn column(
    session: Entity<Session>,
    panel: WeakEntity<TrackPanel>,
    track: Instance<TrackState>,
    slot: InstanceId,
) -> impl Fn(&App) -> Column + 'static {
    move |cx| {
        let project = session.read(cx).project();
        let keyed = project
            .state(&track)
            .and_then(|state| {
                state
                    .effects
                    .iter()
                    .find(|effect| effect.name == slot.name())
            })
            .and_then(|effect| effect.sidechain.clone());
        let tracks = track
            .id()
            .parent()
            .map(|arrangement| crate::tracks(project, &arrangement))
            .unwrap_or_default();
        let rows = tracks
            .iter()
            .map(|(track, state)| MenuItem::new(track.id().name().to_string(), state.name.clone()));
        let rows = std::iter::once(MenuItem::new(OFF, OFF)).chain(rows);
        let source = match &keyed {
            Some(keyed) => SharedString::from(keyed.track.clone()),
            None => OFF.into(),
        };
        let pick_source = {
            let (panel, slot) = (panel.clone(), slot.clone());
            move |picked: SharedString, _: &mut gpui::Window, cx: &mut App| {
                let source = (picked != OFF).then(|| picked.to_string());
                panel
                    .update(cx, |panel, cx| panel.key_effect(&slot, source, cx))
                    .ok();
            }
        };
        let source = Select::new("sidechain", source.clone())
            .entries(vec![MenuEntry::Group(MenuGroup::new().items(rows))])
            // A track that is gone still shows by the name the record has. Its problem says why.
            .placeholder(source)
            .trigger_width(TRIGGER_WIDTH)
            .menu_width(MENU_WIDTH)
            .on_change(pick_source);
        let column = Column::new()
            .span(SPAN)
            .top(Cell::new(source).span(SPAN).label("Sidechain"));
        let Some(keyed) = keyed else {
            return column;
        };
        let rows = TAPS.map(|(_, value, label)| MenuItem::new(value, label));
        let tap = TAPS
            .iter()
            .find(|(tap, ..)| *tap == keyed.tap)
            .map_or("", |(_, value, _)| *value);
        let pick_tap = {
            let (panel, slot) = (panel.clone(), slot.clone());
            move |picked: SharedString, _: &mut gpui::Window, cx: &mut App| {
                let Some((tap, ..)) = TAPS.iter().find(|(_, value, _)| *value == picked.as_ref())
                else {
                    return;
                };
                panel
                    .update(cx, |panel, cx| panel.set_tap(&slot, *tap, cx))
                    .ok();
            }
        };
        let tap = Select::new("sidechain-tap", tap)
            .entries(vec![MenuEntry::Group(MenuGroup::new().items(rows))])
            .trigger_width(TRIGGER_WIDTH)
            .menu_width(MENU_WIDTH)
            .on_change(pick_tap);
        column.bottom(Cell::new(tap).span(SPAN).label("Tap"))
    }
}
