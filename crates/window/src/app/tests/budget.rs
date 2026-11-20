//! What a frame of the window costs with many things placed, measured: scrolling, playing,
//! zooming and dragging run no view in Bun, and stay within their budgets.
//!
//! A cost is in calibrations: the frame's time over that of a fixed piece of work run beside it
//! on the same thread. A slower or busier computer slows both, so the ratio holds where a time in
//! milliseconds swings threefold.

use std::time::{Duration, Instant};

use super::{frame, laid_out, new_project, opened, redraw, with_tools};
use crate::app::State;

/// 64 tracks with 500 clips, each a box with its name, a drag and a place that follows it, a
/// ruler that shares their time, and a playhead.
const CLIPS: &str = r#"import { h, input, live, memo, number, tool } from "./sdk";
const [ROW, BAR] = [40, 3840];
export const clips = tool({
  name: "clips", title: "Clips", when: "Never", doc: "Many clips.", sound: () => input,
  state: { moved: number({ min: -1e9, max: 1e9, default: 0 }) },
  view: (self, { update }) => (
    <col w={1200}>
      <box key="ruler" label="Ruler" w={1200} h={24} plane={{ x: "song" }}>
        {Array.from({ length: 64 }, (_, b) => <box place={[b * BAR, 12]} w={1} h={12} fill="text" />)}
      </box>
      <box key="song" label="Song" w={1200} h={700} plane={{ x: 0.03, y: 1, zoom: [0.003, 0.24] }}>
        {Array.from({ length: 64 }, (_, r) => memo(`row-${r}`, [r], () => (
          <box>{Array.from({ length: 8 }, (_, k) => r + 64 * k).filter((i) => i < 500).map((i) => {
            const d = live.drag(`c${i}`);
            return (
              <box key={`c${i}`} label={`Clip ${i}`} place={[d.dx.plus(Math.floor(i / 64) * 2 * BAR), d.dy.plus(r * ROW), BAR, ROW - 4]}
                fill="fill" radius="sm" clip drag={{ x: true, y: true, snap: [240, ROW] }} onDragEnd={(p) => update("Move", (draft) => void (draft.state.moved += p.dx))}>
                <text at={[4, 2]} style="label">{`Clip ${i}`}</text>
              </box>
            );
          })}</box>
        )))}
        <box place={[live.playhead, 350]} w={2} h={700} fill="text" />
      </box>
    </col>
  ),
});
"#;

/// A piano roll of 2,000 notes, a memo a bar, each note a box that drags.
const NOTES: &str = r#"import { h, input, live, memo, number, tool } from "./sdk";
const [BAR, NOTE] = [3840, 240];
export const notes = tool({
  name: "notes", title: "Notes", when: "Never", doc: "Many notes.", sound: () => input,
  state: { moved: number({ min: -1e9, max: 1e9, default: 0 }) },
  view: (self, { update }) => (
    <col w={1200}>
      <box key="roll" label="Notes" w={1200} h={700} plane={{ x: 0.08, y: 12, zoom: [0.008, 1.28] }}>
        {Array.from({ length: 125 }, (_, bar) => memo(`bar-${bar}`, [bar], () => (
          <box>{Array.from({ length: 16 }, (_, k) => bar * 16 + k).map((i) => {
            const d = live.drag(`n${i}`), pitch = 36 + ((i * 7) % 48);
            return <box key={`n${i}`} label={`Note ${i}`} place={[d.dx.plus(i * NOTE), d.dy.plus(127 - pitch), NOTE, 1]} fill="fill" radius="sm"
              drag={{ x: true, y: true, snap: [NOTE, 1] }} onDragEnd={(p) => update("Move", (draft) => void (draft.state.moved += p.dx))} />;
          })}</box>
        )))}
        <box place={[live.playhead, 64]} w={2} h={700} fill="text" />
      </box>
    </col>
  ),
});
"#;

/// The window of the main view `tool`, laid out and settled.
fn shown(tool: &str, code: &str) -> (tempfile::TempDir, rune::Runtime<State>) {
    let (scratch, folder) = with_tools(&[(tool, code)]);
    let mut state = State::open(crate::app::Options {
        folder,
        headless: true,
        play: None,
        main: Some(sound_core::InstanceId::new(&format!("song/t00/chain/{tool}")).unwrap()),
        act: None,
    })
    .unwrap();
    frame(&mut state, 0.0);
    let mut window = laid_out(state);
    for _ in 0..3 {
        redraw(&mut window);
    }
    (scratch, window)
}

/// A frame's cost, in calibrations.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub(super) struct Cost(f64);

