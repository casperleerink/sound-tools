//! The DAW song page of SPEC.md in Rune. What moves every frame (playhead, time, meters, scroll)
//! is a `Live` value the renderer reads without running the view; the view runs again only when
//! the zoom, the visible page of the timeline or the song changes.

use std::cell::Cell;
use std::time::Duration;

use daw_model::{
    self as model, ACROSS, BAR, BEAT, HEADER, LEAD, MIXER, Options, PITCHES, ROW, RULER, STRIP, Scenario,
    Song, TRANSPORT, Track, WINDOW,
};
use rune::color::hex;
use rune::{AppSpec, Color, Cx, El, LineCap, Live, LiveText, ScrollSpec, Shape, TextStyle, Tx, canvas, col, el, memo, row, text};

/// How far the timeline is built past each edge of the window, so a scroll rebuilds once a page.
const PAGE: f32 = 1200.0;
const METER: f32 = 140.0;
const FADER_TOP: f32 = 6.0;

struct State {
    options: Options,
    song: Song,
    end: u64,
    zoom: f32,
    /// Which page of the timeline is built: `scroll_x / PAGE`.
    page: i64,
    edits: u64,
    playing: bool,
    frames: u64,
    /// When the last update was: updates are held to 60 a second, as the GPUI app's poll timer,
    /// since a `Live` set wakes the next display frame (120 Hz here).
    last: f64,
    builds: Cell<u64>,
    scroll: Live<[f32; 2]>,
    playhead: Live<[f32; 2]>,
    time: LiveText,
    /// Per track and then the master: the header's meter, the mixer's left and right.
    meters: Vec<[Live<[f32; 4]>; 3]>,
}

fn init() -> State {
    let options = Options::parse(rune::extra_args().iter().cloned());
    let song = model::song(options.tracks, options.clips);
    let end = song.end();
    let meters = (0..=options.tracks).map(|_| std::array::from_fn(|_| Live::new([0.0; 4]))).collect();
    State {
        options,
        song,
        end,
        zoom: ACROSS,
        page: 0,
        edits: 0,
        playing: options.scenario.playing(),
        frames: 0,
        last: f64::NEG_INFINITY,
        builds: Cell::new(0),
        scroll: Live::new([0.0; 2]),
        playhead: Live::new([LEAD, 0.0]),
        time: LiveText::new(model::position(0)),
        meters,
    }
}

fn poll(s: &mut State, seconds: f64, tx: &mut Tx) -> bool {
    const INTERVAL: f64 = 1.0 / 60.0;
    if s.options.scenario != Scenario::Idle && seconds - s.last < INTERVAL * 0.95 {
        tx.wake_after(Duration::from_secs_f64(INTERVAL - (seconds - s.last)));
        return false;
    }
    s.last = seconds;
    s.frames += 1;
    let m = model::moment(s.options.scenario, seconds, s.end);
    let mut rebuild = false;
    if m.zoom != s.zoom {
        s.zoom = m.zoom;
        rebuild = true;
    }
    let page = (m.scroll_x / PAGE).floor() as i64;
    if page != s.page {
        s.page = page;
        rebuild = true;
    }
    if s.options.scenario == Scenario::Edit {
        while s.edits < model::edits(seconds) {
            model::apply_edit(&mut s.song, s.edits);
            s.edits += 1;
            rebuild = true;
        }
    }
    s.scroll.set([-m.scroll_x, 0.0]);
    s.playhead.set([LEAD + m.playhead as f32 * m.zoom - m.scroll_x, 0.0]);
    s.time.set(model::position(m.playhead));
    if s.playing {
        for (index, [header, left, right]) in s.meters.iter().enumerate() {
            header.set(meter_rect(model::meter(index, seconds), 4.0, ROW - 8.0));
            left.set(meter_rect(model::meter(index, seconds), 4.0, METER));
            right.set(meter_rect(model::meter(index, seconds + 0.05), 4.0, METER));
        }
    }
    match s.options.seconds {
        Some(limit) if seconds >= limit.as_secs_f64() => {
            println!(
                "{{\"app\":\"rune\",\"scenario\":\"{}\",\"tracks\":{},\"clips\":{},\"seconds\":{seconds:.2},\"frames\":{},\"view_renders\":{}}}",
                s.options.scenario.name(),
                s.options.tracks,
                s.options.clips,
                s.frames,
                s.builds.get()
            );
            tx.quit();
        }
        Some(limit) if s.options.scenario == Scenario::Idle => {
            tx.wake_after(limit.saturating_sub(Duration::from_secs_f64(seconds)));
        }
        _ if s.options.scenario != Scenario::Idle => tx.wake_after(Duration::from_millis(16)),
        _ => {}
    }
    tx.instant();
    rebuild
}

