//! Audio section: the waveform display of the Clip card and the Sampler, as in `sampler.png`.

use std::sync::Arc;

use gpui::{AnyElement, App, Window, div, point, prelude::*, px};
use sound_media::{Audio, Overview};
use sound_ui::ActiveTheme;
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::device_card::{Column, DeviceCard, NARROW_CARD_WIDTH};
use sound_ui::components::display::{Axis, Handle};
use sound_ui::components::knob::{Knob, KnobRange};
use sound_ui::components::waveform_display::{FileDrop, NoFile, WaveformDisplay, place};

use super::rack::{block, sample};

/// Something like a voice: syllables in phrases, loud to quiet, as a peak from 0 to 1.
fn voice(time: f32) -> f32 {
    let phrase = (time * 0.5).fract();
    let open = if phrase < 0.72 { 1. } else { 0.02 };
    let syllable = (time * std::f32::consts::PI * 4.3).sin().abs().powf(1.5);
    let swell = 0.35 + 0.65 * (time * 1.3).sin().abs();
    (syllable * swell * open).max(0.02)
}

/// Fourteen and a half seconds of the voice as a WAV file, and its overview.
fn overview() -> Option<Arc<Overview>> {
    let rate = 48_000_u32;
    let frames = (14.6 * rate as f32) as u32;
    let data = frames * 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for frame in 0..frames {
        let time = frame as f32 / rate as f32;
        let tone = (time * 2. * std::f32::consts::PI * 180.).sin();
        let sample = (tone * voice(time) * 30_000.) as i16;
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    let audio = Audio::parse(bytes).ok()?;
    Some(Arc::new(Overview::of(&audio)))
}

/// The display of the Clip card: the whole file, trimmed from 2.1 s to 10.1 s, the gain line
/// with its fades, and the green line of the playhead.
fn clip_display(overview: Option<Arc<Overview>>) -> WaveformDisplay {
    let (seconds, start, end) = (14.6, 2.1, 10.1);
    let across = |time: f32| place(time, seconds);
    let up = KnobRange::linear(-48., 24.).position(-0.9);
    let (fade_in, fade_out) = (start + 0.15, end - 0.26);
    let handle = |name: &'static str, x: f32, hollow| {
        Handle::new(name, Axis::fixed(x), Axis::fixed(up)).hollow(hollow)
    };
    WaveformDisplay::new("clip-display", 312., overview, seconds)
        .trim(start, end)
        .on_start(|_, _, _| {})
        .on_end(|_, _, _| {})
        .playhead(Some(5.3))
        .curve([
            point(across(start), 0.),
            point(across(fade_in), up),
            point(across(fade_out), up),
            point(across(end), 0.),
        ])
        .handle(handle("fade-in", across(fade_in), false))
        .handle(handle(
            "gain",
            (across(fade_in) + across(fade_out)) / 2.,
            true,
        ))
        .handle(handle("fade-out", across(fade_out), false))
        .caption("2.1 s to 10.1 s of 14.6 s")
}

fn knob(id: &'static str, label: &'static str, value: f32, readout: &'static str) -> Knob {
    Knob::new(id)
        .range(KnobRange::linear(0., 1.))
        .value(value)
        .label(label)
        .readout(readout)
}

fn displays(cx: &App) -> AnyElement {
    let overview = overview();
    let title = |text: &'static str| div().child(text);
    let card = DeviceCard::new("clip-card", title("voice-take-3.wav"))
        .expand(true, |_, _, _| {})
        .display(clip_display(overview))
        .column(
            Column::new()
                .top(knob("gain", "Gain", 0.65, "-0.9 dB"))
                .bottom(knob("fade-in", "Fade in", 0.04, "150 ms")),
        )
        .column(Column::new().bottom(knob("fade-out", "Fade out", 0.06, "260 ms")))
        .hidden_column(
            Column::new()
                .top(knob("start", "Start", 0.14, "2.1 s"))
                .bottom(knob("end", "End", 0.69, "10.1 s")),
        );
    let empty = DeviceCard::new("clip-card-empty", title("Clip"))
        .w(px(NARROW_CARD_WIDTH))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .text_size(px(12.))
                .line_height(px(14.))
                .text_color(cx.theme().gray_800)
                .child("Select a clip."),
        );
    let waiting = WaveformDisplay::new("waiting", 312., None, 14.6)
        .trim(2.1, 10.1)
        .caption("2.1 s to 10.1 s of 14.6 s");
    block(
        "Waveform display, and the Clip card",
        cx,
        [
            sample("the Clip card, expanded", cx, card),
            sample("no clip selected", cx, empty),
            sample("while the overview is made", cx, waiting),
        ],
    )
}

