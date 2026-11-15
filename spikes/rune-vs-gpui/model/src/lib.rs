//! The song both spike apps draw, and what a scenario changes each frame. No dependencies, so the
//! Rune app (toolchain 1.99) and the GPUI app (1.98, the repo's) share it as a path crate.
//!
//! The song is the one `crates/runtime/tests/shared/song.rs` writes for `tooling/perf/ui.py`:
//! the same seed, clip lengths and note counts.

use std::time::Duration;

pub const BAR: u64 = 3840;
pub const BEAT: u64 = BAR / 4;
/// Sizes from `song-page.ts`: a track's row, its header, points per tick, points left of bar 1.
pub const ROW: f32 = 64.0;
pub const HEADER: f32 = 176.0;
pub const ACROSS: f32 = 0.025;
pub const LEAD: f32 = 8.0;
pub const RULER: f32 = 28.0;
pub const TRANSPORT: f32 = 44.0;
pub const MIXER: f32 = 232.0;
pub const STRIP: f32 = 72.0;
pub const WINDOW: (f32, f32) = (1440.0, 900.0);
/// Lowest and highest pitch the clip previews span.
pub const PITCHES: (u8, u8) = (48, 84);
/// Ticks a second at 120 bpm.
pub const TICKS_PER_SECOND: f64 = 1920.0;

#[derive(Clone, Debug)]
pub struct Note {
    pub start: u64,
    pub length: u64,
    pub pitch: u8,
    pub velocity: u8,
}

#[derive(Clone, Debug)]
pub struct Clip {
    pub name: String,
    pub start: u64,
    pub length: u64,
    pub notes: Vec<Note>,
}

#[derive(Clone, Debug)]
pub struct Track {
    pub name: String,
    /// 0xRRGGBB.
    pub colour: u32,
    pub clips: Vec<Clip>,
    pub mute: bool,
    pub solo: bool,
    pub armed: bool,
    pub gain_db: f32,
}

#[derive(Clone, Debug)]
pub struct Song {
    pub tracks: Vec<Track>,
}

impl Song {
    /// The last tick any clip reaches, rounded up to a bar, plus 8 bars of room.
    pub fn end(&self) -> u64 {
        let last = (self.tracks.iter().flat_map(|t| &t.clips))
            .map(|c| c.start + c.length)
            .max()
            .unwrap_or(0);
        (last.div_ceil(BAR) + 8) * BAR
    }

    pub fn notes(&self) -> usize {
        self.tracks.iter().flat_map(|t| &t.clips).map(|c| c.notes.len()).sum()
    }
}

const COLOURS: [u32; 6] = [0xa9b1ff, 0x7ee0a0, 0xf0b44c, 0xff8fa3, 0x7cc7ff, 0xd0a6ff];

fn noise(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
    *seed >> 33
}

/// `tracks` tracks with `clips` clips in all, each 1 or 2 bars of 30 to 150 notes, a clip every
/// 3 bars: the perf harness's song (8 tracks, 32 clips) by default.
pub fn song(tracks: usize, clips: usize) -> Song {
    let mut seed = 7;
    let tracks = (0..tracks)
        .map(|track| {
            let mine = clips / tracks + usize::from(track < clips % tracks);
            let clips = (0..mine)
                .map(|clip| {
                    let bars = 1 + noise(&mut seed) % 2;
                    let length = bars * BAR;
                    let count = 30 + noise(&mut seed) % 121;
                    let notes = (0..count)
                        .map(|index| Note {
                            start: index * length / count,
                            pitch: 48 + (noise(&mut seed) % 36) as u8,
                            length: 120 + noise(&mut seed) % 360,
                            velocity: 60 + (noise(&mut seed) % 60) as u8,
                        })
                        .collect();
                    Clip { name: format!("Clip {}", clip + 1), start: clip as u64 * 3 * BAR, length, notes }
                })
                .collect();
            Track {
                name: format!("Track {}", track + 1),
                colour: COLOURS[track % COLOURS.len()],
                clips,
                mute: track % 5 == 3,
                solo: false,
                armed: track == 0,
                gain_db: -((track % 4) as f32) * 3.0,
            }
        })
        .collect();
    Song { tracks }
}

