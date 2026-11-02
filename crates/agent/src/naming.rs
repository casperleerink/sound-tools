//! Names for the versions of the project history, found by the agent's small model from what
//! each version changed. A version named by hand keeps its name.

use std::collections::HashSet;
use std::io;
use std::time::Duration;

use sound_core::{FileChange, History, Line, PointId};

/// A version is named once the history has been still this long, and again after it grew.
pub(crate) const QUIET: Duration = Duration::from_secs(5);

/// What the model reads of what changed, at most, and of one file, so a version that changed
/// many files still shows several.
const DESCRIPTION_LIMIT: usize = 6000;
const FILE_LIMIT: usize = 1500;
/// How many of the newest points are read for the order of versions and their steps.
const POINTS_READ: usize = 200;
const STEPS_SHOWN: usize = 20;
const NAME_LIMIT: usize = 60;

pub(crate) const INSTRUCTIONS: &str = "You name versions of a piece of music for its \
composer. A version is a run of changes to their project in a music app; you get the steps of \
the version and what changed in the project's files. Answer with only the name: 2 to 5 words \
about the music, in a musician's words, such as \"Octave lead, open filter\" or \"Half-time \
drums\". No quotes, no file names, no code or programming words, no full stop.";

/// One version to name, at its tip.
#[derive(Debug)]
pub(crate) struct Naming {
    pub start: PointId,
    pub tip: PointId,
    /// What the model reads.
    pub description: String,
}

/// The version to name next: the current one first, then the newest. Only versions that show
/// and were not named at their tip yet, by hand never. `failed` are versions, by start and tip,
/// whose naming failed: they wait until they grow.
pub(crate) fn next(
    history: &History,
    failed: &HashSet<(PointId, PointId)>,
) -> io::Result<Option<Naming>> {
    let points = history.points(POINTS_READ)?;
    let current = history.line_of(history.current())?.map(|line| line.start);
    let age = |line: &Line| {
        let tip = points.iter().position(|point| point.id == line.tip);
        tip.unwrap_or(points.len())
    };
    let mut waiting: Vec<&Line> = (history.lines().iter())
        .filter(|line| {
            line.is_version()
                && !line.named_by_hand
                && line.named_at != Some(line.tip)
                && !failed.contains(&(line.start, line.tip))
        })
        .collect();
    waiting.sort_by_key(|line| (Some(line.start) != current, age(line)));
    for line in waiting {
        if history.folded_away(line)? {
            continue;
        }
        let base = history.parents(line.start)?.first().copied();
        let changes = history.changes(base, line.tip)?;
        let mut steps: Vec<&str> = (points.iter())
            .filter(|point| point.line == line.start)
            .map(|point| point.label.as_str())
            .take(STEPS_SHOWN)
            .collect();
        steps.reverse();
        steps.dedup();
        return Ok(Some(Naming {
            start: line.start,
            tip: line.tip,
            description: describe(&steps, &changes),
        }));
    }
    Ok(None)
}

fn describe(steps: &[&str], changes: &[FileChange]) -> String {
    let mut text = String::from("Steps of this version, oldest first:\n");
    for step in steps {
        text.push_str("- ");
        text.push_str(step);
        text.push('\n');
    }
    text.push_str("\nWhat changed:\n");
    for change in changes {
        if text.len() >= DESCRIPTION_LIMIT {
            break;
        }
        let mut file = file_change(change);
        cut(&mut file, FILE_LIMIT);
        text.push_str(&file);
    }
    cut(&mut text, DESCRIPTION_LIMIT);
    text
}

/// The lines that differ, between the lines both sides begin and end with.
fn file_change(change: &FileChange) -> String {
    let kind = match (&change.before, &change.after) {
        (None, _) => "new",
        (_, None) => "removed",
        _ => "changed",
    };
    let text = |bytes: &Option<Vec<u8>>| {
        let bytes = bytes.as_deref().unwrap_or_default();
        String::from_utf8_lossy(bytes).into_owned()
    };
    let (before, after) = (text(&change.before), text(&change.after));
    let (before, after): (Vec<&str>, Vec<&str>) =
        (before.lines().collect(), after.lines().collect());
    let same_start = (before.iter().zip(&after))
        .take_while(|(old, new)| old == new)
        .count();
    let same_end = (before.iter().skip(same_start).rev())
        .zip(after.iter().skip(same_start).rev())
        .take_while(|(old, new)| old == new)
        .count();
    let mut file = format!("\n{} ({kind})\n", change.path);
    for (sign, lines) in [("- ", &before), ("+ ", &after)] {
        let differ = lines.len().saturating_sub(same_start + same_end);
        for line in lines.iter().skip(same_start).take(differ) {
            file.push_str(sign);
            file.push_str(line.trim());
            file.push('\n');
        }
    }
    file
}

fn cut(text: &mut String, limit: usize) {
    if text.len() <= limit {
        return;
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str("…\n");
}

/// The name in the model's answer: its first line, without quotes or a full stop.
pub(crate) fn name_in(answer: &str) -> Option<String> {
    let line = answer
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let quotes = |letter: char| matches!(letter, '"' | '\'' | '“' | '”' | '‘' | '’' | '*' | '`');
    let name = line.trim_matches(quotes).trim_end_matches('.').trim();
    (!name.is_empty()).then(|| name.chars().take(NAME_LIMIT).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_file_shows_only_the_lines_that_differ() {
        let change = FileChange {
            path: "state/lead.json".to_string(),
            before: Some(b"{\n  \"octave\": 0,\n  \"cutoff\": 800\n}".to_vec()),
            after: Some(b"{\n  \"octave\": 1,\n  \"cutoff\": 800\n}".to_vec()),
        };
        assert_eq!(
            file_change(&change),
            "\nstate/lead.json (changed)\n- \"octave\": 0,\n+ \"octave\": 1,\n"
        );
    }

    #[test]
    fn many_changes_stay_within_the_limit_and_show_several_files() {
        let big = |index: usize| FileChange {
            path: format!("state/clip-{index}.json"),
            before: None,
            after: Some("é".repeat(5000).into_bytes()),
        };
        let changes: Vec<FileChange> = (0..20).map(big).collect();
        let text = describe(&["Add clips"], &changes);
        assert!(text.len() <= DESCRIPTION_LIMIT + "…\n".len());
        assert!(text.contains("state/clip-2.json"));
    }

    #[test]
    fn the_name_is_the_first_line_without_quotes() {
        assert_eq!(
            name_in("\n“Octave lead, open filter.”\nmore").as_deref(),
            Some("Octave lead, open filter")
        );
        assert_eq!(name_in("  \n"), None);
    }
}
