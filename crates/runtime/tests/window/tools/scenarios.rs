//! Scenarios in the window, and what each part of it renders in them. On the song of the DAW
//! template they are the render budgets below. `tooling/perf/ui.py` runs them on any project
//! and measures more, see `measure`.

use std::path::Path;
use std::time::{Duration, Instant};

use gpui::{Modifiers, ScrollDelta, ScrollEvent, TestAppContext, TouchPhase, point, px, size};
use plugin_host::{Plugins, ScanCache};
use runtime::window::DeviceAccess;
use sound_ui::renders::{Part, Renders};
use tempfile::TempDir;

use super::Opened;
use crate::plugin_hosts::scanner;
use crate::song::{BAR, song_files};
use crate::support::{SimulatedInput, open_folder_with};

#[cfg(feature = "perf")]
mod measure;

/// Polls measured in a scenario that plays or stands still: a second.
const POLLS: usize = 60;
/// Polls before the measurement, for what opening and starting set off to settle.
const WARM_UP: usize = 10;
/// Bun op batches measured in a scenario of them.
const BATCHES: usize = 6;
/// How long a measured update waits for Bun's ops.
const PATIENCE: Duration = Duration::from_secs(20);
/// The DAW song the scenarios run on, unless they are given a project.
const TRACKS: usize = 8;
const CLIPS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(feature = "perf"),
    allow(dead_code, reason = "only the measurement runs some")
)]
pub(crate) enum Scenario {
    Idle,
    Playing,
    Scroll,
    /// A resize: one Bun op batch, for a page that reads its size.
    Resize,
    /// The ones below need the song of the DAW template.
    PlayingWithTheMixer,
    Recording,
    /// Card A is the synth of the first track, card B the filter after it.
    KnobDrag,
    /// The page with one `absolute` div, while it plays.
    AbsoluteDiv,
    /// A clip dragged a bar on: one Bun op batch.
    MovedClip,
}

/// What a scenario cost, over its measured updates only.
pub(crate) struct Measured {
    pub renders: Renders,
    #[cfg(feature = "perf")]
    pub cost: measure::Cost,
}

/// The folder of the DAW song the scenarios run on.
pub(crate) fn song_folder() -> TempDir {
    let folder = tempfile::tempdir().unwrap();
    for (path, text) in song_files(TRACKS, CLIPS) {
        super::write(folder.path(), &path, &text);
    }
    folder
}

