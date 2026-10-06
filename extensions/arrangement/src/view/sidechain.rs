//! The sidechain picker of an effect card: which track keys the effect, and where on that track
//! the sound is taken. The rack makes it, because the tracks and the slot record are its own,
//! and hands it to the view of the effect in the frame of the card.
//!
//! The source select says `Off`, or lists the tracks by name, the track of the effect too. The
//! tap select shows once a track is picked. Both are controlled: they read the slot when the
//! card draws, and a pick is one undo step of the panel.

use gpui::{App, Entity, SharedString, WeakEntity, Window};
use sound_core::{Instance, InstanceId};
use sound_ui::Session;
use sound_ui::components::cell::{CELL_WIDTH, Cell};
use sound_ui::components::device_card::Column;
use sound_ui::components::dropdown_menu::{MenuEntry, MenuGroup, MenuItem};
use sound_ui::components::select::Select;

use super::track_panel::TrackPanel;
use crate::{Sidechain, Tap, TrackState};

/// The row of no sidechain. A folder name is lowercase, so no track has this value.
const OFF: &str = "Off";

/// Each tap and what the select says, which is also its value.
const TAPS: [(Tap, &str); 3] = [
    (Tap::PreFx, "Pre FX"),
    (Tap::PostFx, "Post FX"),
    (Tap::PostMixer, "Post mixer"),
];

/// Two cells wide, for the name of a track.
const SPAN: usize = 2;

/// A select of the picker in a cell of two columns, with its label under it.
fn cell(select: Select, label: &'static str) -> Cell {
    let select = select.trigger_width(SPAN as f32 * CELL_WIDTH - 8.);
    Cell::new(select).span(SPAN).label(label)
}

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
        let own = track.id().name().to_string();
        let pick_source = {
            let (panel, slot) = (panel.clone(), slot.clone());
            move |picked: SharedString, _: &mut Window, cx: &mut App| {
                let source = (picked != OFF).then(|| picked.to_string());
                panel
                    .update(cx, |panel, cx| {
                        panel.edit_slot(&slot, cx, |effect| {
                            let label = match (&effect.sidechain, &source) {
                                (_, None) => "Turn off sidechain",
                                (None, Some(_)) => "Turn on sidechain",
                                (Some(_), Some(_)) => "Change sidechain",
                            };
                            // Its own track keys it only before its effects, else it would
                            // key itself. Another track keeps the tap, or starts after its
                            // effects.
                            let kept = effect.sidechain.as_ref().map(|key| key.tap);
                            effect.sidechain = source.map(|track| Sidechain {
                                tap: match track == own {
                                    true => Tap::PreFx,
                                    false => kept.unwrap_or(Tap::PostFx),
                                },
                                track,
                            });
                            label.into()
                        })
                    })
                    .ok();
            }
        };
        let source = Select::new("sidechain", source.clone())
            .entries(vec![MenuEntry::Group(MenuGroup::new().items(rows))])
            // A track that is gone still shows by the name the record has. Its problem says why.
            .placeholder(source)
            .on_change(pick_source);
        let column = Column::new().span(SPAN).top(cell(source, "Sidechain"));
        let Some(keyed) = keyed else {
            return column;
        };
        let rows = TAPS.map(|(_, label)| MenuItem::new(label, label));
        let tap = TAPS
            .iter()
            .find(|(tap, _)| *tap == keyed.tap)
            .map_or("", |(_, label)| *label);
        let pick_tap = {
            let (panel, slot) = (panel.clone(), slot.clone());
            move |picked: SharedString, _: &mut Window, cx: &mut App| {
                let Some((tap, _)) = TAPS.iter().find(|(_, label)| *label == picked.as_ref())
                else {
                    return;
                };
                panel
                    .update(cx, |panel, cx| {
                        panel.edit_slot(&slot, cx, |effect| {
                            if let Some(key) = &mut effect.sidechain {
                                key.tap = *tap;
                            }
                            "Change sidechain tap".into()
                        })
                    })
                    .ok();
            }
        };
        let tap = Select::new("sidechain-tap", tap)
            .entries(vec![MenuEntry::Group(MenuGroup::new().items(rows))])
            .on_change(pick_tap);
        column.bottom(cell(tap, "Tap"))
    }
}
