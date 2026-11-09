//! Writing automation by hand: while the session writes automation and the project plays, a
//! drag of a number that a `lane` moves takes the number over, and what the hand plays becomes
//! points of the lane when the drag ends. This is touch: before the drag and after it the lane
//! plays as it did.
//!
//! The lane lets go of the number while the hand holds it, so the device follows its record,
//! which the drag moves. The hand is sampled on every poll, also while it stands still. At the
//! end of the drag the samples go into the lane, and the number goes back to its record from
//! before the drag, so the drag is one undo step that changes only the lane.

use serde_json::Value;
use sound_core::{InstanceId, Project, ProjectEdit, ProjectError, Ticks};
use sound_ui::{GestureFollower, Playhead};

use crate::lane::{self, LanePlayer, LaneUpdate};

/// The drag that writes into lanes, see the module.
#[derive(Default)]
pub(crate) struct Writer {
    touches: Vec<Touch>,
}

/// One number a hand holds, and what it played.
struct Touch {
    instance: InstanceId,
    number: String,
    lane: InstanceId,
    /// The number in the record before the drag, as JSON: `None` when the record left it out.
    before: Option<Value>,
    /// What the hand played, in runs of rising ticks: a stop or a jump ends one.
    runs: Vec<Vec<(Ticks, f32)>>,
    /// The jumps of the playhead and whether it played, at the last sample.
    jumps: u64,
    playing: bool,
}

impl GestureFollower for Writer {
    fn follow(
        &mut self,
        project: &mut Project,
        gesture: &ProjectEdit,
        playhead: Playhead,
        writes: bool,
    ) -> Result<(), ProjectError> {
        if writes && playhead.playing {
            self.take_over(project, gesture, playhead)?;
        }
        for touch in &mut self.touches {
            touch.sample(project, playhead);
        }
        Ok(())
    }

    fn finish(
        &mut self,
        project: &mut Project,
        gesture: &mut ProjectEdit,
    ) -> Result<(), ProjectError> {
        if self.touches.iter().any(|touch| !touch.runs.is_empty()) {
            gesture.rename("Write automation");
        }
        // The lane plays its new line before the number goes back to its record, so no block
        // plays the record between the two.
        let written = (self.touches.iter()).try_for_each(|touch| touch.write(project, gesture));
        let touches = std::mem::take(&mut self.touches);
        let released = self.let_go(project, &touches);
        let restored = (touches.iter()).try_for_each(|touch| touch.restore(project, gesture));
        written.and(released).and(restored)
    }

    fn cancel(&mut self, project: &mut Project) -> Result<(), ProjectError> {
        let touches = std::mem::take(&mut self.touches);
        self.let_go(project, &touches)
    }
}

impl Writer {
    /// Each number a lane moves that the gesture changed and no hand holds yet: its lane lets go.
    fn take_over(
        &mut self,
        project: &mut Project,
        gesture: &ProjectEdit,
        playhead: Playhead,
    ) -> Result<(), ProjectError> {
        let mut taken = Vec::new();
        for (instance, before) in gesture.before_json() {
            let (Some(lanes), Some(now)) = (project.lanes(instance), project.state_json(instance))
            else {
                continue;
            };
            let before: Option<Value> = before.and_then(|json| serde_json::from_str(&json).ok());
            let Ok(now) = serde_json::from_str::<Value>(&now) else {
                continue;
            };
            let mut played = Vec::new();
            lanes.values_at(playhead.tick, &mut played);
            for (number, _) in played {
                let held = (self.touches.iter())
                    .any(|touch| touch.instance == *instance && *touch.number == *number);
                let was = before.as_ref().and_then(|before| at_path(before, &number));
                let Some(lane) = lanes.source(&number) else {
                    continue;
                };
                if held
                    || was == at_path(&now, &number)
                    || project.tool_of(lane) != Some(lane::NAME)
                {
                    continue;
                }
                taken.push(Touch {
                    instance: instance.clone(),
                    number: number.to_string(),
                    lane: lane.clone(),
                    before: was.cloned(),
                    runs: Vec::new(),
                    jumps: playhead.jumps,
                    playing: false,
                });
            }
        }
        for touch in taken {
            project.send::<LanePlayer>(&touch.lane, lane::PROCESSOR, LaneUpdate::Held(true))?;
            self.touches.push(touch);
        }
        Ok(())
    }

    /// The lanes of `touches` play again.
    fn let_go(&self, project: &mut Project, touches: &[Touch]) -> Result<(), ProjectError> {
        let mut result = Ok(());
        for touch in touches {
            let released =
                project.send::<LanePlayer>(&touch.lane, lane::PROCESSOR, LaneUpdate::Held(false));
            // A lane that went during the drag has no player to tell.
            if !matches!(released, Err(ProjectError::MissingProcessor { .. })) {
                result = result.and(released);
            }
        }
        result
    }
}