/// What each run of an app does by itself, so no input is needed to measure it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    /// Nothing moves: the window should draw nothing.
    Idle,
    /// The song plays: the playhead, the time and a meter per track and the master move every frame.
    Play,
    /// The timeline scrolls sideways at 600 pt/s, wrapping at the end. The song is stopped.
    Scroll,
    /// The zoom swings between 0.4x and 2.5x over 4 s. The song is stopped.
    Zoom,
    /// Every 100 ms one clip moves a beat later: a record edit, as an agent's file write or a drag
    /// does, which the views must show. Plays meanwhile.
    Edit,
}

impl Scenario {
    pub const ALL: [Scenario; 5] = [Scenario::Idle, Scenario::Play, Scenario::Scroll, Scenario::Zoom, Scenario::Edit];

    pub fn name(self) -> &'static str {
        match self {
            Scenario::Idle => "idle",
            Scenario::Play => "play",
            Scenario::Scroll => "scroll",
            Scenario::Zoom => "zoom",
            Scenario::Edit => "edit",
        }
    }

    pub fn parse(name: &str) -> Option<Scenario> {
        Scenario::ALL.into_iter().find(|s| s.name() == name)
    }

    pub fn playing(self) -> bool {
        matches!(self, Scenario::Play | Scenario::Edit)
    }
}

/// Where the song is at `seconds` into a scenario.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moment {
    pub playhead: u64,
    pub scroll_x: f32,
    /// Points per tick.
    pub zoom: f32,
    pub playing: bool,
}

pub fn moment(scenario: Scenario, seconds: f64, song_end: u64) -> Moment {
    let mut m = Moment { playhead: 0, scroll_x: 0.0, zoom: ACROSS, playing: scenario.playing() };
    match scenario {
        Scenario::Idle => {}
        Scenario::Play | Scenario::Edit => m.playhead = ((seconds * TICKS_PER_SECOND) as u64) % song_end,
        Scenario::Scroll => {
            let width = song_end as f32 * ACROSS;
            m.scroll_x = (seconds as f32 * 600.0) % (width * 0.8);
        }
        Scenario::Zoom => {
            let phase = (seconds as f32 / 4.0 * std::f32::consts::TAU).sin();
            m.zoom = ACROSS * 2.5f32.powf(phase);
        }
    }
    m
}

/// The level of a meter in 0..1 at `seconds`, for track `index` (the master is `tracks`), while
/// playing. Moves every frame.
pub fn meter(index: usize, seconds: f64) -> f32 {
    let i = index as f64;
    let wobble = (seconds * (3.0 + i * 0.7)).sin() * 0.5 + 0.5;
    let beat = 1.0 - (seconds * 2.0 + i * 0.13).fract();
    (0.35 * wobble + 0.55 * beat * beat) as f32
}

/// The clip the edit scenario moves at `seconds`, and how many edits happened by then.
pub fn edits(seconds: f64) -> u64 {
    (seconds / 0.1) as u64
}

/// Applies edit number `n`: moves one clip a beat later, cycling through every clip.
pub fn apply_edit(song: &mut Song, n: u64) {
    let clips: usize = song.tracks.iter().map(|t| t.clips.len()).sum();
    if clips == 0 {
        return;
    }
    let mut index = (n as usize * 7) % clips;
    for track in &mut song.tracks {
        if index < track.clips.len() {
            track.clips[index].start += BEAT;
            return;
        }
        index -= track.clips.len();
    }
}

/// "1.1.000" style position of a tick: bar, beat, tick in the beat.
pub fn position(tick: u64) -> String {
    format!("{}.{}.{:03}", tick / BAR + 1, (tick % BAR) / BEAT + 1, tick % BEAT)
}

/// Run options both apps read from their arguments.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub scenario: Scenario,
    pub tracks: usize,
    pub clips: usize,
    /// Quit after this long; `None` runs until closed.
    pub seconds: Option<Duration>,
}

impl Options {
    /// `--scenario idle|play|scroll|zoom|edit --tracks N --clips N --seconds S`, ignoring others.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Options {
        let mut o = Options { scenario: Scenario::Idle, tracks: 8, clips: 32, seconds: None };
        let args: Vec<String> = args.into_iter().collect();
        let value = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1));
        if let Some(s) = value("--scenario").and_then(|s| Scenario::parse(s)) {
            o.scenario = s;
        }
        if let Some(n) = value("--tracks").and_then(|s| s.parse().ok()) {
            o.tracks = n;
        }
        if let Some(n) = value("--clips").and_then(|s| s.parse().ok()) {
            o.clips = n;
        }
        if let Some(s) = value("--seconds").and_then(|s| s.parse::<f64>().ok()) {
            o.seconds = Some(Duration::from_secs_f64(s));
        }
        o
    }
}
