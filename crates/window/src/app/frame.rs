//! The start of every frame: Bun, the project, the engine and what moves.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::PoisonError;
use std::time::Duration;

use rune::Tx;
use sound_core::{GROUPING_WINDOW, InstanceId};
use sound_typescript::window::{FRAME_INTERVAL, Looped, Tools};

use crate::controls;
use crate::feeds::UNREAD;
use crate::plugin_windows::Spare;
use crate::theme::Theme;
use crate::tree;

use super::{DID_NOTHING, FAILED, REPORT, State};

/// How often a playing window draws: what moves, moves at 60 frames a second.
const FRAME: Duration = Duration::from_millis(16);
/// How often an idle window looks at what has no way to wake it: plugins, and sounds that load.
const IDLE: Duration = Duration::from_millis(250);
/// How often facts are read for a change no edit brings, such as a file that arrived: reading
/// those of 900 instances took 1.5 ms.
const FACTS: f64 = 0.25;
/// How long quitting waits for the takes to end and their code to keep them.
const FINISHING: Duration = Duration::from_secs(5);

impl State {
    /// Shows what the views name, and waits for their trees in a run without a window. Only
    /// what the trees bind now stays bound.
    fn discover(&mut self, settle: bool) -> bool {
        let changed = self.show_named(settle);
        // What follows reads only the trees, which a frame mostly leaves as they were.
        let heard = self.tools.as_ref().map_or(0, Tools::trees_heard);
        if !self.walked.unwalked(heard, &self.shown) {
            return changed;
        }
        self.lives.keep(&tree::bound(self));
        tree::report_missing(self);
        self.planes.discover(self);
        if let Some(problems) = self.planes.misplaced(self) {
            (self.session).background(|project| project.set_problems_in(tree::MISPLACED, problems));
        }
        if let Some(problems) = self.feeds.unread(self) {
            (self.session).background(|project| project.set_problems_in(UNREAD, problems));
        }
        changed
    }

    fn show_named(&mut self, settle: bool) -> bool {
        let mut changed = false;
        loop {
            let shown = controls::collect(self).shown;
            let more = shown != self.shown;
            self.shown = shown.clone();
            let gathered = self.gathered(shown);
            let Some(tools) = &mut self.tools else {
                return changed;
            };
            if !tools.show(gathered) {
                return changed;
            }
            if !settle {
                return true;
            }
            changed |= self.settle_tools();
            if !more {
                return changed;
            }
        }
    }

    /// The numbers a lane moves of each instance of `shown`, for `held` of its view.
    pub(super) fn holds(&self, shown: &BTreeSet<InstanceId>) -> BTreeMap<InstanceId, Vec<String>> {
        self.session.held(shown)
    }

    /// Polls the session and gives the code and the agent what changed in the project. Whether
    /// what the window shows changed.
    pub(super) fn poll_session(&mut self) -> bool {
        let polled = self.poll_renders();
        let mut changed = polled.redraw || !polled.events.is_empty();
        if let Some(tools) = &mut self.tools {
            let drawn = (self.session).background(|project| tools.poll(project, &polled.events));
            if drawn {
                self.trees_came();
            }
            changed |= drawn;
        }
        // What Bun's edits did while a run without a window waited for it, first.
        let mut events = std::mem::take(&mut self.heard);
        events.extend(polled.events);
        changed |= crate::sidebar::pump(self, &events);
        changed | self.take_in(&events)
    }

    /// Waits for Bun; a run without a window fails when it does not finish.
    fn settle_tools(&mut self) -> bool {
        let Some(tools) = &mut self.tools else {
            return false;
        };
        // The rest of the window hears what Bun's edits did as a window's poll would have.
        let heard = &mut self.heard;
        let settled = (self.session).background(|project| tools.settle(project, heard));
        self.session.heard(&self.heard);
        let mut changed = settled.unwrap_or_else(|why| {
            *FAILED.lock().unwrap_or_else(PoisonError::into_inner) = Some(why);
            true
        });
        self.imported();
        self.dropped();
        changed |= self.serve_wants();
        if changed {
            self.trees_came();
        }
        changed
    }

