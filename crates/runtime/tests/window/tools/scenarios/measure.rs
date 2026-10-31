//! What `tooling/perf/ui.py` runs, with the `perf` feature: every scenario that fits the project
//! in a window of its own, with what each cost written as JSON. Allocations are the test
//! thread's, which runs the window and the engine; Bun's thread is not counted.

use std::collections::BTreeMap;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bench_metrics::{AllocationCounter, AllocationMetric, CountingAllocator, MetricScope};
use criterion::measurement::Measurement;
use gpui::TestAppContext;
use gpui::profiler::journal::{
    ForegroundEvent, ForegroundJournalCollector, ForegroundJournalEntry, IntervalBoundary,
};
use serde_json::{Map, Value, json};
use sound_ui::renders::{Part, Renders};

use super::{Measured, Opened, Scenario, run, song_folder};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::new(std::alloc::System);

impl Scenario {
    const ALL: [Scenario; 9] = [
        Scenario::Idle,
        Scenario::Playing,
        Scenario::Scroll,
        Scenario::Resize,
        Scenario::PlayingWithTheMixer,
        Scenario::Recording,
        Scenario::KnobDrag,
        Scenario::AbsoluteDiv,
        Scenario::MovedClip,
    ];

    fn name(self) -> &'static str {
        match self {
            Scenario::Idle => "idle",
            Scenario::Playing => "playing",
            Scenario::Scroll => "scroll",
            Scenario::Resize => "resize",
            Scenario::PlayingWithTheMixer => "playing with the mixer",
            Scenario::Recording => "recording",
            Scenario::KnobDrag => "knob drag",
            Scenario::AbsoluteDiv => "absolute div while playing",
            Scenario::MovedClip => "moved clip",
        }
    }

    fn needs_the_song(self) -> bool {
        !matches!(
            self,
            Scenario::Idle | Scenario::Playing | Scenario::Scroll | Scenario::Resize
        )
    }
}

/// Whether `folder` holds the song of the DAW template, whose controls some scenarios press.
fn is_song(folder: &Path) -> bool {
    folder.join("state/song/instance.json").is_file()
        && folder.join("extensions/song.tsx").is_file()
}

/// What a scenario cost beyond its renders.
pub(crate) struct Cost {
    updates: Vec<Duration>,
    allocations: Vec<u64>,
    allocated_bytes: Vec<u64>,
    draws: Vec<Duration>,
    /// The own time of each view type in each draw phase, as gpui records it: only a phase of
    /// 0.5 ms or more.
    views: BTreeMap<(&'static str, &'static str), Duration>,
    journal: ForegroundJournalCollector,
    /// Bytes the window told Bun and heard from it.
    bytes: (u64, u64),
    /// Nodes per card of a tool of the project, by instance, at the end.
    nodes: BTreeMap<String, usize>,
}

/// One update as it starts.
pub(crate) struct Probe {
    started: Instant,
    allocations: u64,
    allocated_bytes: u64,
}

fn counter(metric: AllocationMetric) -> AllocationCounter {
    AllocationCounter::new(metric, MetricScope::CallingThread).unwrap()
}

impl Probe {
    pub(crate) fn start() -> Self {
        Self {
            allocations: counter(AllocationMetric::Allocations).start(),
            allocated_bytes: counter(AllocationMetric::AllocatedBytes).start(),
            started: Instant::now(),
        }
    }
}

impl Cost {
    pub(crate) fn start(window: &mut Opened) -> Self {
        let journal = window
            .cx
            .update(|_, cx| cx.foreground_journal().collector());
        Self {
            updates: Vec::new(),
            allocations: Vec::new(),
            allocated_bytes: Vec::new(),
            draws: Vec::new(),
            views: BTreeMap::new(),
            journal,
            bytes: (
                sound_typescript::sent_bytes(),
                sound_typescript::heard_bytes(),
            ),
            nodes: BTreeMap::new(),
        }
    }

    pub(crate) fn add(&mut self, probe: Probe) {
        self.updates.push(probe.started.elapsed());
        let allocations = counter(AllocationMetric::Allocations).end(probe.allocations);
        let bytes = counter(AllocationMetric::AllocatedBytes).end(probe.allocated_bytes);
        self.allocations.push(allocations as u64);
        self.allocated_bytes.push(bytes as u64);
        self.read_journal();
    }

    fn read_journal(&mut self) {
        for entry in self.journal.collect_unseen().entries {
            match entry {
                ForegroundJournalEntry::Event(ForegroundEvent::Draw(frame))
                | ForegroundJournalEntry::Boundary(IntervalBoundary::Presented(
                    gpui::profiler::journal::PresentedFrame { frame, .. },
                )) => self.draws.push(frame.draw_duration()),
                ForegroundJournalEntry::Event(ForegroundEvent::View(view)) => {
                    *self
                        .views
                        .entry((view.name, view.phase.as_str()))
                        .or_default() += view.self_duration;
                }
                _ => {}
            }
        }
    }

