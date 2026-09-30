//! The Wavetable in the window, on a Pad track at the end of the piece, playing a chord:
//!
//! - `wavetable.png`: the card: the wavetable of the first oscillator, its position, the
//!   cutoff and the resonance of the first filter, and the gain.
//! - `wavetable-oscillators.png`: the card expanded, at the start of the rack: the rest of the
//!   first oscillator and the second, with its own wavetable.
//! - `wavetable-envelopes.png`: the same scrolled on to the envelopes, showing Env 2, and the
//!   LFOs, showing a synced LFO 1.
//! - `wavetable-matrix.png`: the same scrolled to the end: the voicing and a matrix of three
//!   routes.

use anyhow::{Context as _, Result};
use arrangement::{Colour, TrackState};
use gpui::{Entity, HeadlessAppContext, PlatformInput, ScrollDelta, ScrollWheelEvent, point, px};
use sound_core::{Changes, FilterType, Instance, LfoShape, Project, Ticks};
use sound_notes::{Division, Feel};
use wavetable::state::{Effect, Oscillator};
use wavetable::view::{EnvelopeShown, WavetableView};
use wavetable::{Destination, Route, Source, Table, WavetableState};

use super::{BAR, HEADER_WIDTH, Opened, WINDOW_HEIGHT, clip, note, piece};

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

/// A Pad track at the end of the piece whose instrument is `sound`, holding a chord.
fn add_pad(project: &mut Project, sound: WavetableState) -> Result<()> {
    let arrangement = runtime::main_arrangement(project).context("no arrangement")?;
    let mut changes = Changes::new();
    let track = arrangement::add_track(
        project,
        &mut changes,
        arrangement.id(),
        "Pad",
        Colour::Sky,
        sound,
    )?;
    let chord = [57, 60, 64, 67]
        .map(|pitch| note(0, 4 * BAR, pitch))
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    changes.create(track.id().child("chord")?, clip(4, 4, chord)?);
    project.commit("Add pad", changes)?;
    Ok(())
}

/// Opens the panel of the last track and gives its card.
fn open_panel(opened: &Opened, cx: &mut HeadlessAppContext) -> Result<Entity<WavetableView>> {
    let view = opened.arrangement_view(cx)?;
    let track = cx.update(|cx| {
        let project = opened.session.read(cx).project();
        let arrangement = runtime::main_arrangement(project).context("no arrangement")?;
        let tracks = project.children::<TrackState>(arrangement.id());
        let last = tracks.max_by_key(|(_, state)| state.order);
        last.map(|(track, _): (Instance<TrackState>, _)| track)
            .context("no track")
    })?;
    cx.update_window(opened.window.into(), |_, window, cx| {
        view.update(cx, |view, cx| view.open_track_panel(track, window, cx));
    })?;
    cx.run_until_parked();
    cx.update(|cx| {
        let panel = view.read(cx).track_panel().cloned().context("no panel")?;
        let card = panel.read(cx).device_views().next().flatten().cloned();
        let card = card.context("no card")?.downcast::<WavetableView>().ok();
        card.context("not a Wavetable")
    })
}

/// Two fingers sideways over the rack, by `by` points: a positive `by` moves it on.
fn scroll_rack(opened: &Opened, by: f32, cx: &mut HeadlessAppContext) -> Result<()> {
    let wheel = PlatformInput::ScrollWheel(ScrollWheelEvent {
        position: point(px(HEADER_WIDTH + 300.), px(WINDOW_HEIGHT - 40.)),
        delta: ScrollDelta::Pixels(point(px(-by), px(0.))),
        ..Default::default()
    });
    opened.mouse(wheel, cx)?;
    Ok(())
}

pub fn snapshots(
    cx: &mut HeadlessAppContext,
    save: &impl Fn(&mut HeadlessAppContext, &Opened, &str) -> Result<()>,
) -> Result<()> {
    let mut opened = Opened::new(cx, |project| {
        piece(project)?;
        add_pad(project, pad())
    })?;
    let card = open_panel(&opened, cx)?;
    opened.play_from(Ticks(4 * BAR + 1920), cx)?;
    opened.listen(0.3, cx)?;
    save(cx, &opened, "wavetable")?;
    cx.update(|cx| {
        card.update(cx, |card, cx| {
            card.set_expanded(true, cx);
            card.show_envelope(EnvelopeShown::Env2, cx);
        })
    });
    cx.run_until_parked();
    save(cx, &opened, "wavetable-oscillators")?;
    scroll_rack(&opened, 1_735., cx)?;
    save(cx, &opened, "wavetable-envelopes")?;
    scroll_rack(&opened, 10_000., cx)?;
    save(cx, &opened, "wavetable-matrix")?;
    Ok(())
}
