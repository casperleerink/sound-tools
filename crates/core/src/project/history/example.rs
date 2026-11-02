//! A long made-up history, to judge the History tab on: `HISTORY_EXAMPLE=<a project folder>
//! cargo test -p sound-core write_a_long_history -- --ignored`. The folder needs a
//! `project.json`; each point changes its tempo.

#![allow(clippy::unwrap_used)]

use super::{Going, History, Origin};
use std::path::Path;

const MINUTE: u64 = 60_000;

struct Maker {
    history: History,
    clock: u64,
    step: u64,
}

impl Maker {
    fn edit(&mut self, minutes: u64) {
        self.clock += minutes * MINUTE / 2 + (self.step * 7919 % 5) * MINUTE / 2;
        self.step += 1;
        let path = self.history.root.join("project.json");
        let text = std::fs::read_to_string(&path).unwrap();
        let bpm = 70 + self.step % 90;
        let start = text.find("\"bpm\":").unwrap();
        let end = start + text[start..].find(['\n', '}']).unwrap();
        let text = format!("{}\"bpm\": {bpm}.0{}", &text[..start], &text[end..]);
        std::fs::write(&path, text).unwrap();
        super::CLOCK.set(Some(self.clock));
        self.history.save(Origin::Edit, "Edit").unwrap();
    }

    fn pause(&mut self, hours: u64) {
        self.clock += hours * 60 * MINUTE;
    }

    fn edits(&mut self, count: usize, minutes: u64) {
        for _ in 0..count {
            self.edit(minutes);
        }
    }

    fn undo(&mut self, count: usize) {
        for _ in 0..count {
            let target = self.history.undo_target().unwrap().unwrap();
            self.history.go(target, Going::Undo).unwrap();
        }
    }

    fn back(&mut self, points: usize) {
        let target = self.history.points(points + 1).unwrap()[points].id;
        self.history.go(target, Going::Back).unwrap();
    }

    fn name_current(&mut self, name: &str) {
        let line = self
            .history
            .line_of(self.history.head)
            .unwrap()
            .unwrap()
            .clone();
        self.history
            .name(line.start, name.into(), line.tip)
            .unwrap();
    }
}

#[test]
#[ignore = "writes an example project, see the module"]
fn write_a_long_history() {
    let folder = std::env::var("HISTORY_EXAMPLE").unwrap();
    let root = Path::new(&folder);
    let start = super::now_millis() - 50 * 60 * MINUTE;
    super::CLOCK.set(Some(start));
    let mut maker = Maker {
        history: History::open(root, Vec::new()).unwrap(),
        clock: start,
        step: 0,
    };
    maker.name_current("Slow piano sketch");
    maker.edits(90, 3);
    maker.pause(14);
    maker.undo(3);
    maker.edits(40, 2);
    maker.undo(2);
    maker.edits(30, 2);
    // An idea tried an hour back, then left.
    maker.back(45);
    maker.edits(25, 2);
    maker.name_current("Octave lead, open filter");
    maker.back(26);
    maker.pause(13);
    maker.edits(60, 2);
    maker.undo(4);
    maker.edits(20, 2);
    // A version that is merged back.
    maker.back(10);
    maker.edits(12, 2);
    maker.name_current("Extra notes");
    let merged = maker
        .history
        .line_of(maker.history.head)
        .unwrap()
        .unwrap()
        .start;
    maker.back(13);
    maker.edits(30, 2);
    super::CLOCK.set(Some(maker.clock));
    maker.history.merge(merged, false).unwrap();
    // Twelve points undone are a version of their own.
    maker.edits(15, 2);
    maker.undo(12);
    maker.edits(25, 2);
    maker.name_current("Moved track 1 to start");
    super::CLOCK.set(None);
    assert!(maker.clock < super::now_millis());
    assert!(maker.history.points(1000).unwrap().len() > 300);
}