/// A vertical meter's fill, from the bottom of a `width` by `height` well.
fn meter_rect(level: f32, width: f32, height: f32) -> [f32; 4] {
    let filled = level.clamp(0.0, 1.0) * height;
    [0.0, height - filled, width, filled]
}

fn colour(rgb: u32) -> Color {
    hex(&format!("#{rgb:06x}")).unwrap_or(Color { r: 1.0, g: 0.0, b: 1.0, a: 1.0 })
}

struct Palette {
    bg: Color,
    panel: Color,
    line: Color,
    text: Color,
    dim: Color,
    well: Color,
    accent: Color,
}

fn palette() -> Palette {
    Palette {
    bg: colour(0x16171a),
    panel: colour(0x1e2024),
    line: colour(0x2c2f35),
    text: colour(0xe6e7ea),
    dim: colour(0x8b8f98),
    well: colour(0x0f1012),
    accent: colour(0x7cc7ff),
    }
}

const LABEL: TextStyle = TextStyle::new(12.0, 500.0);
const SMALL: TextStyle = TextStyle::new(10.0, 500.0);

fn view(s: &State, _: &mut Cx<State>) -> El {
    s.builds.set(s.builds.get() + 1);
    let p = palette();
    let timeline_h = WINDOW.1 - TRANSPORT - RULER - MIXER;
    let tracks = col()
        .w_full()
        .h(timeline_h)
        .scroll(ScrollSpec { stick_bottom: false })
        .child(col().w_full().children(s.song.tracks.iter().enumerate().map(|(index, track)| track_row(s, index, track, &p))));
    let playhead = el()
        .key("playhead-layer")
        .pin(Some(HEADER), Some(TRANSPORT), Some(0.0), Some(MIXER))
        .clip_soft(0.0)
        .inert()
        .child(el().key("playhead").pin(Some(0.0), Some(0.0), None, Some(0.0)).w(1.0).fill(p.accent).live_offset(&s.playhead));
    el().w_full()
        .h_full()
        .fill(p.bg)
        .child(
            col().w_full().h_full().child(transport(s, &p)).child(ruler(s, &p)).child(tracks).child(mixer(s, &p)),
        )
        .child(playhead)
}

fn transport(s: &State, p: &Palette) -> El {
    let play = el()
        .key("play")
        .role("button")
        .label("Play")
        .size(28.0, 28.0)
        .radius(6.0)
        .fill(if s.playing { p.accent } else { p.line })
        .on_press(|s: &mut State| s.playing = !s.playing);
    row()
        .w_full()
        .h(TRANSPORT)
        .px(12.0)
        .gap(16.0)
        .center_y()
        .fill(p.panel)
        .child(play)
        .child(
            text(model::position(0))
                .key("time")
                .live_text(&s.time)
                .w(90.0)
                .font(TextStyle::new(14.0, 500.0).features("'tnum' 1"))
                .color(p.text),
        )
        .child(text("120 bpm").font(LABEL).color(p.dim))
}

/// The bars from the built page's start to its end, in ticks.
fn built_ticks(s: &State) -> (u64, u64) {
    let left = (s.page as f32 * PAGE - PAGE).max(0.0);
    let right = s.page as f32 * PAGE + WINDOW.0 + PAGE;
    let tick = |x: f32| ((x - LEAD).max(0.0) / s.zoom) as u64;
    (tick(left) / BAR * BAR, tick(right).div_ceil(BAR) * BAR)
}

