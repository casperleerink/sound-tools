//! The time signature control in the transport: two numbers, the beats and the note value. It
//! edits the time signature change in effect at the playhead, as the tempo does the tempo
//! change, and reads it from the project when it renders.

use gpui::{Context, IntoElement, ParentElement, Styled, div, px};
use sound_core::{Changes, TempoMap, TimeSignature};
use sound_ui::components::drag_number::DragNumber;
use sound_ui::components::gesture::ValueChange;
use sound_ui::{ActiveTheme, DragEdit, typography, weak_callback};

use super::TransportPill;

const LABEL: &str = "Change time signature";

/// One number of a time signature, each its own control.
#[derive(Copy, Clone)]
enum Part {
    Beats,
    /// As the power of two of the note value, so each step is the next note value.
    NoteValue,
}

impl Part {
    fn value(self, signature: TimeSignature) -> f64 {
        match self {
            Part::Beats => f64::from(signature.numerator()),
            Part::NoteValue => f64::from(signature.denominator().trailing_zeros()),
        }
    }

    /// From a whole note to a thirty-second, see [`TimeSignature::new`].
    fn range(self) -> (f64, f64) {
        match self {
            Part::Beats => (1., 32.),
            Part::NoteValue => (0., 5.),
        }
    }

    /// `signature` with this number at the whole value nearest `value`. A shift drag moves by
    /// tenths, which round to the same number for a while.
    fn with(self, signature: TimeSignature, value: f64) -> Option<TimeSignature> {
        let (min, max) = self.range();
        let value = value.round().clamp(min, max) as u32;
        let (numerator, denominator) = (signature.numerator(), signature.denominator());
        match self {
            Part::Beats => TimeSignature::new(value, denominator),
            Part::NoteValue => TimeSignature::new(numerator, 1 << value),
        }
        .ok()
    }
}

/// A drag of one number, from its first move to its end, on the change it began on.
pub(super) struct SignatureDrag {
    bar: u64,
    edit: DragEdit,
}

/// The tempo map with the time signature that starts at bar `bar` set by one number. `None`
/// when no change starts there any more, or nothing changes.
fn with_value(tempo_map: &TempoMap, bar: u64, part: Part, value: f64) -> Option<TempoMap> {
    let time_signatures = tempo_map.time_signatures();
    let signature = time_signatures.bar(bar)?.signature;
    let changed = part
        .with(signature, value)
        .filter(|changed| *changed != signature)?;
    let time_signatures = time_signatures.with_signature_at(bar, changed)?;
    Some(tempo_map.with_time_signatures(time_signatures))
}

impl TransportPill {
    /// The first bar of the time signature in effect at the playhead. Read from the project,
    /// never kept.
    fn signature_bar(&self, cx: &gpui::App) -> sound_core::Bar {
        let tick = self.playhead.read(cx).tick;
        let project = self.session.read(cx).project();
        let time_signatures = project.project_file().tempo_map.time_signatures();
        time_signatures.change_at(tick)
    }

    /// A drag of a number is one gesture of the session, so the bars move with it and it is
    /// one undo step; an arrow key is one commit.
    fn on_signature(&mut self, part: Part, change: ValueChange<f64>, cx: &mut Context<Self>) {
        match change {
            ValueChange::Drag(value) => {
                let bar = self.signature_bar(cx).number;
                let drag = self.signature_drag.get_or_insert(SignatureDrag {
                    bar,
                    edit: DragEdit::default(),
                });
                let bar = drag.bar;
                drag.edit
                    .publish(&self.session, LABEL, cx, |project, edit| {
                        let live = &project.project_file().tempo_map;
                        let Some(tempo_map) = with_value(live, bar, part, value) else {
                            return Ok(());
                        };
                        let mut changes = Changes::new();
                        changes.set_tempo_map(tempo_map);
                        project.publish(edit, changes)
                    });
            }
            ValueChange::DragEnd => {
                if let Some(mut drag) = self.signature_drag.take() {
                    drag.edit.finish(&self.session, cx);
                }
            }
            ValueChange::DragCancel => {
                if let Some(mut drag) = self.signature_drag.take() {
                    drag.edit.cancel(&self.session, cx);
                }
            }
            ValueChange::Set(value) => {
                let bar = self.signature_bar(cx).number;
                self.session.update(cx, |session, cx| {
                    session.edit(cx, |project| {
                        let live = &project.project_file().tempo_map;
                        let Some(tempo_map) = with_value(live, bar, part, value) else {
                            return Ok(());
                        };
                        let mut changes = Changes::new();
                        changes.set_tempo_map(tempo_map);
                        project.commit(LABEL, changes)
                    })
                });
            }
        }
    }

    /// The time signature at the playhead, `7/8`: the beats and the note value each a number
    /// that a drag and the arrows change.
    pub(super) fn time_signature(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().gray_700;
        let signature = self.signature_bar(cx).signature;
        let number = |part: Part, id: &'static str, text: u32, cx: &mut Context<Self>| {
            let (min, max) = part.range();
            DragNumber::new(id, part.value(signature), min, max)
                .drag(0.1, 1.)
                .keys(1., 1.)
                .on_change(weak_callback(cx, move |pill: &mut Self, change, cx| {
                    pill.on_signature(part, change, cx)
                }))
                .child(div().font(typography::tabular()).child(text.to_string()))
        };
        div()
            .flex()
            .items_baseline()
            .child(number(Part::Beats, "beats", signature.numerator(), cx))
            .child(div().text_color(muted).text_size(px(12.)).child("/"))
            .child(number(
                Part::NoteValue,
                "note-value",
                signature.denominator(),
                cx,
            ))
    }
}

#[cfg(test)]
mod tests {
    use sound_core::{Tempo, TempoMap, TimeSignatures};

    use super::*;

    /// The note value steps through the powers of two, and each number keeps the other.
    #[test]
    fn each_number_changes_only_itself_and_stays_a_time_signature() {
        let seven_eight = TimeSignature::new(7, 8).unwrap();
        let with = |part: Part, value| part.with(seven_eight, value).unwrap().to_string();
        assert_eq!(with(Part::NoteValue, 4.), "7/16");
        assert_eq!(with(Part::NoteValue, 2.4), "7/4");
        assert_eq!(with(Part::NoteValue, 9.), "7/32");
        assert_eq!(with(Part::Beats, 5.), "5/8");
        assert_eq!(with(Part::Beats, 0.), "1/8");

        let map = TempoMap::constant(TimeSignatures::constant(seven_eight), Tempo::default());
        let changed = with_value(&map, 1, Part::Beats, 3.).unwrap();
        assert_eq!(changed.time_signatures().first().to_string(), "3/8");
        assert_eq!(changed.tempo_changes(), map.tempo_changes());
        // Nothing to change, and no change at a bar where none starts.
        assert_eq!(with_value(&map, 1, Part::Beats, 7.), None);
        assert_eq!(with_value(&map, 2, Part::Beats, 3.), None);
    }
}