impl Touch {
    /// What the hand plays at the playhead, while the project plays.
    fn sample(&mut self, project: &Project, playhead: Playhead) {
        let went_on = self.playing && playhead.jumps == self.jumps;
        (self.jumps, self.playing) = (playhead.jumps, playhead.playing);
        if !playhead.playing {
            return;
        }
        let number = project.automation(&self.instance, &self.number);
        // A lane holds no -inf: a volume's bottom is -100 dB.
        let Some(value) = number
            .and_then(|number| number.record)
            .map(|value| value.max(-100.0))
        else {
            return;
        };
        if !went_on || self.runs.is_empty() {
            self.runs.push(Vec::new());
        }
        let Some(run) = self.runs.last_mut() else {
            return;
        };
        match run.last_mut() {
            Some((tick, last)) if *tick == playhead.tick => *last = value,
            Some((tick, _)) if *tick > playhead.tick => {}
            _ => run.push((playhead.tick, value)),
        }
    }

    /// Writes what the hand played into the lane.
    fn write(&self, project: &mut Project, gesture: &mut ProjectEdit) -> Result<(), ProjectError> {
        let runs: Vec<_> = self
            .runs
            .iter()
            .filter(|run| !run.is_empty())
            .cloned()
            .collect();
        let travel = project
            .automation(&self.instance, &self.number)
            .map(|number| number.range);
        if let (false, Some(travel)) = (runs.is_empty(), travel)
            && project.tool_of(&self.lane) == Some(lane::NAME)
        {
            let mut refused = None;
            project.update_json(gesture, &self.lane, |state| {
                match lane::written(state, &runs, travel) {
                    Ok(written) => *state = written,
                    Err(error) => refused = Some(error),
                }
            })?;
            if let Some(message) = refused {
                return Err(ProjectError::InvalidState {
                    id: self.lane.clone(),
                    message,
                });
            }
        }
        Ok(())
    }

    /// Puts the number back in the record as it was before the drag.
    fn restore(
        &self,
        project: &mut Project,
        gesture: &mut ProjectEdit,
    ) -> Result<(), ProjectError> {
        if project.tool_of(&self.instance).is_some() {
            let before = self.before.clone();
            project.update_json(gesture, &self.instance, |state| {
                set_path(state, &self.number, before);
            })?;
        }
        Ok(())
    }
}

/// The parts of the path of a number in a record: `bands[0].gain_db` is `bands`, `0` and
/// `gain_db`, `parameters.12.value` is `parameters`, `12` and `value`.
fn parts(path: &str) -> impl Iterator<Item = &str> {
    path.split(['.', '[', ']']).filter(|part| !part.is_empty())
}

fn child<'a>(value: &'a Value, part: &str) -> Option<&'a Value> {
    match value {
        Value::Array(items) => items.get(part.parse::<usize>().ok()?),
        _ => value.get(part),
    }
}

fn at_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    parts(path).try_fold(value, child)
}

/// Puts `to` at `path`, or takes the number out where `to` is `None`.
fn set_path(value: &mut Value, path: &str, to: Option<Value>) {
    let parts: Vec<&str> = parts(path).collect();
    let Some((last, objects)) = parts.split_last() else {
        return;
    };
    let mut at = value;
    for part in objects {
        let next = match at {
            Value::Array(items) => part
                .parse::<usize>()
                .ok()
                .and_then(|index| items.get_mut(index)),
            _ => at.get_mut(*part),
        };
        let Some(next) = next else {
            return;
        };
        at = next;
    }
    match (at, to) {
        (Value::Array(items), Some(to)) => {
            if let Some(item) = last
                .parse::<usize>()
                .ok()
                .and_then(|index| items.get_mut(index))
            {
                *item = to;
            }
        }
        (Value::Object(fields), Some(to)) => {
            fields.insert((*last).to_string(), to);
        }
        (Value::Object(fields), None) => {
            fields.remove(*last);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{at_path, set_path};

    #[test]
    fn a_number_is_found_and_put_back_by_its_path_in_the_record() {
        let mut record = json!({"bands": [{"gain_db": 1}, {"gain_db": 2}], "parameters": {"12": {"value": 0.5}}});
        assert_eq!(at_path(&record, "bands[1].gain_db"), Some(&json!(2)));
        assert_eq!(at_path(&record, "parameters.12.value"), Some(&json!(0.5)));
        set_path(&mut record, "bands[1].gain_db", Some(json!(3)));
        set_path(&mut record, "parameters.12.value", None);
        assert_eq!(
            record,
            json!({"bands": [{"gain_db": 1}, {"gain_db": 3}], "parameters": {"12": {}}})
        );
    }
}
