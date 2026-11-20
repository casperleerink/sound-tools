#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::sync::{Arc, mpsc};

use plugin_host::{Plugins, ScanCache, ScanCommand};
use serde_json::{Value, json};
use sound_core::{Engine, InstanceId, Ticks};

use super::{Drag, Held, Loop, Session};

/// Ticks in a bar of 4/4.
const BAR: u64 = 3_840;
const TRACK: &str = "song/t00";
const SYNTH: &str = "song/t00/synth";
/// The synth's cutoff when its record leaves it out.
const DEFAULT_CUTOFF: f64 = 2_000.0;

/// A session of the DAW on the offline engine, with what plays it and what its waker said.
struct Opened {
    session: Session,
    engine: Engine,
    plugins: Plugins,
    woken: mpsc::Receiver<()>,
    _folder: tempfile::TempDir,
}

impl Opened {
    /// The DAW, with these files written over it.
    fn daw(files: &[(&str, Value)]) -> Self {
        assert!(sound_typescript::has_bun(), "see tooling/fetch-bun.sh");
        let folder = tempfile::tempdir().unwrap();
        crate::templates::write(folder.path(), crate::templates::named("DAW").unwrap()).unwrap();
        for (path, contents) in files {
            std::fs::write(folder.path().join(path), contents.to_string()).unwrap();
        }
        let plugins =
            Plugins::listing(Vec::new(), ScanCommand::this_program().unwrap(), ScanCache::none());
        let (control, engine) = Engine::new(crate::OFFLINE);
        let project = crate::open_or_create_with(folder.path(), control, plugins.clone()).unwrap();
        let (wake, woken) = mpsc::channel();
        let wake = std::sync::Mutex::new(wake);
        let waker = Arc::new(move || _ = wake.lock().unwrap().send(()));
        let mut opened = Self {
            session: Session::new(project, plugins.clone(), waker),
            engine,
            plugins,
            woken,
            _folder: folder,
        };
        opened.play(512);
        opened
    }

    /// The DAW whose first track's lane plays `points` into its synth's cutoff.
    fn automated(points: &[(u64, f64)]) -> Self {
        let points: Vec<Value> = (points.iter())
            .map(|(tick, value)| json!({ "tick": tick, "value": value }))
            .collect();
        let opened = Self::daw(&[
            ("state/song/t00/instance.json", automated_track()),
            (
                "state/song/t00/lane-cutoff.json",
                json!({ "tool": "lane", "state": { "points": points } }),
            ),
        ]);
        assert_eq!(opened.session.project().problems(), []);
        opened
    }

    /// Plays `frames` on the offline engine in device blocks, then polls as a frame does.
    fn play(&mut self, frames: usize) {
        let Self {
            session,
            engine,
            plugins,
            ..
        } = self;
        let mut left = frames;
        let mut buffer = vec![0.0; 512 * engine.channels()];
        while left > 0 {
            let size = left.min(512);
            let output = &mut buffer[..size * engine.channels()];
            (session.background(|project| crate::render_block(project, engine, plugins, output)))
                .unwrap();
            left -= size;
        }
        session.poll();
    }

    fn record(&self, id: &str) -> Value {
        let id = InstanceId::new(id).unwrap();
        let state = self.session.project().state_json(&id).unwrap();
        serde_json::from_str(&state).unwrap()
    }

    fn cutoff(&self) -> f64 {
        self.record(SYNTH)["cutoff_hz"].as_f64().unwrap()
    }

    fn undo_label(&self) -> Option<String> {
        self.session.project().undo_label()
    }

    /// Writes `contents` at `path` as an agent would, and applies it.
    fn write_outside(&mut self, path: &str, contents: Option<Value>) {
        let file = self.session.project().root().join(path);
        match contents {
            Some(contents) => std::fs::write(&file, contents.to_string()).unwrap(),
            None => std::fs::remove_file(&file).unwrap(),
        }
        (self.session.background(|project| project.apply_outside_changes(&[file]))).unwrap();
    }

    /// The cutoff the lane holds, by tick.
    fn lane(&self) -> Vec<(u64, f64)> {
        let lane = self.record("song/t00/lane-cutoff");
        let points = lane["points"].as_array().cloned().unwrap_or_default();
        (points.iter())
            .map(|point| (point["tick"].as_u64().unwrap(), point["value"].as_f64().unwrap()))
            .collect()
    }
}