    /// New trees came: what drags moved is where the records say now, but a drag that goes on.
    fn trees_came(&mut self) {
        let held = self.gesture.as_ref().and_then(tree::Gesture::dragging);
        self.lives.drop_drags(self, held);
        self.trees_moved = true;
    }

    pub(crate) fn settle(&mut self) -> bool {
        self.settle_tools() | self.discover(true)
    }
}

pub(crate) fn setup(state: &mut State, waker: rune::Waker) {
    *state.waker.lock().unwrap_or_else(PoisonError::into_inner) = Some(waker);
}

/// The start of every frame: Bun, the project, the engine and what moves.
pub(crate) fn poll(state: &mut State, seconds: f64, tx: &mut Tx) -> bool {
    // What a plugin's window does not use: the shortcuts, then the computer keys.
    for spare in state.plugin_windows.take_spare_keys() {
        match &spare {
            Spare::Down { key, repeat } => super::keys::key_down(state, key, *repeat, tx),
            Spare::Up(key) => super::keys::on_key_up(state, key, tx),
            Spare::LetGo => {
                state.let_go_of_keys();
                false
            }
        };
    }
    let (mut changed, next) = frame(state, seconds);
    keep_focus_in_view(state, tx);
    changed |= state.planes.focus(tx.focused());
    state.open_file_dialogs(tx);
    if let Some(size) = state.panel.widen.take() {
        tx.resize_window(size);
    }
    crate::sidebar::open_pages(state, tx);
    if let Some(next) = next {
        tx.wake_after(next);
    }
    tx.instant();
    changed
}

/// What came before the focused element in new trees moved it: it stays in view, where a
/// wheel need not keep it.
pub(crate) fn keep_focus_in_view(state: &mut State, tx: &mut Tx) {
    if std::mem::take(&mut state.trees_moved)
        && let Some(focused) = tx.focused()
    {
        let focused = focused.to_string();
        tx.scroll_into_view(focused);
    }
}

