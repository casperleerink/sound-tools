//! Feeds and facts as a view reads them: from the record as it plays, with one take of the
//! engine's peaks per frame shared by every feed.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sound_core::feeds::{self, Feed, FeedRequest, NoSources, Readings};
use sound_core::{
    BehaviourContext, BehaviourError, Changes, Engine, EngineConfig, Parameter, PlayedLanes,
    Project, Scale, State, Ticks, facts, ranges,
};

use crate::tools::{EXTENSION, SAMPLE_RATE, id, registry};

const DEPTH: Parameter<Dial> = Parameter {
    field: "depth",
    min: 0.0,
    max: 1.0,
    default: 0.5,
    scale: Scale::Linear,
    get: |dial| dial.depth,
    set: |dial, value| dial.depth = value,
};

const NUMBERS: [&Parameter<Dial>; 1] = [&DEPTH];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(transform = ranges(&NUMBERS))]
struct Dial {
    depth: f32,
    /// A lane plays `depth` from tick 100 on.
    automated: bool,
    steps: Vec<Step>,
    page: Page,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
struct Step {
    on: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
struct Page {
    #[serde(rename = "type")]
    shape: Shape,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Shape {
    #[default]
    Low,
    High,
}

impl State for Dial {
    const TOOL: &'static str = "test.dial";
}

struct Lane;

impl PlayedLanes for Lane {
    fn values_at(&self, tick: Ticks, values: &mut Vec<(Arc<str>, f32)>) {
        if tick >= Ticks(100) {
            values.push(("depth".into(), 0.75));
        }
    }
}

#[derive(Serialize, JsonSchema)]
struct DialFacts {
    deep: bool,
}

fn apply(state: &Dial, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    context.peaks("level");
    if state.automated {
        context.show_lanes(None, Arc::new(Lane));
    }
    Ok(())
}

fn open() -> (Project, Engine, tempfile::TempDir) {
    let folder = tempfile::tempdir().unwrap();
    let mut registry = registry();
    registry
        .tool::<Dial>(EXTENSION)
        .unwrap()
        .behaviour(apply)
        .schema(&NUMBERS)
        .feed("depth", "Its depth as it plays.", |dial, _| dial.depth)
        .feed("steps", "1 for each step that is on.", |dial, _| {
            (dial.steps.iter())
                .map(|step| f32::from(u8::from(step.on)))
                .collect::<Vec<f32>>()
        })
        .facts(|dial, _| DialFacts {
            deep: dial.depth > 0.5,
        });
    let (control, engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 1));
    let project = Project::open(folder.path(), registry, control).unwrap();
    (project, engine, folder)
}

fn add(project: &mut Project, dial: Dial) {
    let mut changes = Changes::new();
    changes.create(id("dial"), dial);
    project.commit("Add dial", changes).unwrap();
}

fn at(tick: u64) -> FeedRequest {
    FeedRequest {
        tick: Ticks(tick),
        points: 0,
        range: None,
    }
}

fn read(project: &Project, readings: &Readings, name: &str, tick: u64) -> Option<Feed> {
    feeds::read(project, readings, &NoSources, &id("dial"), name, at(tick))
}

#[test]
fn a_feed_reads_the_record_as_its_lane_plays_it() {
    let (mut project, _engine, _folder) = open();
    add(
        &mut project,
        Dial {
            depth: 0.25,
            automated: true,
            ..Dial::default()
        },
    );
    let readings = Readings::new();
    assert_eq!(
        read(&project, &readings, "depth", 0),
        Some(Feed::Number(0.25))
    );
    assert_eq!(
        read(&project, &readings, "depth", 200),
        Some(Feed::Number(0.75))
    );
    assert_eq!(read(&project, &readings, "nothing", 0), None);
}

#[test]
fn an_item_of_a_list_feed_reads_by_its_index() {
    let (mut project, _engine, _folder) = open();
    let on = |on| Step { on };
    add(
        &mut project,
        Dial {
            steps: vec![on(false), on(true)],
            ..Dial::default()
        },
    );
    let readings = Readings::new();
    assert_eq!(
        read(&project, &readings, "steps", 0),
        Some(Feed::List(vec![0.0, 1.0]))
    );
    assert_eq!(
        read(&project, &readings, "steps.1", 0),
        Some(Feed::Number(1.0))
    );
    // Past the end is silence, not a missing feed: a list may be shorter for a while.
    assert_eq!(
        read(&project, &readings, "steps.7", 0),
        Some(Feed::Number(0.0))
    );
    assert_eq!(read(&project, &readings, "depth.0", 0), None);
}

#[test]
fn every_feed_of_a_frame_shares_one_take_of_the_peaks() {
    let (mut project, _engine, _folder) = open();
    add(&mut project, Dial::default());
    let peaks = project.peaks(&id("dial"), "level").unwrap();
    let mut readings = Readings::new();
    peaks.record_block([&[0.5], &[-0.25]]);
    assert_eq!(
        read(&project, &readings, "peak_left", 0),
        Some(Feed::Number(0.5))
    );
    assert_eq!(
        read(&project, &readings, "peak_right", 0),
        Some(Feed::Number(0.25))
    );
    assert_eq!(
        read(&project, &readings, "peak", 0),
        Some(Feed::Number(0.5))
    );
    readings.next_frame(1.0 / 60.0);
    assert_eq!(
        read(&project, &readings, "peak", 0),
        Some(Feed::Number(0.0))
    );
}

#[test]
fn facts_show_nested_fields_and_what_the_tool_adds_and_compare_cheaply() {
    let (mut project, _engine, _folder) = open();
    add(&mut project, Dial::default());
    let of = |project: &Project| facts::of(project, &NoSources, &id("dial"), None).unwrap();
    let first = of(&project);
    let value = first.to_value();
    assert_eq!(value["title"], "Dial");
    assert_eq!(value["deep"], false);
    let fields = &value["fields"];
    let named = |name: &str| {
        (fields.as_array().unwrap().iter())
            .find(|field| field["name"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(named("depth")["kind"], "number");
    assert_eq!(named("depth")["unit"], "percent");
    assert_eq!(named("automated")["kind"], "switch");
    assert_eq!(named("steps")["kind"], "list");
    assert_eq!(named("steps")["item"]["fields"][0]["name"], "on");
    assert_eq!(named("page")["kind"], "group");
    assert_eq!(named("page")["fields"][0]["kind"], "choice");
    assert_eq!(named("page")["fields"][0]["options"][1]["value"], "high");
    assert!(first == of(&project));

    let mut edit = project.begin("Deeper");
    let dial = project.resolve::<Dial>(&id("dial")).unwrap();
    project
        .update(&mut edit, &dial, |dial| dial.depth = 0.9)
        .unwrap();
    project.finish(edit).unwrap();
    assert!(first != of(&project));
    assert_eq!(of(&project).to_value()["deep"], true);
}