fn automated_track() -> Value {
    let automation = json!([{ "lane": "lane-cutoff", "target": "synth.cutoff_hz" }]);
    json!({ "tool": "track", "state": { "title": "Track 1", "automation": automation } })
}

fn id(id: &str) -> InstanceId {
    InstanceId::new(id).unwrap()
}

/// A drag opens no step at its press, publishes every move so the sound follows, and lets go as
/// one undo step named by its label.
#[test]
fn a_drag_is_one_undo_step_and_a_press_that_moves_nothing_is_none() {
    let mut opened = Opened::daw(&[]);
    let before = opened.undo_label();
    opened.session.finish(Drag::new("Change cutoff"));
    assert_eq!(opened.undo_label(), before);

    let mut drag = Drag::new("Change cutoff");
    for cutoff in [600.0, 700.0, 800.5] {
        (opened.session.play(&mut drag, &id(TRACK), &[("synth.cutoff_hz", cutoff)])).unwrap();
        assert_eq!(opened.cutoff(), cutoff);
    }
    assert!(opened.session.gesture_open());
    opened.session.finish(drag);
    assert!(!opened.session.gesture_open());
    assert_eq!(opened.undo_label().as_deref(), Some("Change cutoff"));
    opened.session.undo();
    assert_eq!(opened.cutoff(), DEFAULT_CUTOFF);
    assert_eq!(opened.undo_label(), before);
}

/// Escape puts back the state from before the drag, with no step.
#[test]
fn escape_puts_back_the_state_from_before_the_drag() {
    let mut opened = Opened::daw(&[]);
    let before = (opened.record(SYNTH), opened.undo_label());
    let mut drag = Drag::new("Change cutoff");
    (opened.session.play(&mut drag, &id(SYNTH), &[("cutoff_hz", 600.0)])).unwrap();
    opened.session.cancel(drag);
    assert_eq!((opened.record(SYNTH), opened.undo_label()), before);
}

/// A typed value is a drag that goes there and lets go at once.
#[test]
fn a_typed_value_is_one_step() {
    let mut opened = Opened::daw(&[]);
    (opened.session.set("Change cutoff", &id(SYNTH), &[("cutoff_hz", 440.0)])).unwrap();
    assert!(!opened.session.gesture_open());
    assert_eq!(opened.cutoff(), 440.0);
    assert_eq!(opened.undo_label().as_deref(), Some("Change cutoff"));
}

/// Another drag closes a drag's gesture as its step; the drag's next move opens it again.
#[test]
fn a_gesture_another_closed_opens_again_on_the_next_move() {
    let mut opened = Opened::daw(&[]);
    let mut cutoff = Drag::new("Change cutoff");
    let mut resonance = Drag::new("Change resonance");
    (opened.session.play(&mut cutoff, &id(SYNTH), &[("cutoff_hz", 600.0)])).unwrap();
    (opened.session.play(&mut resonance, &id(SYNTH), &[("resonance", 0.5)])).unwrap();
    assert_eq!(opened.undo_label().as_deref(), Some("Change cutoff"));
    opened.session.finish(resonance);
    (opened.session.play(&mut cutoff, &id(SYNTH), &[("cutoff_hz", 700.0)])).unwrap();
    assert!(opened.session.gesture_open());
    opened.session.finish(cutoff);
    assert_eq!(opened.undo_label().as_deref(), Some("Change cutoff"));
    opened.session.undo();
    assert_eq!(opened.cutoff(), 600.0);
}

/// A record deleted from outside during its drag finishes the gesture: a cancel would bring
/// it back.
#[test]
fn a_record_deleted_during_its_drag_finishes_the_gesture() {
    let mut opened = Opened::daw(&[]);
    let mut drag = Drag::new("Change cutoff");
    (opened.session.play(&mut drag, &id(SYNTH), &[("cutoff_hz", 600.0)])).unwrap();
    opened.write_outside("state/song/t00/synth.json", None);
    opened.session.poll();
    assert!(!opened.session.gesture_open());
    opened.session.cancel(drag);
    assert_eq!(opened.session.project().tool_of(&id(SYNTH)), None);
}