/// One frame `seconds` after the start. Whether what the window shows changed, and, in a
/// window, when the next frame is due: Bun, the watchers and the engine wake it sooner.
pub(crate) fn frame(state: &mut State, seconds: f64) -> (bool, Option<Duration>) {
    let headless = state.headless();
    let mut changed = false;
    // A press that a drag's release brings comes before the next frame.
    state.dragged = None;
    if state.modals.get() > state.modals_seen {
        state.let_go_of_pointer();
    }
    state.modals_seen = state.modals.get();
    state.open_menu();
    if headless {
        changed |= state.settle();
        state.feeds.wait_for_files();
        if let Err(error) = state.first_frame() {
            *FAILED.lock().unwrap_or_else(PoisonError::into_inner) = Some(format!("{error:#}"));
        }
        if let Err(error) = state.play_offline(seconds) {
            state.session.report(error);
        }
    }
    state.plugin_windows.settle(&state.plugins, seconds);
    changed |= state.poll_session();
    // The list of the files that are not live goes with the last of them.
    if state.problems_open && state.files_not_live() == 0 {
        state.problems_open = false;
    }
    state.imported();
    changed |= state.menu_failures() | state.devices_listed();
    state.dropped();
    changed |= state.serve_wants();
    super::plays::settle(state);
    // What the views name changes only with what they show, and a lane bound or let go as a
    // behaviour answers, a key the composer gave an action, or facts, which say nothing.
    let holds = state.holds(&state.shown);
    let keys = state.action_keys();
    let facts_due = seconds - state.facts_read >= FACTS;
    if facts_due {
        state.facts_read = seconds;
    }
    let moved = (state.tools.as_ref())
        .is_some_and(|tools| *tools.holds() != holds || *tools.keys() != keys)
        || (facts_due && state.facts_now(&state.shown) != state.facts);
    if changed || moved {
        changed |= state.discover(headless);
    }
    let status = state.session.status();
    (state.playing, state.playhead) = (status.playing, status.playhead_tick);
    state.engine_time = status.frames as f64 / f64::from(state.project().clock().sample_rate());
    if run_loops(state, seconds) && headless {
        changed |= state.settle();
    }
    if let Some(tools) = &mut state.tools
        && !tools.take_failed_actions().is_empty()
    {
        state.session.report(DID_NOTHING);
    }
    if let Some(tools) = &mut state.tools {
        if tools.loads() != state.theme_loads {
            state.theme_loads = tools.loads();
            let (theme, problem) = Theme::of(state.project().root());
            state.theme = theme;
            if let Some(problem) = problem {
                state.session.report(problem);
            }
            changed = true;
        }
    }
    changed |= tree::follow_fields(state);
    changed |= super::history::follow(state);
    let dt = (seconds - state.last).max(0.0);
    state.last = seconds;
    state.feeds.next_frame(dt);
    let mut moving = state.lives.update(state, dt) || state.planes.zooming();
    changed |= state.follow_planes(dt);
    let (falling, light) = (state.transport).update(state.session.project(), state.playhead, dt);
    (moving, changed) = (moving || falling, changed || light);
    changed |= state.lives.take_wider();
    if headless {
        *REPORT.lock().unwrap_or_else(PoisonError::into_inner) = Some(state.report());
        return (changed, None);
    }
    let tools = state.tools.as_ref();
    let mut next = if state.playing || moving { FRAME } else { IDLE };
    if tools.is_some_and(|tools| tools.loops(state.session.project())) {
        // Overdue only while Bun runs the last step: it wakes the window when done.
        let left = FRAME_INTERVAL.saturating_sub(Duration::from_secs_f64(seconds - state.looped));
        next = next.min(if left.is_zero() { FRAME_INTERVAL } else { left });
    }
    if tools.is_some_and(Tools::waits) {
        next = next.min(GROUPING_WINDOW + FRAME);
    }
    (changed, Some(next))
}

/// Runs the control loops when their interval passed on the window's clock, the virtual one of
/// a run without a window. A step Bun is still busy with keeps its time for the next. Whether a
/// step went to Bun.
pub(super) fn run_loops(state: &mut State, seconds: f64) -> bool {
    let dt = seconds - state.looped;
    if state.tools.is_none() || dt < FRAME_INTERVAL.as_secs_f64() {
        return false;
    }
    let clock = state.session.clock();
    let Some(tools) = &mut state.tools else {
        return false;
    };
    let looped = tools.run_loops(state.session.project(), clock, dt as f32);
    if looped != Looped::Busy {
        state.looped = seconds;
    }
    looped == Looped::Sent
}

/// The main window closes: the app ends with it, not with the last plugin window open. The
/// quit runs `quitting`.
pub(crate) fn closing(_: &mut State, tx: &mut Tx) {
    tx.quit();
}

/// Before any window goes, so what was recorded is in the song or waits for the next open,
/// every plugin saves its state and lets go of its view, and the agent's writes end.
pub(crate) fn quitting(state: &mut State, tx: &mut Tx) {
    finish_takes(state);
    if let Some(tools) = &mut state.tools {
        tools.close(state.session.project());
    }
    state.session.close();
    state.plugin_windows.close_all(&state.plugins);
    crate::sidebar::before_quit(state, tx);
}

/// The piece stops, which ends the takes, and their code keeps them: frames until it did, or
/// until [`FINISHING`] passed.
fn finish_takes(state: &mut State) {
    if state.session.recording() {
        state.session.background(|project| project.engine().stop());
    }
    let (start, began) = (std::time::Instant::now(), state.last);
    while start.elapsed() < FINISHING {
        let keeping = state.tools.as_ref().is_some_and(Tools::keeping_takes);
        if !state.session.recording() && !keeping {
            return;
        }
        frame(state, began + start.elapsed().as_secs_f64());
        std::thread::sleep(Duration::from_millis(2));
    }
}
