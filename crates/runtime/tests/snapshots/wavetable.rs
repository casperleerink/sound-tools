//! The Wavetable in the window, the instrument of a rack, playing a chord:
//!
//! - `wavetable.png`: the card: the wavetable of the first oscillator, its position, the
//!   cutoff and the resonance of the first filter, and the gain.
//! - `wavetable-oscillators.png`: the card expanded on its Osc page: the rest of the first
//!   oscillator and the second, with its own wavetable. Dots mark every page but Voice.
//! - `wavetable-voice.png`: the Voice page: the sub, the unison and the voicing.
//! - `wavetable-filters.png`: the Filter page: both filters, the second a warm high pass.
//! - `wavetable-envelopes.png`: the Env page, showing Env 2.
//! - `wavetable-lfos.png`: the LFO page, showing a synced LFO 1.
//! - `wavetable-matrix.png`: the Matrix page, with three routes.

use anyhow::Result;
use gpui::HeadlessAppContext;
use sound_core::Ticks;
use sound_dsp::{FilterCharacter, FilterType, LfoShape};
use sound_notes::{Division, Feel};
use wavetable::state::{Effect, Oscillator};
use wavetable::view::{EnvelopeShown, Page, WavetableView};
use wavetable::{Destination, Route, Source, Table, WavetableState};

use super::{Opened, Save};
use crate::rack::{notes, rack_files};

/// A slow pad: vowels that an LFO morphs, a folded second oscillator an octave up, a filter
/// the second envelope opens, and three routes.
fn pad() -> WavetableState {
    let default = WavetableState::default();
    let route = |source, destination, amount| Route {
        source,
        destination,
        amount,
    };
    WavetableState {
        osc_1: Oscillator {
            table: Table::Vowels,
            position: 0.35,
            ..default.osc_1
        },
        osc_2: Oscillator {
            table: Table::Harmonics,
            position: 0.7,
            effect: Effect::Fold,
            effect_amount: 0.3,
            octave: 1,
            gain: 0.45,
            ..default.osc_2
        },
        filter_1: wavetable::state::Filter {
            cutoff_hz: 1_800.,
            resonance: 0.35,
            ..default.filter_1
        },
        filter_2: wavetable::state::Filter {
            on: true,
            kind: FilterType::HighPass,
            cutoff_hz: 180.,
            character: FilterCharacter::Warm,
            ..default.filter_2
        },
        amp_env: wavetable::state::Adsr {
            attack_seconds: 0.4,
            release_seconds: 1.2,
            ..default.amp_env
        },
        env_2: wavetable::state::Adsr {
            attack_seconds: 0.02,
            decay_seconds: 1.5,
            sustain: 0.3,
            decay_curve: 0.9,
            ..default.env_2
        },
        lfo_1: wavetable::state::LfoSettings {
            shape: LfoShape::Triangle,
            sync: true,
            division: Division::Half,
            feel: Feel::Dotted,
            ..default.lfo_1
        },
        matrix: vec![
            route(Source::Lfo1, Destination::Osc1Position, 0.4),
            route(Source::Env2, Destination::Filter1Cutoff, 0.5),
            route(Source::Velocity, Destination::AmpLevel, -0.25),
        ],
        ..default
    }
}

pub(crate) fn snapshots(cx: &mut HeadlessAppContext, save: &Save) -> Result<()> {
    let sound = serde_json::json!({ "tool": "wavetable", "state": pad() }).to_string();
    let chord = notes(
        4 * 3840,
        &[
            (0, 15360, 57),
            (0, 15360, 60),
            (0, 15360, 64),
            (0, 15360, 67),
        ],
    );
    let children = [("instrument", sound.as_str()), ("chord", chord.as_str())];
    let mut opened = Opened::open(cx, &rack_files(&children))?;
    let card = opened.card::<WavetableView>("rack/instrument", cx)?;
    opened.play_from(Ticks(1920), cx)?;
    save(cx, opened.window.into(), "wavetable")?;
    cx.update(|cx| {
        card.update(cx, |card, cx| {
            card.set_expanded(true, cx);
            card.show_envelope(EnvelopeShown::Env2, cx);
        })
    });
    for (page, name) in [
        (Page::Osc, "wavetable-oscillators"),
        (Page::Voice, "wavetable-voice"),
        (Page::Filter, "wavetable-filters"),
        (Page::Env, "wavetable-envelopes"),
        (Page::Lfo, "wavetable-lfos"),
        (Page::Matrix, "wavetable-matrix"),
    ] {
        cx.update(|cx| card.update(cx, |card, cx| card.show_page(page, cx)));
        cx.run_until_parked();
        save(cx, opened.window.into(), name)?;
    }
    Ok(())
}