/// Undo waits for an open drag and for a take that records, and says why.
#[test]
fn undo_waits_for_a_drag_and_a_take_and_says_why() {
    let mut opened = Opened::daw(&[]);
    let mut drag = Drag::new("Change cutoff");
    (opened.session.play(&mut drag, &id(SYNTH), &[("cutoff_hz", 600.0)])).unwrap();
    opened.session.undo();
    assert_eq!(opened.session.notice(), Some("Let go of the drag first."));
    assert_eq!(opened.cutoff(), 600.0);
    opened.session.finish(drag);

    opened.session.set_recording(true);
    opened.session.redo();
    assert_eq!(opened.session.notice(), Some("Stop recording first."));
    opened.session.set_recording(false);
    opened.session.undo();
    assert_eq!(opened.session.notice(), None);
    assert_eq!(opened.cutoff(), DEFAULT_CUTOFF);
}

/// The notice of a failed edit goes with the next good one; one from the system stays until it
/// is dismissed.
#[test]
fn a_good_edit_clears_only_the_notice_of_a_failed_edit() {
    let mut opened = Opened::daw(&[]);
    let edit = |opened: &mut Opened, cutoff: Value| {
        opened.session.edit(|project| {
            let mut edit = project.begin("Change cutoff");
            project.update_json(&mut edit, &id(SYNTH), |state| state["cutoff_hz"] = cutoff)?;
            project.finish(edit)
        })
    };
    assert_eq!(edit(&mut opened, json!("loud")), None);
    assert!(opened.session.notice().is_some());
    edit(&mut opened, json!(600)).unwrap();
    assert_eq!(opened.session.notice(), None);

    opened.session.report("The sound device went away.");
    edit(&mut opened, json!(700)).unwrap();
    assert_eq!(opened.session.notice(), Some("The sound device went away."));
    opened.session.dismiss_notice();
    assert_eq!(opened.session.notice(), None);
}

/// An engine that stopped fails every poll and is said once.
#[test]
fn a_stopped_engine_is_said_once() {
    let mut opened = Opened::daw(&[]);
    let Opened {
        session, engine, ..
    } = &mut opened;
    drop(std::mem::replace(engine, Engine::new(crate::OFFLINE).1));
    session.poll();
    assert!(session.notice().is_some());
    session.dismiss_notice();
    session.poll();
    assert_eq!(session.notice(), None);
}

/// The loop, the click, the computer keys and writing automation are no edit, and the clock
/// told to code says them.
#[test]
fn the_transport_state_is_no_edit_and_the_clock_says_it() {
    let mut opened = Opened::daw(&[]);
    let before = opened.undo_label();
    let looped = opened.session.transport().looped.clone();
    assert_eq!(looped, Loop { range: Ticks(0)..Ticks(4 * BAR), on: false });
    assert!(!opened.session.transport().click);

    opened.session.set_loop(Loop {
        range: Ticks(0)..Ticks(BAR),
        on: true,
    });
    opened.session.set_click(true);
    opened.session.set_computer_keys(true);
    opened.session.set_writes_automation(true);
    opened.session.background(|project| project.engine().play());
    // Two bars and a half at 120 bpm: past the end of the loop.
    opened.play(5 * 48_000);
    let clock = opened.session.clock().transport;
    assert!(clock.playing && clock.tick < BAR, "{clock:?}");
    assert_eq!(clock.repeat, Some([0, BAR]));
    assert!(clock.click && clock.keys && clock.automation);
    assert_eq!(opened.undo_label(), before);
}

/// The selection is worked out from the project as it is now; live notes go to the nearest
/// instance that takes notes, or with nothing selected, to every one at the top.
#[test]
fn the_selection_follows_the_project_and_live_notes_go_to_the_nearest_notes() {
    let mut opened = Opened::daw(&[]);
    let project = opened.session.project();
    let notes_of = |path: &str| project.input_port(&id(path), sound_notes::NOTES_INPUT).unwrap();
    let (track, song) = (notes_of(TRACK), notes_of("song"));
    assert_eq!(opened.session.live_notes(), [song]);

    let mix = id("song/t00/mix");
    let span = Some((Ticks(BAR), Ticks(2 * BAR)));
    opened.session.select(Some(mix), span);
    assert_eq!(opened.session.live_notes(), [track]);
    assert_eq!(opened.session.selected_span(), span);
    opened.write_outside("state/song/t00/mix.json", None);
    assert_eq!(opened.session.selected(), None);
    assert_eq!(opened.session.selected_span(), None);
}