/// Runs `scenario` on a copy of `project`, which it leaves as it was.
pub(crate) fn run(scenario: Scenario, cx: &mut TestAppContext, project: &Path) -> Measured {
    let folder = tempfile::tempdir().unwrap();
    copy(project, folder.path());
    if scenario == Scenario::AbsoluteDiv {
        add_absolute_div(folder.path());
    }
    let machine = tempfile::tempdir().unwrap();
    crate::agent::install_sidebar(cx, machine.path());
    let input = SimulatedInput::default();
    let device = DeviceAccess {
        open_input: Some(input.opener(cx.executor())),
        ..DeviceAccess::default()
    };
    let plugins = |_: &Path| Plugins::new(Vec::new(), scanner(), ScanCache::none());
    let mut window = open_folder_with(cx, folder, plugins, device);
    window.wait_for_bun();
    match scenario {
        Scenario::Idle => window.measure(POLLS, |_, _| {}),
        Scenario::Playing | Scenario::AbsoluteDiv => {
            window.start_playing();
            window.measure(POLLS, |_, _| {})
        }
        Scenario::PlayingWithTheMixer => {
            let toggle = window.until_found("button-mixer-toggle");
            window.click(toggle);
            window.until_found("div-mixer");
            window.start_playing();
            window.measure(POLLS, |_, _| {})
        }
        Scenario::Recording => {
            window.show_toggles("audio-voice");
            let arm = window.until_found("toggle-arm-audio-voice");
            window.click(arm);
            window.until("the input opens", |_| {
                input.openings.load(std::sync::atomic::Ordering::Relaxed) == 1
            });
            window.press_found("button-record");
            window.until("the song plays", Opened::playing);
            window.measure(POLLS, |window, _| {
                input.write_until(window.engine.frames(), |_| [0.5, 0.25]);
            })
        }
        Scenario::KnobDrag => {
            let name = window.until_found("div-name-t00");
            window.click(name);
            let knob = window.until_found("knob-gain");
            window.press(knob);
            let measured = window.measure(POLLS, |window, step| {
                let up = if step < POLLS / 2 { step } else { POLLS - step };
                window.drag_to(knob - point(px(0.), px(up as f32 * 2.)));
            });
            window.release(knob);
            measured
        }
        Scenario::Scroll => {
            let at = match window.find("space-timeline") {
                Some(timeline) => timeline,
                None => window.middle(),
            };
            window.measure(POLLS, |window, step| {
                let sign = if step < POLLS / 2 { -1. } else { 1. };
                window.cx.simulate_event(ScrollEvent {
                    position: at,
                    delta: ScrollDelta::Pixels(point(px(sign * 20.), px(sign * 10.))),
                    modifiers: Modifiers::default(),
                    touch_phase: TouchPhase::Moved,
                    ..Default::default()
                });
            })
        }
        Scenario::Resize => {
            let first = window.cx.update(|window, _| window.viewport_size());
            let resize = move |window: &mut Opened, step: usize| {
                let wider = if step.is_multiple_of(2) {
                    px(40.)
                } else {
                    px(0.)
                };
                (window.cx).simulate_resize(size(first.width + wider, first.height));
            };
            let answered = window.after_a_batch(Duration::from_secs(3), |window| resize(window, 0));
            assert!(
                answered,
                "the page does not read its size: a resize sends Bun nothing"
            );
            window.measure(BATCHES, |window, step| {
                let answered = window.after_a_batch(PATIENCE, |window| resize(window, step + 1));
                assert!(answered, "Bun sent no ops for a resize");
            })
        }
        Scenario::MovedClip => {
            // The first clip of the first track, a bar on and back.
            let clip = window.on_lane(BAR / 2, 0);
            window.measure(BATCHES, |window, step| {
                let (from, to) = match step % 2 {
                    0 => (
                        clip,
                        clip + point(px(BAR as f32 * crate::song::ACROSS), px(0.)),
                    ),
                    _ => (
                        clip + point(px(BAR as f32 * crate::song::ACROSS), px(0.)),
                        clip,
                    ),
                };
                window.press(from);
                window.drag_to(point((from.x + to.x) / 2., from.y));
                window.drag_to(to);
                let answered = window.after_a_batch(PATIENCE, |window| window.release(to));
                assert!(answered, "Bun sent no ops for a moved clip");
            })
        }
    }
}

impl Opened<'_> {
    /// One poll of the engine, with its timers and the frame it asked for, as the app does.
    pub(super) fn poll(&mut self) {
        let frames = sound_ui::POLL_INTERVAL.as_millis() as usize
            * runtime::OFFLINE.sample_rate as usize
            / 1000;
        self.render(frames);
        self.cx.executor().advance_clock(sound_ui::POLL_INTERVAL);
        self.cx.update(|window, cx| window.simulate_next_frame(cx));
        self.cx.run_until_parked();
    }

    /// `polls` polls: what each part of the window rendered in them.
    pub(super) fn renders_in(&mut self, polls: usize) -> Renders {
        let before = sound_ui::renders::renders();
        for _ in 0..polls {
            self.poll();
        }
        sound_ui::renders::renders().since(&before)
    }

    /// `updates` times `update` and a poll, after the warm-up.
    fn measure(&mut self, updates: usize, mut update: impl FnMut(&mut Self, usize)) -> Measured {
        self.renders_in(WARM_UP);
        let before = sound_ui::renders::renders();
        #[cfg(feature = "perf")]
        let mut cost = measure::Cost::start(self);
        for step in 0..updates {
            #[cfg(feature = "perf")]
            let probe = measure::Probe::start();
            update(self, step);
            self.poll();
            #[cfg(feature = "perf")]
            cost.add(probe);
        }
        Measured {
            renders: sound_ui::renders::renders().since(&before),
            #[cfg(feature = "perf")]
            cost: cost.end(self),
        }
    }

    /// `act`, then waits up to `patience` for the batch of ops it makes Bun send: whether one
    /// came.
    fn after_a_batch(&mut self, patience: Duration, act: impl FnOnce(&mut Self)) -> bool {
        let batches = sound_typescript::heard_batches();
        act(self);
        let started = Instant::now();
        while sound_typescript::heard_batches() == batches {
            if started.elapsed() > patience {
                return false;
            }
            // A frame, for the page to measure a new size.
            self.cx.update(|window, cx| window.simulate_next_frame(cx));
            self.cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(1));
        }
        self.cx.run_until_parked();
        true
    }
}