fn ruler(s: &State, p: &Palette) -> El {
    let (from, to) = built_ticks(s);
    let marks = memo("ruler", (s.zoom.to_bits(), from, to), || {
        let mut shape = Shape::new();
        let mut labels = Vec::new();
        let mut tick = from;
        while tick <= to {
            let x = LEAD + tick as f32 * s.zoom;
            let bar = tick % BAR == 0;
            shape = shape.line([x, if bar { 8.0 } else { 18.0 }], [x, RULER], 1.0, LineCap::Butt, p.line);
            if bar {
                labels.push(text(format!("{}", tick / BAR + 1)).font(SMALL).color(p.dim).absolute(x + 4.0, 2.0));
            }
            tick += BEAT;
        }
        let width = LEAD + to as f32 * s.zoom;
        el().size(width, RULER).child(canvas(shape).size(width, RULER).absolute(0.0, 0.0)).children(labels)
    });
    row()
        .w_full()
        .h(RULER)
        .fill(p.panel)
        .child(el().w(HEADER).h(RULER))
        .child(el().grow(1.0).h(RULER).clip_soft(0.0).child(el().absolute(0.0, 0.0).live_offset(&s.scroll).child(marks)))
}

fn track_row(s: &State, index: usize, track: &Track, p: &Palette) -> El {
    let header = memo(format!("head-{index}"), (track.mute, track.solo, track.armed), || header(s, index, track, p));
    let (from, to) = built_ticks(s);
    let starts: Vec<u64> = track.clips.iter().map(|c| c.start).collect();
    let lane = memo(format!("lane-{index}"), (starts, s.zoom.to_bits(), from, to), || lane(s, track, from, to, p));
    row()
        .w_full()
        .h(ROW)
        .border_sides([0.0, 0.0, 1.0, 0.0], p.line)
        .child(header)
        .child(el().grow(1.0).h(ROW).clip_soft(0.0).child(el().absolute(0.0, 0.0).live_offset(&s.scroll).child(lane)))
}

fn toggle(key: String, label: &'static str, on: bool, lit: Color, p: &Palette, press: impl Fn(&mut Track) + 'static) -> El {
    let index: usize = key.rsplit('-').next().and_then(|i| i.parse().ok()).unwrap_or(0);
    el().key(key)
        .role("button")
        .label(label)
        .size(20.0, 18.0)
        .radius(4.0)
        .center()
        .fill(if on { lit } else { p.line })
        .child(text(label).font(SMALL).color(if on { p.well } else { p.dim }))
        .on_press(move |s: &mut State| {
            if let Some(track) = s.song.tracks.get_mut(index) {
                press(track);
            }
        })
}

fn header(s: &State, index: usize, track: &Track, p: &Palette) -> El {
    let well = el().size(4.0, ROW - 8.0).radius(2.0).fill(p.well).child(
        el().pin(Some(0.0), Some(0.0), None, None)
            .size(4.0, ROW - 8.0)
            .radius(2.0)
            .fill(colour(track.colour))
            .live_rect(&s.meters[index][0]),
    );
    row()
        .w(HEADER)
        .h(ROW)
        .fill(p.panel)
        .border_sides([0.0, 1.0, 0.0, 0.0], p.line)
        .child(el().w(4.0).h(ROW).fill(colour(track.colour)))
        .child(
            col()
                .grow(1.0)
                .padding(8.0)
                .gap(8.0)
                .child(text(track.name.clone()).font(LABEL).color(p.text))
                .child(
                    row()
                        .gap(4.0)
                        .child(toggle(format!("mute-{index}"), "M", track.mute, colour(0xf0b44c), p, |t| t.mute = !t.mute))
                        .child(toggle(format!("solo-{index}"), "S", track.solo, colour(0x7ee0a0), p, |t| t.solo = !t.solo))
                        .child(toggle(format!("arm-{index}"), "R", track.armed, colour(0xff6b6b), p, |t| t.armed = !t.armed)),
                ),
        )
        .child(el().padding(4.0).child(well))
}