/// A path names a number of an instance inside when it starts with one, else of its own record:
/// `pads.36.pan` of a kit is its own, as the kit has no `pads` inside it.
#[test]
fn a_path_names_an_instance_inside_only_when_there_is_one() {
    let opened = Opened::daw(&[]);
    let project = opened.session.project();
    let number_at = |instance: &str, path| super::gesture::number_at(project, &id(instance), path);
    assert_eq!(number_at("song", "t00/synth.cutoff_hz"), (id(SYNTH), "cutoff_hz"));
    assert_eq!(number_at(TRACK, "mix.gain_db"), (id("song/t00/mix"), "gain_db"));
    assert_eq!(number_at(SYNTH, "pads.36.pan"), (id(SYNTH), "pads.36.pan"));
    assert_eq!(number_at(SYNTH, "bands[0].gain_db"), (id(SYNTH), "bands[0].gain_db"));
}

/// A number a lane moves is held: a move of it changes nothing, and a lane that arrives
/// during a drag ends its gesture, so undo does not wait for the let-go.
#[test]
fn a_lane_holds_its_number_and_ends_a_drag_of_it() {
    let mut opened = Opened::daw(&[]);
    let mut drag = Drag::new("Change cutoff");
    (opened.session.play(&mut drag, &id(SYNTH), &[("cutoff_hz", 600.0)])).unwrap();
    opened.write_outside(
        "state/song/t00/lane-cutoff.json",
        Some(json!({ "tool": "lane", "state": { "points": [{ "tick": 0, "value": 300 }] } })),
    );
    opened.write_outside("state/song/t00/instance.json", Some(automated_track()));
    opened.play(512);
    let shown = BTreeSet::from([id("song"), id(TRACK)]);
    let held = opened.session.held(&shown);
    assert_eq!(held[&id(TRACK)], ["synth.cutoff_hz"]);
    assert_eq!(held[&id("song")], ["t00/synth.cutoff_hz"]);

    let refused = opened.session.play(&mut drag, &id(TRACK), &[("synth.cutoff_hz", 700.0)]);
    assert_eq!(refused, Err(Held(vec!["synth.cutoff_hz".to_string()])));
    assert!(!opened.session.gesture_open());
    assert_eq!(opened.cutoff(), 600.0);
    opened.session.undo();
    assert_eq!(opened.cutoff(), DEFAULT_CUTOFF);
    opened.session.finish(drag);
    let typed = opened.session.set("Change cutoff", &id(SYNTH), &[("cutoff_hz", 900.0)]);
    assert!(typed.is_err());
}

/// While automation is written and the piece plays, a drag of a number a lane moves writes what
/// the hand plays into the lane: one step "Write automation", and the record keeps its number.
#[test]
fn a_drag_of_a_held_number_while_it_plays_writes_into_its_lane() {
    let far = 64 * BAR;
    let mut opened = Opened::automated(&[(0, 1_000.0), (far, 1_000.0)]);
    opened.session.set_writes_automation(true);
    opened.session.background(|project| project.engine().play());
    opened.play(24_000);
    assert_eq!(opened.session.held(&BTreeSet::from([id(TRACK)])).len(), 0);
    let before = opened.undo_label();
    let mut drag = Drag::new("Change cutoff");
    (opened.session.play(&mut drag, &id(TRACK), &[("synth.cutoff_hz", 2_500.0)])).unwrap();
    // The hand holds still, then moves on: each poll samples it.
    for _ in 0..10 {
        opened.play(4_800);
    }
    (opened.session.play(&mut drag, &id(TRACK), &[("synth.cutoff_hz", 3_000.0)])).unwrap();
    for _ in 0..5 {
        opened.play(4_800);
    }
    opened.session.finish(drag);

    let written = opened.lane();
    assert_eq!(written.first(), Some(&(0, 1_000.0)), "{written:?}");
    assert_eq!(written.last(), Some(&(far, 1_000.0)), "{written:?}");
    let values: Vec<f64> = written.iter().map(|(_, value)| *value).collect();
    assert!(values.contains(&2_500.0) && values.contains(&3_000.0), "{written:?}");
    assert_eq!(opened.cutoff(), DEFAULT_CUTOFF);
    assert_eq!(opened.undo_label().as_deref(), Some("Write automation"));
    opened.session.undo();
    assert_eq!(opened.lane(), [(0, 1_000.0), (far, 1_000.0)]);
    assert_eq!(opened.undo_label(), before);
}