/// What `work` costs: the least it says it took of 15 runs after 3 to warm up, over the least
/// the calibration took between them. Other work on the computer only adds to either.
pub(super) fn measured(mut work: impl FnMut() -> Duration) -> Cost {
    for _ in 0..3 {
        work();
    }
    let (mut least, mut calibration) = (Duration::MAX, Duration::MAX);
    for _ in 0..15 {
        least = least.min(work());
        calibration = calibration.min(timed(calibrate));
    }
    Cost(least.as_secs_f64() / calibration.as_secs_f64())
}

/// How long `work` takes on this thread: the other tests running beside it take none of it.
pub(super) fn timed(work: impl FnOnce()) -> Duration {
    let started = spent();
    work();
    spent().saturating_sub(started)
}

/// Work of a fixed size like a frame's: small allocations, strings and a sorted map.
fn calibrate() {
    let mut map = std::collections::BTreeMap::new();
    for i in 0..4_000u32 {
        map.insert(
            format!("clip-{}", i.wrapping_mul(2_654_435_761)),
            vec![i; 8],
        );
    }
    std::hint::black_box(map);
}

#[cfg(unix)]
fn spent() -> Duration {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: it only writes the time into `now`.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut now) };
    Duration::new(now.tv_sec as u64, now.tv_nsec as u32)
}

// Wall time: Windows counts a thread's time in steps of 15 ms, too coarse for a frame.
#[cfg(not(unix))]
fn spent() -> Duration {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed()
}

pub(super) fn view_runs(window: &rune::Runtime<State>) -> u64 {
    window.state.tools.as_ref().unwrap().view_runs()
}

/// Scrolls, plays, zooms and drags the plane `plane` of `tool`; what each frame cost.
fn budgets(tool: &str, code: &str, plane: &str, dragged: &str) -> [Cost; 3] {
    let (_scratch, mut window) = shown(tool, code);
    let (times, rebuilt) = measure(&mut window, tool, plane, dragged);
    assert!(!rebuilt, "a scroll built the plane again");
    times
}

/// Scrolls, plays, zooms and drags the plane `plane` of the window, named `name`; what each
/// frame cost, and whether a scroll built a plane again. Bun runs no view meanwhile.
pub(super) fn measure(
    window: &mut rune::Runtime<State>,
    name: &str,
    plane: &str,
    dragged: &str,
) -> ([Cost; 3], bool) {
    let runs = view_runs(window);
    let [x, y, w, h] = window.presented(plane).unwrap().0;
    let at = [x + w / 2.0, y + h / 2.0];
    let builds = window.state.planes.builds.get();
    // A small scroll stays in what is built: one live offset.
    let mut step = -1.0;
    let scroll = measured(|| {
        step = -step;
        let wheel = rune::WheelEvent::new(step, step).at(at[0], at[1]);
        timed(|| {
            window.wheel_event(wheel);
            window.frame(1.0 / 60.0);
        })
    });
    let rebuilt = window.state.planes.builds.get() != builds;
    // The window's part of a frame of a playing piece: its live values and Rune's frame, not
    // the engine, which a window plays on a thread of its own.
    (window.state.session).background(|project| project.engine().play());
    let mut seconds = window.state.last;
    let play = measured(|| {
        seconds += 1.0 / 60.0;
        frame(&mut window.state, seconds);
        timed(|| {
            window.state.lives.update(&window.state, 1.0 / 60.0);
            window.frame(1.0 / 60.0);
        })
    });
    assert!(window.state.playing);
    (window.state.session).background(|project| project.engine().stop());
    let mut zoom_in = true;
    let zoom = measured(|| {
        let delta = if zoom_in { 0.25 } else { -0.2 };
        zoom_in = !zoom_in;
        timed(|| {
            window.magnify_event(rune::MagnifyEvent::new(delta).at(at[0], at[1]));
            window.frame(1.0 / 60.0);
        })
    });
    let (x, y) = window.find(dragged).unwrap();
    window.pointer_down(x, y);
    window.frame(1.0 / 60.0);
    for step in 1..=10 {
        window.pointer_move(x + step as f32 * 3.0, y + step as f32);
        window.frame(1.0 / 60.0);
    }
    assert!(window.state.lives.hand(dragged).active);
    assert_eq!(view_runs(window), runs, "Bun ran a view");
    eprintln!("{name}: scroll {scroll:?}, play {play:?}, zoom {zoom:?} a frame");
    ([scroll, play, zoom], rebuilt)
}

/// The budget of the window's work in a frame that scrolls, plays or zooms: 4 ms as the app is
/// built, 12 in a test build, which is not optimized, where a calibration takes about 2.7.
pub(super) const FRAME: Cost = Cost(4.4);