fn lane(s: &State, track: &Track, from: u64, to: u64, p: &Palette) -> El {
    let width = LEAD + to as f32 * s.zoom;
    let mut bars = Shape::new();
    let mut tick = from;
    while tick <= to {
        let x = LEAD + tick as f32 * s.zoom;
        bars = bars.line([x, 0.0], [x, ROW], 1.0, LineCap::Butt, p.line.alpha(0.5));
        tick += BAR;
    }
    let tint = colour(track.colour);
    let (low, high) = (PITCHES.0 as f32, PITCHES.1 as f32);
    let clips = (track.clips.iter())
        .filter(|c| c.start + c.length >= from && c.start <= to)
        .map(|clip| {
            let w = clip.length as f32 * s.zoom;
            let h = ROW - 6.0;
            let mut notes = Shape::new();
            for note in &clip.notes {
                let x = note.start as f32 * s.zoom;
                let y = 16.0 + (1.0 - (note.pitch as f32 - low) / (high - low)) * (h - 20.0);
                let length = (note.length as f32 * s.zoom).max(1.0);
                notes = notes.line([x, y], [x + length, y], 2.0, LineCap::Butt, tint.alpha(0.9));
            }
            el().absolute(LEAD + clip.start as f32 * s.zoom, 3.0)
                .size(w, h)
                .radius(4.0)
                .fill(tint.alpha(0.18))
                .border(1.0, tint.alpha(0.8))
                .clip_soft(0.0)
                .child(text(clip.name.clone()).font(SMALL).color(p.text).absolute(4.0, 2.0))
                .child(canvas(notes).size(w, h).absolute(0.0, 0.0))
        });
    el().size(width, ROW).child(canvas(bars).size(width, ROW).absolute(0.0, 0.0)).children(clips)
}

fn mixer(s: &State, p: &Palette) -> El {
    let strips = (s.song.tracks.iter().enumerate())
        .map(|(index, track)| strip(s, index, &track.name, track.gain_db, colour(track.colour), p))
        .chain(std::iter::once(strip(s, s.song.tracks.len(), "Master", 0.0, p.accent, p)));
    row().w_full().h(MIXER).fill(p.panel).border_sides([0.0, 1.0, 0.0, 0.0], p.line).px(8.0).gap(4.0).children(strips)
}

fn strip(s: &State, index: usize, name: &str, gain_db: f32, tint: Color, p: &Palette) -> El {
    let thumb_y = FADER_TOP + (1.0 - (gain_db + 60.0) / 66.0).clamp(0.0, 1.0) * (METER - 12.0);
    let fader = el()
        .size(24.0, METER)
        .child(el().pin(Some(10.0), Some(0.0), None, None).size(4.0, METER).radius(2.0).fill(p.well))
        .child(el().pin(Some(0.0), Some(thumb_y), None, None).size(24.0, 12.0).radius(3.0).fill(p.text));
    let meter = |live: &Live<[f32; 4]>| {
        el().size(4.0, METER)
            .radius(2.0)
            .fill(p.well)
            .child(el().pin(Some(0.0), Some(0.0), None, None).size(4.0, METER).fill(tint).live_rect(live))
    };
    col()
        .key(format!("strip-{index}"))
        .w(STRIP)
        .h_full()
        .py(10.0)
        .gap(10.0)
        .center()
        .child(text(name.to_string()).font(LABEL).color(p.text).truncate())
        .child(row().gap(6.0).child(fader).child(meter(&s.meters[index][1])).child(meter(&s.meters[index][2])))
        .child(text(format!("{gain_db:.1} dB")).font(SMALL).color(p.dim))
}

fn main() {
    rune::run(AppSpec::<State> {
        poll: Some(poll),
        ..AppSpec::new("Rune DAW spike", WINDOW, init, view)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    #[test]
    fn a_click_on_solo_lights_it_and_solos_the_track() {
        let mut rt = Runtime::new(init(), view, WINDOW, Env { system_dark: true, scale: 2.0 });
        rt.frame(0.0);
        let (x, y) = rt.find("solo-1").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        // Colours spring to their new value, so let the fade settle.
        for _ in 0..60 {
            rt.frame(1.0 / 60.0);
        }
        assert!(rt.state.song.tracks[1].solo);
        let solo = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("solo-1")).unwrap();
        assert_eq!(solo.fill, colour(0x7ee0a0));
    }
}
