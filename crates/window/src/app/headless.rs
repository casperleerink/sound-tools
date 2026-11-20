//! A run without a window: a copy of the project, the engine played offline, and the report
//! `--tree` adds.

use std::path::{Path, PathBuf};
use std::sync::PoisonError;

use anyhow::{Context as _, Result};
use serde_json::{Value, json};
use sound_typescript::next::Tools;

use crate::controls;

use super::{REPORT, Sound, State};

impl State {
    /// Input was handled. A run without a window answers it at once, so the frame it came in
    /// draws it and the report says it: Rune runs input after `poll`, and the last frame has none after.
    pub(super) fn handled(&mut self) {
        if self.headless() {
            self.settle();
            *REPORT.lock().unwrap_or_else(PoisonError::into_inner) = Some(self.report());
        }
    }

    /// Plays the engine offline up to `seconds`, in blocks as a device plays them.
    pub(super) fn play_offline(&mut self, seconds: f64) -> Result<()> {
        let Sound::Offline { engine, played } = &mut self.sound else {
            return Ok(());
        };
        let rate = f64::from(engine.sample_rate());
        let channels = engine.channels();
        let mut frames = ((seconds - *played) * rate).round().max(0.0) as usize;
        *played += frames as f64 / rate;
        let mut buffer = vec![0.0_f32; 512 * channels];
        while frames > 0 {
            let size = frames.min(512);
            let rendered = self.session.background(|project| {
                runtime::render_block(
                    project,
                    engine,
                    &self.plugins,
                    &mut buffer[..size * channels],
                )
            })?;
            for problem in rendered {
                self.session.report(problem);
            }
            frames -= size;
        }
        Ok(())
    }

    /// The first frame of a run without a window: the steps of `--act`, then `--play`.
    pub(super) fn first_frame(&mut self) -> Result<()> {
        let Some((steps, play)) = self.first.take() else {
            return Ok(());
        };
        self.bind_lanes()?;
        for step in steps {
            step.run(self)?;
            self.bind_lanes()?;
        }
        if play.is_some() {
            self.session.background(|project| project.engine().play());
        }
        Ok(())
    }

    /// Lanes bind once the tools' behaviours answered and the engine played what they bound:
    /// in a run without a window, a step and the picture see what the step before it left. A
    /// block of 2 ms, not heard and not counted, so `--play` plays all it says.
    fn bind_lanes(&mut self) -> Result<()> {
        let Sound::Offline { played, .. } = &self.sound else {
            return Ok(());
        };
        let before = *played;
        self.play_offline(before + 0.002)?;
        if let Sound::Offline { played, .. } = &mut self.sound {
            *played = before;
        }
        for _ in 0..2 {
            self.poll_session();
            self.settle();
        }
        Ok(())
    }

    /// What `--tree` adds for an agent: the controls, where the piece plays and the feeds the views read.
    pub(super) fn report(&self) -> Value {
        let controls = controls::collect(self).controls;
        let problems: Vec<String> = self
            .project()
            .problems()
            .iter()
            .map(|problem| format!("{}: {}", problem.path, problem.message))
            .collect();
        let selected = self.session.selected();
        let span = self.session.selected_span();
        let transport = self.session.transport();
        let asked = self.waiting.asked.front().map(|asked| {
            json!({ "message": asked.message, "detail": asked.detail, "confirm": asked.confirm })
        });
        json!({
            "controls": controls,
            "actions": self.shown_actions(),
            "dialog": asked,
            "selected": selected.map(|id| json!({ "instance": id, "span": span.map(|(from, to)| [from.0, to.0]) })),
            "repeat": transport.looped.on.then(|| [transport.looped.range.start.0, transport.looped.range.end.0]),
            "click": transport.click,
            "computer_keys": transport.keys,
            "write_automation": transport.automation,
            "playing": self.playing,
            "playhead": self.playhead.0,
            "time": self.time(),
            "feeds": self.feeds.numbers().into_iter().map(|(name, number)| (name, json!(number))).collect::<serde_json::Map<_, _>>(),
            "view_runs": self.tools.as_ref().map_or(0, Tools::view_runs),
            "problems": problems,
            "notice": self.session.notice(),
            "plugin_windows": self.plugin_windows.titles(),
        })
    }

    fn time(&self) -> String {
        let at = self
            .project()
            .clock()
            .tempo_map()
            .time_signatures()
            .bar_beat_of(self.playhead);
        format!("{}.{}", at.bar, at.beat)
    }
}

/// A copy of the project in `folder`, for a run without a window: opening writes into a folder.
/// The scratch folder, and the copy in it, called `name`.
pub(super) fn copied(folder: &Path, name: &str) -> Result<(tempfile::TempDir, PathBuf)> {
    let scratch = tempfile::tempdir()?;
    let copy = scratch.path().join(name);
    if folder.join(runtime::PROJECT_FILE).is_file() {
        copy_folder(folder, &copy)?;
    }
    Ok((scratch, copy))
}

/// Copies `from` to `to`, without hidden files, such as `.git` and the lock, or links.
pub(super) fn copy_folder(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in
        std::fs::read_dir(from).with_context(|| format!("could not read {}", from.display()))?
    {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with('.')
            || entry.file_name() == "node_modules"
        {
            continue;
        }
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_folder(&source, &target)?;
        } else if kind.is_file() {
            std::fs::copy(&source, &target)
                .with_context(|| format!("could not copy {}", source.display()))?;
        }
    }
    Ok(())
}
