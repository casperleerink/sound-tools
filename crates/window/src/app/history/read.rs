//! What the History tab shows, read from the project's history once per change of it: the
//! card of the current point open on its latest changes, with where other lines start between
//! them, then the other versions, newest first.

use std::collections::{HashMap, HashSet};
use std::io;
use std::time::SystemTime;

use sound_core::{History, Line, Point, PointId};

/// How many changes the open card shows at first, and how many more each "Show more" adds.
pub(super) const SHOWN: usize = 6;
pub(super) const MORE: usize = 50;

pub(super) struct Shown {
    pub(super) open: Option<Open>,
    /// The other versions, newest first.
    pub(super) others: Vec<Card>,
    pub(super) folded_away: Vec<Card>,
}

pub(super) struct Card {
    pub(super) line: Line,
    /// Its colour: where its line is in the history's lines.
    pub(super) color: usize,
    /// Of its latest point.
    pub(super) time: SystemTime,
    pub(super) from: Option<From>,
}

/// The line a line started from.
pub(super) struct From {
    pub(super) start: PointId,
    pub(super) title: String,
    pub(super) color: usize,
}

/// The line of the current point.
pub(super) struct Open {
    pub(super) card: Card,
    /// Newest first.
    pub(super) entries: Vec<Entry>,
    /// Whether older changes are left to show.
    pub(super) more: bool,
}

pub(super) enum Entry {
    Change(Point),
    /// A version that starts after the change below it.
    Split {
        start: PointId,
        title: String,
        color: usize,
    },
    /// Work taken back with undo after the change below it.
    Undone {
        start: PointId,
        count: usize,
    },
}

/// Reads what shows, with `wanted` changes of the line that starts there.
pub(super) fn read(history: &History, wanted: Option<(PointId, usize)>) -> io::Result<Shown> {
    let lines = history.lines();
    let color = |start: PointId| {
        (lines.iter())
            .position(|line| line.start == start)
            .unwrap_or(0)
    };
    let splits = history.splits()?;
    let folded_away = history.folded_away()?;
    let card = |line: &Line| -> io::Result<Card> {
        let tip = history.line_points(line.start).next().transpose()?;
        let from = (splits.get(&line.start))
            .and_then(|point| history.line(point.line))
            .map(|from| From {
                start: from.start,
                title: from.title(),
                color: color(from.start),
            });
        Ok(Card {
            line: line.clone(),
            color: color(line.start),
            time: tip.map_or(SystemTime::UNIX_EPOCH, |tip| tip.time),
            from,
        })
    };

    let current = history.current();
    let open_line = history.line_of(current)?.cloned();
    let open = match &open_line {
        Some(line) => {
            let wanted = match wanted {
                Some((start, count)) if start == line.start => count,
                _ => SHOWN,
            };
            let mut changes = Vec::new();
            let (mut more, mut seen_current) = (false, false);
            for point in history.line_points(line.start) {
                let point = point?;
                if changes.len() >= wanted && seen_current {
                    more = true;
                    break;
                }
                seen_current |= point.id == current;
                changes.push(point);
            }
            // Lines that start from a change shown here, by that change.
            let mut started: HashMap<PointId, Vec<&Line>> = HashMap::new();
            let shown: HashSet<PointId> = changes.iter().map(|change| change.id).collect();
            for other in lines {
                if let Some(from) = splits.get(&other.start)
                    && shown.contains(&from.id)
                {
                    started.entry(from.id).or_default().push(other);
                }
            }
            let mut entries = Vec::new();
            for change in changes {
                for other in started.remove(&change.id).unwrap_or_default() {
                    entries.push(match other.is_version() {
                        true => Entry::Split {
                            start: other.start,
                            title: other.title(),
                            color: color(other.start),
                        },
                        false => Entry::Undone {
                            start: other.start,
                            count: history.line_points(other.start).count(),
                        },
                    });
                }
                entries.push(Entry::Change(change));
            }
            Some(Open {
                card: card(line)?,
                entries,
                more,
            })
        }
        None => None,
    };
    let open_start = open_line.map(|line| line.start);
    let mut others = Vec::new();
    let mut folded = Vec::new();
    for line in (lines.iter()).filter(|line| line.is_version() && Some(line.start) != open_start) {
        let card = card(line)?;
        match folded_away.contains(&line.start) {
            true => folded.push(card),
            false => others.push(card),
        }
    }
    others.sort_by_key(|card| std::cmp::Reverse(card.time));
    folded.sort_by_key(|card| std::cmp::Reverse(card.time));
    Ok(Shown {
        open,
        others,
        folded_away: folded,
    })
}

/// How long ago, in the words of a person.
pub(super) fn ago(time: SystemTime, now: SystemTime) -> String {
    let minutes = now.duration_since(time).unwrap_or_default().as_secs() / 60;
    let (hours, days) = (minutes / 60, minutes / 60 / 24);
    match () {
        _ if minutes < 1 => "Just now".into(),
        _ if hours < 1 => format!("{minutes} min ago"),
        _ if days < 1 => format!("{hours} h ago"),
        _ if days == 1 => "1 day ago".into(),
        _ => format!("{days} days ago"),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::ago;

    #[test]
    fn times_are_said_as_a_person_says_them() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10 * 24 * 3600);
        let before = |seconds: u64| now - Duration::from_secs(seconds);
        let said =
            [59, 60, 59 * 60, 3600, 23 * 3600, 24 * 3600, 48 * 3600].map(|s| ago(before(s), now));
        assert_eq!(
            said,
            [
                "Just now",
                "1 min ago",
                "59 min ago",
                "1 h ago",
                "23 h ago",
                "1 day ago",
                "2 days ago"
            ]
        );
        // A clock that went back says nothing strange.
        assert_eq!(ago(now + Duration::from_secs(90), now), "Just now");
    }
}
