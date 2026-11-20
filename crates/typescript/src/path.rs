//! The path of a number as a view names it: the instance it is in, a dot, and where its record
//! holds it, as a lane names it: `cutoff_hz`, `bass/synth.cutoff_hz`, `kit.pads.36.volume_db`,
//! `eq.bands[0].gain_db`, or `voices[2].detune` in the record of the view's own tool.

use serde_json::{Map, Value};

/// The parts of the path of a number in a record: `bands[0].gain_db` is `bands`, `0` and
/// `gain_db`, `parameters.12.value` is `parameters`, `12` and `value`.
pub(crate) fn parts(path: &str) -> impl Iterator<Item = &str> {
    path.split(['.', '[', ']']).filter(|part| !part.is_empty())
}

fn child<'a>(value: &'a Value, part: &str) -> Option<&'a Value> {
    match value {
        Value::Array(items) => items.get(part.parse::<usize>().ok()?),
        _ => value.get(part),
    }
}

pub(crate) fn at<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    parts(path).try_fold(value, child)
}

/// A number a record holds, `"-inf"` for silence too, which JSON has no number for.
pub(crate) fn number_of(value: &Value) -> Option<f32> {
    match value.as_str() {
        Some("-inf") => Some(f32::NEG_INFINITY),
        _ => value.as_f64().map(|value| value as f32),
    }
}

/// Puts `to` at `path`, or takes the number out where `to` is `None`. An object on the way that
/// the record leaves out is made, such as a pad of a kit at its defaults; an item of a list is not.
pub fn set(value: &mut Value, path: &str, to: Option<Value>) {
    let parts: Vec<&str> = parts(path).collect();
    let Some((last, objects)) = parts.split_last() else {
        return;
    };
    let mut at = value;
    for part in objects {
        let makes = to.is_some();
        let next = match at {
            Value::Array(items) => part
                .parse::<usize>()
                .ok()
                .and_then(|index| items.get_mut(index)),
            Value::Object(fields) => {
                if makes && !fields.contains_key(*part) {
                    fields.insert((*part).to_string(), Value::Object(Map::new()));
                }
                fields.get_mut(*part)
            }
            _ => None,
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

    use super::{at, set};

    #[test]
    fn a_number_is_found_and_put_back_by_its_path_in_the_record() {
        let mut record = json!({"bands": [{"gain_db": 1}, {"gain_db": 2}], "parameters": {"12": {"value": 0.5}}});
        assert_eq!(at(&record, "bands[1].gain_db"), Some(&json!(2)));
        assert_eq!(at(&record, "parameters.12.value"), Some(&json!(0.5)));
        set(&mut record, "bands[1].gain_db", Some(json!(3)));
        set(&mut record, "parameters.12.value", None);
        assert_eq!(
            record,
            json!({"bands": [{"gain_db": 1}, {"gain_db": 3}], "parameters": {"12": {}}})
        );
    }

    #[test]
    fn a_number_of_an_object_the_record_leaves_out_makes_the_object_and_no_item_of_a_list() {
        let mut record = json!({"pads": {}, "bands": []});
        set(&mut record, "pads.36.volume_db", Some(json!(-6)));
        set(&mut record, "bands[2].gain_db", Some(json!(3)));
        set(&mut record, "pads.40.pan", None);
        assert_eq!(
            record,
            json!({"pads": {"36": {"volume_db": -6}}, "bands": []})
        );
    }
}
