//! Renders in a process of their own, through the session: an export of the piece, and a part
//! of the song that code asked for, which comes back as a sound file of the project.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use runtime::session::{Polled, Rendered};
use sound_core::{InstanceId, Ticks};
use sound_typescript::window::Answer;

use super::State;

/// What a render is for: where its file goes once it ends.
pub(super) enum Purpose {
    /// The piece, into the file the composer named.
    Export(PathBuf),
    /// A part code asked for, into a scratch folder, then into `assets/audio/`.
    Part {
        question: u64,
        name: String,
        file: PathBuf,
        scratch: tempfile::TempDir,
    },
}

pub(crate) struct Renders {
    /// The program that renders: this one, whose `--render` reads the project from its folder.
    pub(super) program: PathBuf,
    running: Option<Purpose>,
}

impl Renders {
    pub(super) fn new() -> Result<Self> {
        Ok(Self {
            program: std::env::current_exe()?,
            running: None,
        })
    }
}

impl State {
    /// Renders the project into `file`, with `arguments` after it, while the notice says
    /// `notice`. Fails while another render runs.
    pub(super) fn start_render(
        &mut self,
        notice: String,
        file: &Path,
        arguments: &[String],
        purpose: Purpose,
    ) -> Result<()> {
        (self.session).render(&self.renders.program, notice, file.to_path_buf(), arguments)?;
        self.renders.running = Some(purpose);
        Ok(())
    }

    /// Polls the session, and sends a render that ended where it was for. A run without a
    /// window waits for a render to end, as it waits for Bun.
    pub(super) fn poll_renders(&mut self) -> Polled {
        let mut polled = self.session.poll();
        while self.headless() && polled.rendered.is_none() && self.session.rendering().is_some() {
            std::thread::sleep(Duration::from_millis(5));
            let more = self.session.poll();
            polled.events.extend(more.events);
            polled.redraw |= more.redraw;
            polled.rendered = more.rendered;
        }
        if let Some(rendered) = polled.rendered.take() {
            match self.renders.running.take() {
                Some(Purpose::Export(file)) => self.exported(file, rendered),
                Some(Purpose::Part {
                    question,
                    name,
                    file,
                    scratch,
                }) => self.part_rendered(question, &name, file, scratch, rendered),
                None => {}
            }
            polled.redraw = true;
        }
        polled
    }

    /// Renders `span` of `instance`, after its effects, into a sound file of the project, and
    /// answers `question` with its name; with `tail`, what rings on after the span too.
    pub(super) fn render_part(
        &mut self,
        question: u64,
        instance: &InstanceId,
        span: Range<Ticks>,
        tail: bool,
        name: String,
    ) {
        let started = tempfile::tempdir().map_err(Into::into).and_then(|scratch| {
            // The file's name becomes the sound's: the import makes it one the project takes.
            let stem: String = (name.chars())
                .map(|letter| match letter.is_alphanumeric() {
                    true => letter,
                    false => '-',
                })
                .collect();
            let file = scratch.path().join(format!("{stem}.wav"));
            let mut arguments = vec![
                "--from".to_string(),
                span.start.0.to_string(),
                "--to".to_string(),
                span.end.0.to_string(),
                "--only".to_string(),
                instance.to_string(),
            ];
            if !tail {
                arguments.push("--no-tail".to_string());
            }
            let purpose = Purpose::Part {
                question,
                name: name.clone(),
                file: file.clone(),
                scratch,
            };
            self.start_render(format!("Rendering {name}"), &file, &arguments, purpose)
        });
        if let Err(error) = started {
            (self.session).report(format!("The render of {name} failed: {error:#}"));
            self.answer(question, &Answer::Asset(None));
        }
    }

    fn part_rendered(
        &mut self,
        question: u64,
        name: &str,
        file: PathBuf,
        scratch: tempfile::TempDir,
        rendered: Result<Rendered>,
    ) {
        match rendered {
            Ok(rendered) => {
                // A plugin that did not load or answer in the render is missing from the file.
                if !rendered.problems.is_empty() {
                    self.session.report(format!(
                        "The render of {name} may be incomplete: {}",
                        rendered.problems.join("; ")
                    ));
                }
                self.import(question, file, Some(scratch));
            }
            Err(error) => {
                (self.session).report(format!("The render of {name} failed: {error:#}"));
                self.answer(question, &Answer::Asset(None));
            }
        }
    }
}