#[test]
fn five_hundred_clips_scroll_play_zoom_and_drag_within_their_budgets_and_run_no_view() {
    let [scroll, play, zoom] = budgets("clips", CLIPS, "song", "c3");
    assert!(scroll < FRAME, "a scroll frame cost {scroll:?}");
    assert!(play < FRAME, "a play frame cost {play:?}");
    assert!(zoom < FRAME, "a zoom frame cost {zoom:?}");
}

#[test]
fn two_thousand_notes_scroll_play_zoom_and_drag_within_their_budgets_and_run_no_view() {
    let [scroll, play, zoom] = budgets("notes", NOTES, "roll", "n5");
    assert!(scroll < FRAME, "a scroll frame cost {scroll:?}");
    assert!(play < FRAME, "a play frame cost {play:?}");
    assert!(zoom < FRAME, "a zoom frame cost {zoom:?}");
}

/// The song of the template with 64 tracks of 500 clips: its records, as the window keeps them.
fn big_song() -> tempfile::TempDir {
    let scratch = new_project();
    let song = scratch.path().join("song/state/song");
    let copy = |from: &std::path::Path, to: &std::path::Path| {
        for entry in walk(from) {
            let into = to.join(entry.strip_prefix(from).unwrap());
            std::fs::create_dir_all(into.parent().unwrap()).unwrap();
            std::fs::copy(&entry, into).unwrap();
        }
    };
    for track in 2..64 {
        copy(&song.join("t00"), &song.join(format!("t{track:02}")));
    }
    let notes: Vec<_> = (0..8).map(|n| serde_json::json!({ "start": n * 480, "length": 480, "pitch": 48 + n * 2, "velocity": 100 })).collect();
    for clip in 0..500 {
        let (track, at) = (clip % 64, clip / 64);
        let record = serde_json::json!({ "tool": "timeline", "state": { "start": at * 2 * 3840, "length": 3840, "notes": notes } });
        let file = song.join(format!("t{track:02}/c{at}.json"));
        std::fs::write(file, record.to_string()).unwrap();
    }
    scratch
}

fn walk(folder: &std::path::Path) -> Vec<std::path::PathBuf> {
    (std::fs::read_dir(folder).unwrap().flatten())
        .flat_map(|entry| match entry.path().is_dir() {
            true => walk(&entry.path()),
            false => vec![entry.path()],
        })
        .collect()
}

#[test]
fn the_song_of_the_template_with_64_tracks_and_500_clips_runs_no_view_as_it_scrolls_plays_and_zooms()
 {
    let scratch = big_song();
    let mut window = laid_out(opened(&scratch.path().join("song")));
    for _ in 0..3 {
        redraw(&mut window);
    }
    // A frame waits 15 seconds for Bun, which a busy computer can take to read 64 tracks.
    let start = Instant::now();
    while window.presented("clip-t05/c1").is_none() {
        let failed = crate::app::FAILED.lock().unwrap().clone();
        assert!(start.elapsed() < Duration::from_secs(45), "{failed:?}");
        redraw(&mut window);
    }
    let ([scroll, play, zoom], rebuilt) = measure(&mut window, "song", "song", "clip-t05/c1");
    assert!(!rebuilt, "a scroll built the plane again");
    assert!(scroll < FRAME, "a scroll frame cost {scroll:?}");
    assert!(play < FRAME, "a play frame cost {play:?}");
    assert!(zoom < FRAME, "a zoom frame cost {zoom:?}");
}

