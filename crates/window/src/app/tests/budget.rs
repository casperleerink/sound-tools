//! What a frame of the window costs with many things placed, measured: scrolling, playing,
//! zooming and dragging run no view in Bun, and stay within their budgets.

use std::time::Duration;

use super::{frame, laid_out, redraw, with_tools};
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
      <box key="song" label="Song" w={1200} h={700} plane={{ x: 0.03, y: 1, zoom: [0.1, 8] }}>
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
      <box key="roll" label="Notes" w={1200} h={700} plane={{ x: 0.08, y: 12, zoom: [0.1, 16] }}>
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

/// The middle of what `work` says it took, of 15 runs after 3 to warm up.
fn measured(mut work: impl FnMut() -> Duration) -> Duration {
    for _ in 0..3 {
        work();
    }
    let mut times: Vec<Duration> = (0..15).map(|_| work()).collect();
    times.sort();
    times[times.len() / 2]
}

/// How long `work` takes on this thread: the other tests running beside it take none of it.
fn timed(work: impl FnOnce()) -> Duration {
    let started = spent();
    work();
    spent().saturating_sub(started)
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

#[cfg(not(unix))]
fn spent() -> Duration {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed()
}

fn view_runs(window: &rune::Runtime<State>) -> u64 {
    window.state.tools.as_ref().unwrap().view_runs()
}

/// Scrolls, plays, zooms and drags the plane `plane` of `tool`; what each frame took.
fn budgets(tool: &str, code: &str, plane: &str, dragged: &str) -> [Duration; 3] {
    let (_scratch, mut window) = shown(tool, code);
    let runs = view_runs(&window);
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
    assert_eq!(
        window.state.planes.builds.get(),
        builds,
        "a scroll built the plane again"
    );
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
    assert!(window.state.planes.builds.get() > builds);
    let (x, y) = window.find(dragged).unwrap();
    window.pointer_down(x, y);
    window.frame(1.0 / 60.0);
    for step in 1..=10 {
        window.pointer_move(x + step as f32 * 3.0, y + step as f32);
        window.frame(1.0 / 60.0);
    }
    assert!(window.state.lives.hand(dragged).active);
    assert_eq!(view_runs(&window), runs, "Bun ran a view");
    eprintln!("{tool}: scroll {scroll:?}, play {play:?}, zoom {zoom:?} a frame");
    [scroll, play, zoom]
}

/// The budgets of the window's work in a frame, as the app is built. A test build of the window
/// is not optimized, which makes this work about three times slower.
const SCROLL_AND_PLAY: Duration = Duration::from_millis(4 * UNOPTIMIZED);
const ZOOM: Duration = Duration::from_millis(8 * UNOPTIMIZED);
const UNOPTIMIZED: u64 = if cfg!(debug_assertions) { 3 } else { 1 };

#[test]
fn five_hundred_clips_scroll_play_zoom_and_drag_within_their_budgets_and_run_no_view() {
    let [scroll, play, zoom] = budgets("clips", CLIPS, "song", "c3");
    assert!(scroll < SCROLL_AND_PLAY, "a scroll frame took {scroll:?}");
    assert!(play < SCROLL_AND_PLAY, "a play frame took {play:?}");
    assert!(zoom < ZOOM, "a zoom frame took {zoom:?}");
}

#[test]
fn two_thousand_notes_scroll_play_zoom_and_drag_within_their_budgets_and_run_no_view() {
    let [scroll, play, zoom] = budgets("notes", NOTES, "roll", "n5");
    assert!(scroll < SCROLL_AND_PLAY, "a scroll frame took {scroll:?}");
    assert!(play < SCROLL_AND_PLAY, "a play frame took {play:?}");
    assert!(zoom < ZOOM, "a zoom frame took {zoom:?}");
}