    pub(crate) fn end(mut self, window: &mut Opened) -> Self {
        self.read_journal();
        self.bytes = (
            sound_typescript::sent_bytes() - self.bytes.0,
            sound_typescript::heard_bytes() - self.bytes.1,
        );
        let shell = window.shell.clone();
        self.nodes = window.cx.read(|cx| {
            (shell.read(cx).main_view())
                .map(|page| sound_typescript::node_counts(page, cx))
                .unwrap_or_default()
        });
        self
    }
}

fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// Mean, median, 90th percentile and largest.
fn spread(mut values: Vec<f64>) -> Value {
    if values.is_empty() {
        return json!({ "count": 0 });
    }
    values.sort_by(f64::total_cmp);
    let at = |share: f64| values[((values.len() - 1) as f64 * share).round() as usize];
    json!({
        "count": values.len(),
        "mean": values.iter().sum::<f64>() / values.len() as f64,
        "median": at(0.5),
        "p90": at(0.9),
        "max": at(1.0),
    })
}

fn renders_json(renders: &Renders) -> (Value, Value) {
    let (mut counts, mut times) = (Map::new(), Map::new());
    for ((part, instance), count) in &renders.0 {
        let entry = counts.entry(part.name()).or_insert_with(|| json!({}));
        entry[instance] = json!(count.renders);
        if *part != Part::Card {
            let entry = times.entry(part.name()).or_insert_with(|| json!({}));
            entry[instance] = json!(milliseconds(count.time));
        }
    }
    (Value::Object(counts), Value::Object(times))
}

fn to_json(measured: Measured) -> Value {
    let Measured { renders, cost } = measured;
    let (renders, render_ms) = renders_json(&renders);
    let mut views = Map::new();
    for ((view, phase), time) in &cost.views {
        let entry = views.entry(*view).or_insert_with(|| json!({}));
        entry[*phase] = json!(milliseconds(*time));
    }
    let as_f64 = |values: &[u64]| values.iter().map(|value| *value as f64).collect();
    json!({
        "updates": cost.updates.len(),
        "update ms": spread(cost.updates.iter().copied().map(milliseconds).collect()),
        "draw ms": spread(cost.draws.iter().copied().map(milliseconds).collect()),
        "allocations per update": spread(as_f64(&cost.allocations)),
        "allocated bytes per update": spread(as_f64(&cost.allocated_bytes)),
        "renders": renders,
        "own render ms": render_ms,
        "view self ms": views,
        "nodes": cost.nodes,
        "bytes to Bun": cost.bytes.0,
        "bytes from Bun": cost.bytes.1,
    })
}

/// One scenario in a window of its own, on macOS with the system's text, as the app shapes it.
fn measure_one(scenario: Scenario, project: &Path) -> Value {
    let project = project.to_path_buf();
    gpui::run_test_once(
        0,
        Box::new(move |dispatcher| {
            #[cfg(target_os = "macos")]
            let mut cx = TestAppContext::build_with_text_system(
                dispatcher,
                None,
                std::sync::Arc::new(gpui_apple::AppleTextSystem::new()),
            );
            #[cfg(not(target_os = "macos"))]
            let mut cx = TestAppContext::build(dispatcher, None);
            let measured = run(scenario, &mut cx, &project);
            cx.run_until_parked();
            cx.update(|cx| {
                cx.background_executor().forbid_parking();
                cx.quit();
            });
            to_json(measured)
        }),
    )
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    (panic.downcast_ref::<String>().cloned())
        .or_else(|| panic.downcast_ref::<&str>().map(ToString::to_string))
        .unwrap_or_else(|| "it panicked".to_string())
}

#[test]
#[ignore = "tooling/perf/ui.py runs it"]
fn measure_the_window() {
    let out = PathBuf::from(std::env::var_os("UI_PERF_OUT").expect("UI_PERF_OUT: where to write"));
    let song = song_folder();
    let project = std::env::var_os("UI_PERF_PROJECT").map(PathBuf::from);
    let folder = project.as_deref().unwrap_or(song.path());
    let mut scenarios = Map::new();
    for scenario in Scenario::ALL {
        eprintln!("measuring {}", scenario.name());
        let result = if scenario.needs_the_song() && !is_song(folder) {
            json!({ "skipped": "needs the song of the DAW template" })
        } else {
            std::panic::catch_unwind(AssertUnwindSafe(|| measure_one(scenario, folder)))
                .unwrap_or_else(|panic| json!({ "failed": panic_message(&*panic) }))
        };
        scenarios.insert(scenario.name().to_string(), result);
    }
    let project = match project {
        Some(path) => path.display().to_string(),
        None => format!(
            "the DAW song, {} tracks of {} clips",
            super::TRACKS,
            super::CLIPS
        ),
    };
    let report = json!({ "project": project, "scenarios": scenarios });
    std::fs::write(&out, serde_json::to_string_pretty(&report).unwrap()).unwrap();
}