/// The song of the template with 16 audio tracks of 100 audio clips of a four-second file, its
/// waves read: they draw only what shows, and run no view as the song scrolls, plays and zooms.
#[test]
fn the_song_of_the_template_with_100_audio_clips_on_16_audio_tracks_runs_no_view_as_it_scrolls_plays_and_zooms()
 {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    let song = folder.join("state/song");
    super::audio::tone(&folder.join("assets/audio/tone.wav"), 4.0);
    for track in 0..16 {
        let at = song.join(format!("audio-{track:02}"));
        std::fs::create_dir_all(at.join("chain")).unwrap();
        let files = [
            (
                "instance.json",
                r#"{"tool": "tape", "state": {"colour": "teal"}}"#,
            ),
            (
                "rec.json",
                r#"{"tool": "recorder", "state": {"channels": [0]}}"#,
            ),
            ("chain/instance.json", r#"{"tool": "chain", "state": {}}"#),
            ("mix.json", r#"{"tool": "fader", "state": {}}"#),
        ];
        for (path, contents) in files {
            std::fs::write(at.join(path), contents).unwrap();
        }
    }
    for clip in 0..100 {
        let (track, at) = (clip % 16, clip / 16);
        let record = serde_json::json!({ "tool": "region", "state": { "asset": "tone.wav", "start": at * 3 * 3840 } });
        std::fs::write(
            song.join(format!("audio-{track:02}/r{at}.json")),
            record.to_string(),
        )
        .unwrap();
    }
    let mut window = laid_out(opened(&folder));
    let start = std::time::Instant::now();
    while window.presented("wave-audio-03/r1").is_none() {
        assert!(start.elapsed().as_secs() < 30, "the waves were never drawn");
        redraw(&mut window);
    }
    for _ in 0..3 {
        redraw(&mut window);
    }
    let ([scroll, play, zoom], rebuilt) = measure(&mut window, "song", "song", "clip-audio-03/r1");
    assert!(!rebuilt, "a scroll built the plane again");
    assert!(scroll < FRAME, "a scroll frame cost {scroll:?}");
    assert!(play < FRAME, "a play frame cost {play:?}");
    assert!(zoom < FRAME, "a zoom frame cost {zoom:?}");
}

/// An idle window changes nothing and runs no view: a frame that finds nothing new draws
/// nothing.
#[test]
fn idle_the_window_changes_nothing_and_runs_no_view() {
    let scratch = new_project();
    let mut state = opened(&scratch.path().join("song"));
    for step in 1..=30 {
        frame(&mut state, f64::from(step) / 60.0);
    }
    let runs = state.tools.as_ref().unwrap().view_runs();
    for step in 31..=150 {
        let (changed, _) = frame(&mut state, f64::from(step) / 60.0);
        assert!(!changed, "frame {step} changed what shows");
    }
    assert_eq!(state.tools.as_ref().unwrap().view_runs(), runs);
}

/// The mixer of the big song: a frame while it plays, its 64 strips' meters moving, and each
/// frame of a fader's drag, which plays the volume as it goes, stay within the budget of a play
/// frame, and Bun runs no view for either.
#[test]
fn the_mixer_of_64_tracks_plays_and_drags_a_fader_within_its_budget_and_runs_no_view() {
    let scratch = big_song();
    let mut state = opened(&scratch.path().join("song"));
    super::act(&mut state, serde_json::json!({ "action": "song.mixer" }));
    let mut window = laid_out(state);
    for _ in 0..3 {
        redraw(&mut window);
    }
    let runs = view_runs(&window);
    (window.state.session).background(|project| project.engine().play());
    let mut seconds = window.state.last;
    let play = measured(|| {
        seconds += 1.0 / 60.0;
        frame(&mut window.state, seconds);
        timed(|| {
            window.state.lives.update(&window.state, 1.0 / 60.0);
            window.frame(1.0 / 60.0);
        })
    });
    assert!(window.state.playing);
    let (x, y) = window.find("fader-t00").unwrap();
    window.pointer_down(x, y);
    window.frame(1.0 / 60.0);
    let mut step = 0.0;
    let drag = measured(|| {
        step += 1.0;
        seconds += 1.0 / 60.0;
        frame(&mut window.state, seconds);
        timed(|| {
            window.pointer_move(x, y - step);
            window.frame(1.0 / 60.0);
        })
    });
    assert!(window.state.lives.hand("fader-t00").active);
    assert_eq!(view_runs(&window), runs, "Bun ran a view");
    eprintln!("mixer of 64 tracks: play {play:?}, drag {drag:?} a frame");
    // Unoptimized, Rune's frame of the many meters of the strips takes about 4.5 times as long.
    let budget = Cost(FRAME.0 * if cfg!(debug_assertions) { 2.0 } else { 1.0 });
    assert!(play < budget, "a play frame cost {play:?}");
    assert!(drag < budget, "a frame of the drag cost {drag:?}");
}

/// A run of the song's view that changes no record, such as one that shows the mixer, builds
/// no memo of the clips or the heads again: Bun sends only what changed.
#[test]
fn showing_the_mixer_sends_no_clips_or_heads_again() {
    let scratch = new_project();
    let mut state = opened(&scratch.path().join("song"));
    let song = sound_core::InstanceId::new("song").unwrap();
    let versions = |state: &State| {
        ["time-t00", "head-t00"].map(|memo| state.memo_of(&song, memo).map(|(version, _)| version))
    };
    let before = versions(&state);
    assert!(before.iter().all(Option::is_some), "{before:?}");
    super::act(&mut state, serde_json::json!({ "action": "song.mixer" }));
    assert!(state.memo_of(&song, "mixer-t00").is_some());
    assert_eq!(versions(&state), before);
}