/// Copies the project folder `from` into `to`, without what git keeps.
fn copy(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_name() == ".git" {
            continue;
        }
        if entry.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A dot over the name of the section, as an agent adds a badge.
fn add_absolute_div(folder: &Path) {
    let path = folder.join("extensions/song.tsx");
    let song = std::fs::read_to_string(&path).unwrap();
    let anchor = "<ClipName page={page} />";
    assert!(song.contains(anchor), "song.tsx has no {anchor}");
    let badge = r#"<div style={{ absolute: { top: 0, right: 0 }, width: 6, height: 6, radius: 3, background: "red" }} />"#;
    std::fs::write(&path, song.replace(anchor, &format!("{anchor}{badge}"))).unwrap();
}

/// A budget that holds: the test fails when it is broken, with the rule it breaks.
fn holds(rule: &str, rendered: u64, budget: u64) {
    assert!(
        rendered <= budget,
        "{rule}: {rendered} renders, the budget is {budget}"
    );
}

/// A budget that does not hold yet: the test says how far off it is, and passes. The change
/// that fixes the cause makes it [`holds`].
fn not_yet(rule: &str, rendered: u64, budget: u64) {
    println!("not held yet, {rule}: {rendered} renders, the budget is {budget}");
}

const STILL: &str = "a still window renders nothing";
const PLAYHEAD: &str = "the playhead draws on the layer over the page, not the page";
const ONCE_A_POLL: &str = "the window draws once a poll at most";
const SIDEBAR: &str = "the sidebar renders only when it changes";
const LEVELS: &str = "a level that moves draws on the layer over the page, not the page";
const TAKE: &str = "a take that grows draws on the layer over the page, not the page";
const KNOB: &str = "a knob on one card draws no other card";
const ABSOLUTE: &str = "one absolute div leaves the playhead on the layer over the page";

fn run_on_the_song(scenario: Scenario, cx: &mut TestAppContext) -> Renders {
    let song = song_folder();
    run(scenario, cx, song.path()).renders
}

#[gpui::test]
fn idle_the_window_renders_nothing(cx: &mut TestAppContext) {
    let renders = run_on_the_song(Scenario::Idle, cx);
    for part in Part::ALL {
        holds(
            &format!("{STILL}: {}", part.name()),
            renders.all_of(part),
            0,
        );
    }
}

#[gpui::test]
fn playing_renders_the_layer_over_the_page_and_not_the_page(cx: &mut TestAppContext) {
    let renders = run_on_the_song(Scenario::Playing, cx);
    holds(PLAYHEAD, renders.of(Part::Body, "song"), 0);
    holds(ONCE_A_POLL, renders.of(Part::Shell, "window"), POLLS as u64);
    holds(SIDEBAR, renders.of(Part::Sidebar, "agent"), 0);
}

#[gpui::test]
fn playing_with_the_mixer_renders_only_the_layer_over_the_page(cx: &mut TestAppContext) {
    let renders = run_on_the_song(Scenario::PlayingWithTheMixer, cx);
    not_yet(LEVELS, renders.of(Part::Body, "song"), 0);
    not_yet(ONCE_A_POLL, renders.of(Part::Shell, "window"), POLLS as u64);
}

#[gpui::test]
fn recording_renders_only_the_layer_over_the_page(cx: &mut TestAppContext) {
    let renders = run_on_the_song(Scenario::Recording, cx);
    not_yet(TAKE, renders.of(Part::Body, "song"), 0);
    not_yet(ONCE_A_POLL, renders.of(Part::Shell, "window"), POLLS as u64);
}

#[gpui::test]
fn a_knob_drag_on_one_card_renders_no_other_card(cx: &mut TestAppContext) {
    let renders = run_on_the_song(Scenario::KnobDrag, cx);
    not_yet(KNOB, renders.of(Part::Card, "song/t00/chain/filter"), 0);
    not_yet(KNOB, renders.of(Part::Body, "song"), 0);
}

#[gpui::test]
fn an_absolute_div_keeps_the_page_still_while_it_plays(cx: &mut TestAppContext) {
    let renders = run_on_the_song(Scenario::AbsoluteDiv, cx);
    holds(ABSOLUTE, renders.of(Part::Body, "song"), 0);
}