/// A kalimba-like tone of 1.4 s: a tine dying away, as a peak from 0 to 1.
fn kalimba_overview() -> Option<Arc<Overview>> {
    let rate = 48_000_u32;
    let frames = (1.4 * rate as f32) as u32;
    let data = frames * 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for frame in 0..frames {
        let time = frame as f32 / rate as f32;
        let tine = (time * 2. * std::f32::consts::PI * 523.25).sin() * (-2.6 * time).exp();
        bytes.extend_from_slice(&((tine * 24_000.) as i16).to_le_bytes());
    }
    let audio = Audio::parse(bytes).ok()?;
    Some(Arc::new(Overview::of(&audio)))
}

/// The display of the Sampler: the whole file, trimmed from 12 ms to 1.18 s, the envelope over
/// it in the time of the file with the attack peak and the decay corner, and the green line.
fn sampler_display(overview: Option<Arc<Overview>>) -> WaveformDisplay {
    let (seconds, start, end) = (1.4, 0.012, 1.18);
    let (attack, decay, sustain, top) = (0.002, 0.4, 0.55, 0.88);
    let across = |time: f32| place(time, seconds);
    let handle =
        |name: &'static str, x: f32, y: f32| Handle::new(name, Axis::fixed(x), Axis::fixed(y));
    WaveformDisplay::new("sampler-display", 312., overview, seconds)
        .trim(start, end)
        .on_start(|_, _, _| {})
        .on_end(|_, _, _| {})
        .playhead(Some(0.16))
        .curve([
            point(across(start), 0.),
            point(across(start + attack), top),
            point(across(start + attack + decay), sustain * top),
            point(across(end), sustain * top),
        ])
        .handle(handle("attack", across(start + attack), top))
        .handle(handle(
            "decay",
            across(start + attack + decay),
            sustain * top,
        ))
        .caption("kalimba.wav · A 2 ms · D 400 ms · S 55%")
}

fn choose_file() -> Button {
    Button::new("choose-file", "Choose file")
        .variant(ButtonVariant::Subtle)
        .size(ButtonSize::Sm)
}

fn sampler(cx: &App) -> AnyElement {
    let overview = kalimba_overview();
    let title = |text: &'static str| div().child(text);
    let knobs = |card: DeviceCard| {
        card.column(
            Column::new()
                .top(knob("root", "Root", 0.47, "C4"))
                .bottom(knob("release", "Release", 0.62, "300 ms")),
        )
        .column(
            Column::new()
                .top(knob("velocity", "Velocity", 0.5, "50%"))
                .bottom(knob("gain", "Gain", 0.67, "0 dB")),
        )
    };
    let playing = knobs(
        DeviceCard::new("sampler-playing", title("Sampler"))
            .expand(false, |_, _, _| {})
            .display(sampler_display(overview.clone())),
    );
    let expanded = knobs(
        DeviceCard::new("sampler-expanded", title("Sampler"))
            .expand(true, |_, _, _| {})
            .display(sampler_display(overview.clone())),
    )
    .hidden_column(
        Column::new()
            .top(knob("start", "Start", 0.01, "12 ms"))
            .bottom(knob("end", "End", 0.84, "1.18 s")),
    )
    .hidden_column(
        Column::new()
            .top(knob("attack", "Attack", 0.12, "2 ms"))
            .bottom(knob("decay", "Decay", 0.64, "400 ms")),
    )
    .hidden_column(Column::new().top(knob("sustain", "Sustain", 0.55, "55%")));
    let empty = knobs(
        DeviceCard::new("sampler-empty", title("Sampler"))
            .expand(false, |_, _, _| {})
            .display(
                NoFile::new("empty-display", 312., "Drop an audio file here").button(choose_file()),
            ),
    );
    let dragged = knobs(
        DeviceCard::new("sampler-dragged", title("Sampler"))
            .expand(false, |_, _, _| {})
            .display(
                NoFile::new("dragged-display", 312., "Drop an audio file here")
                    .button(choose_file())
                    .drop_file(FileDrop::new("Drop to load the file", |_, _, _| {}).shown(true)),
            ),
    );
    let replace = sampler_display(overview)
        .drop_file(FileDrop::new("Drop to replace the file", |_, _, _| {}).shown(true));
    block(
        "The Sampler",
        cx,
        [
            sample("playing: the green line is the last note", cx, playing),
            sample("expanded", cx, expanded),
            sample("empty: Choose file is the way from the keys", cx, empty),
            sample("a file dragged over an empty one", cx, dragged),
            sample("a file dragged over one with a file", cx, replace),
        ],
    )
}

pub fn section(_: &mut Window, cx: &mut App) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(48.))
        .child(displays(cx))
        .child(sampler(cx))
}