/// A typed value while automation is written lands in the lane where the piece plays.
#[test]
fn a_typed_value_while_it_plays_lands_in_the_lane_where_it_plays() {
    let far = 64 * BAR;
    let mut opened = Opened::automated(&[(0, 1_000.0), (far, 1_000.0)]);
    opened.session.background(|project| project.engine().play());
    opened.play(24_000);
    let refused = opened.session.set("Change cutoff", &id(SYNTH), &[("cutoff_hz", 2_500.0)]);
    assert!(refused.is_err());

    opened.session.set_writes_automation(true);
    let playhead = opened.session.status().playhead_tick.0;
    (opened.session.set("Change cutoff", &id(SYNTH), &[("cutoff_hz", 2_500.0)])).unwrap();
    let typed: Vec<_> = (opened.lane().into_iter())
        .filter(|(_, value)| *value == 2_500.0)
        .collect();
    assert!(!typed.is_empty());
    assert!(typed.iter().all(|(tick, _)| tick.abs_diff(playhead) <= 120), "{typed:?}");
    assert_eq!(opened.cutoff(), DEFAULT_CUTOFF);
    assert_eq!(opened.undo_label().as_deref(), Some("Write automation"));
}

/// Escape during a drag that writes automation leaves the lane, the record and the history as
/// they were.
#[test]
fn escape_during_a_drag_that_writes_automation_leaves_the_lane_as_it_was() {
    let mut opened = Opened::automated(&[(0, 300.0), (BAR, 3_000.0)]);
    opened.session.set_writes_automation(true);
    opened.session.background(|project| project.engine().play());
    opened.play(4_800);
    let before = (opened.lane(), opened.record(SYNTH), opened.undo_label());
    let mut drag = Drag::new("Change cutoff");
    (opened.session.play(&mut drag, &id(SYNTH), &[("cutoff_hz", 2_000.0)])).unwrap();
    opened.play(4_800);
    opened.session.cancel(drag);
    assert_eq!((opened.lane(), opened.record(SYNTH), opened.undo_label()), before);
}

/// A session that goes writes the drag it still had open.
#[test]
fn a_drag_open_as_the_session_goes_is_written() {
    let mut opened = Opened::daw(&[]);
    let mut drag = Drag::new("Change cutoff");
    (opened.session.play(&mut drag, &id(SYNTH), &[("cutoff_hz", 600.0)])).unwrap();
    let Opened {
        session, _folder, ..
    } = opened;
    drop(session);
    let file = std::fs::read_to_string(_folder.path().join("state/song/t00/synth.json"));
    let record: Value = serde_json::from_str(&file.unwrap()).unwrap();
    assert_eq!(record["state"]["cutoff_hz"].as_f64(), Some(600.0));
    drop(drag);
}

/// A render runs in a process of its own: the session shows how far it is, and a poll gives
/// what it said once it ends; the last line it printed on stderr is the error of one that
/// failed. One at a time.
#[cfg(unix)]
#[test]
fn a_render_runs_in_its_own_process_and_says_how_far_it_is() {
    let mut opened = Opened::daw(&[]);
    let scripts = tempfile::tempdir().unwrap();
    let program = |name: &str, body: &str| {
        use std::os::unix::fs::PermissionsExt;
        let path = scripts.path().join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    };
    let good = program(
        "good",
        "echo 'progress: 50'\necho 'note: quiet'\necho 'error: a plugin left out'",
    );
    let file = scripts.path().join("out.wav");
    (opened.session.render(&good, "Rendering…", file.clone(), &[])).unwrap();
    assert_eq!(opened.session.rendering(), Some(("Rendering…", 0.0)));
    let busy = opened.session.render(&good, "Again", file.clone(), &[]);
    assert!(busy.is_err());
    let rendered = wait_for_render(&mut opened);
    let rendered = rendered.unwrap();
    assert_eq!(rendered.notes, ["quiet"]);
    assert_eq!(rendered.problems, ["a plugin left out"]);
    assert_eq!(opened.session.rendering(), None);

    let bad = program("bad", "echo 'Error: the disk is full' >&2\nexit 1");
    (opened.session.render(&bad, "Rendering…", file, &[])).unwrap();
    let failed = wait_for_render(&mut opened).unwrap_err();
    assert_eq!(failed.to_string(), "the disk is full");
}

/// Polls each time the render wakes the session, until it ends.
fn wait_for_render(opened: &mut Opened) -> anyhow::Result<super::Rendered> {
    loop {
        opened.woken.recv().unwrap();
        if let Some(rendered) = opened.session.poll().rendered {
            return rendered;
        }
    }
}
