//! The start of every frame: Bun, the project, the engine and what moves.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::PoisonError;
use std::time::Duration;

use rune::Tx;
use serde_json::Value;
use sound_core::{GROUPING_WINDOW, InstanceId};
use sound_typescript::next::{FRAME_INTERVAL, Looped, Tools};

use crate::controls;
use crate::theme::Theme;
use crate::tree;

use super::{DID_NOTHING, FAILED, REPORT, State};

/// How often a playing window draws: what moves, moves at 60 frames a second.
const FRAME: Duration = Duration::from_millis(16);
/// How often an idle window looks at what has no way to wake it: plugins, and sounds that load.
const IDLE: Duration = Duration::from_millis(250);

impl State {
    /// Shows what the views name, and waits for their trees in a run without a window. Only
    /// what the trees bind now stays bound.
    fn discover(&mut self, settle: bool) -> bool {
        let changed = self.show_named(settle);
        self.trees.keep(self);
        self.lives.keep(&tree::bound(self));
        self.planes.discover(self);
        if let Some(problems) = self.planes.misplaced(self) {
            self.project.set_problems_in(tree::MISPLACED, problems);
        }
        changed
    }

    fn show_named(&mut self, settle: bool) -> bool {
        let mut changed = false;
        loop {
            let shown = controls::collect(self).shown;
            let cards = (shown.iter())
                .filter_map(|id| Some((id.clone(), self.card(id)?)))
                .collect();
            self.cards.retain(|id, _| shown.contains(id));
            let holds = self.holds(&shown);
            let more = shown != self.shown;
            self.shown = shown.clone();
            let Some(tools) = &mut self.tools else {
                return changed;
            };
            if !tools.show(shown, cards, holds) {
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

    /// The card of `id` for the generic card, unless its tool draws a view of its own. Its
    /// fields follow its instance's tool and that tool's code: made again only when either
    /// changed.
    fn card(&mut self, id: &InstanceId) -> Option<Value> {
        let tool = self.project.tool_of(id)?.to_string();
        let tools = self.tools.as_ref();
        if tools.is_some_and(|tools| tools.has_view(&tool)) {
            return None;
        }
        let loads = tools.map_or(0, Tools::loads);
        let fresh = !matches!(self.cards.get(id), Some((made_for, made_at, _)) if *made_for == tool && *made_at == loads);
        if fresh {
            let card = crate::fields::card_of(self, id)?;
            self.cards.insert(id.clone(), (tool, loads, card));
        }
        self.cards.get(id).map(|(_, _, card)| card.clone())
    }

    /// The numbers a lane moves of each instance of `shown`, for `held` of its view.
    fn holds(&self, shown: &BTreeSet<InstanceId>) -> BTreeMap<InstanceId, Vec<String>> {
        sound_typescript::next::holds(&self.project, shown)
    }

    /// Waits for Bun; a run without a window fails when it does not finish.
    fn settle_tools(&mut self) -> bool {
        let Some(tools) = &mut self.tools else {
            return false;
        };
        let changed = tools.settle(&mut self.project).unwrap_or_else(|why| {
            *FAILED.lock().unwrap_or_else(PoisonError::into_inner) = Some(why);
            true
        });
        if changed {
            self.trees_came();
        }
        changed
    }

    /// New trees came: what drags moved is where the records say now, but a drag that goes on.
    fn trees_came(&self) {
        let held = self.gesture.as_ref().and_then(tree::Gesture::dragging);
        self.lives.drop_drags(self, held);
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
    let (mut changed, next) = frame(state, seconds);
    changed |= state.planes.focus(tx.focused());
    if let Some(next) = next {
        tx.wake_after(next);
    }
    tx.instant();
    changed
}

/// One frame `seconds` after the start. Whether what the window shows changed, and, in a
/// window, when the next frame is due: Bun, the watchers and the engine wake it sooner.
pub(crate) fn frame(state: &mut State, seconds: f64) -> (bool, Option<Duration>) {
    let headless = state.headless();
    let mut changed = false;
    // A press that a drag's release brings comes before the next frame.
    state.dragged = None;
    state.open_menu();
    if headless {
        changed |= state.settle();
        if let Err(error) = state.first_frame() {
            *FAILED.lock().unwrap_or_else(PoisonError::into_inner) = Some(format!("{error:#}"));
        }
        if let Err(error) = state.play_offline(seconds) {
            state.notice = Some(error.to_string());
        }
    }
    if let Err(error) = state.project.poll() {
        state.notice = Some(error.to_string());
    }
    let (problems, errors) = runtime::tick(&mut state.project, &state.plugins);
    if let Some(problem) = problems.first() {
        state.notice = Some(problem.to_string());
    }
    if let Some(error) = errors.first() {
        state.notice = Some(error.to_string());
    }
    let events = state.project.drain_events();
    changed |= !events.is_empty();
    if let Some(tools) = &mut state.tools {
        let drawn = tools.poll(&mut state.project, &events);
        if drawn {
            state.trees_came();
        }
        changed |= drawn;
    }
    // What the views name changes only with what they show, and a lane bound or let go as a
    // behaviour answers, which says nothing.
    let holds = state.holds(&state.shown);
    let moved = state
        .tools
        .as_ref()
        .is_some_and(|tools| *tools.holds() != holds);
    if changed || moved {
        changed |= state.discover(headless);
    }
    if let Ok(status) = state.project.engine().poll() {
        changed |= status.playing != state.playing;
        (state.playing, state.playhead) = (status.playing, status.playhead_tick);
        state.engine_time = status.frames as f64 / f64::from(state.project.clock().sample_rate());
    }
    if run_loops(state, seconds) && headless {
        changed |= state.settle();
    }
    if let Some(tools) = &mut state.tools
        && !tools.take_failed_actions().is_empty()
    {
        state.notice = Some(DID_NOTHING.to_string());
    }
    if let Some(tools) = &mut state.tools {
        if tools.loads() != state.theme_loads {
            state.theme_loads = tools.loads();
            let (theme, problem) = Theme::of(state.project.root());
            (state.theme, state.notice) = (theme, problem.or(state.notice.take()));
            changed = true;
        }
    }
    let dt = (seconds - state.last).max(0.0);
    state.last = seconds;
    let moving = state.lives.update(state, dt);
    changed |= state.lives.take_wider();
    if headless {
        *REPORT.lock().unwrap_or_else(PoisonError::into_inner) = Some(state.report());
        return (changed, None);
    }
    let tools = state.tools.as_ref();
    let mut next = if state.playing || moving { FRAME } else { IDLE };
    if tools.is_some_and(|tools| tools.loops(&state.project)) {
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
    let Some(tools) = &mut state.tools else {
        return false;
    };
    if dt < FRAME_INTERVAL.as_secs_f64() {
        return false;
    }
    let looped = tools.run_loops(
        &state.project,
        state.playing,
        state.playhead,
        state.engine_time,
        dt as f32,
    );
    if looped != Looped::Busy {
        state.looped = seconds;
    }
    looped == Looped::Sent
}
