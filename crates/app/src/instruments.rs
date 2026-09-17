use crate::{
    editing::{self, Edit},
    workspace::{Workspace, poll_pending},
};
use gpui::{Context, Entity, div, prelude::*};
use sound_core::{Error, Result};
use sound_daw::{Arrangement, Track, sample_asset::import_sample, sampler::SamplerConfig};
use sound_runtime::session::{Command, Snapshot};
use sound_ui::components::{
    button::{Button, ButtonSize},
    text_input::TextInput,
};
use std::{path::Path, sync::mpsc::Receiver};

#[cfg(test)]
#[path = "instruments_tests.rs"]
mod tests;

pub(crate) struct Instruments {
    sample_path: Entity<TextInput>,
    importing: bool,
    import_receipt: Option<Receiver<Result<()>>>,
    import_status: String,
    target: usize,
    port: usize,
}

impl Instruments {
    pub(crate) fn new(cx: &mut Context<Workspace>) -> Self {
        Self {
            sample_path: cx.new(|cx| TextInput::new(cx).placeholder("Path to WAV file")),
            importing: false,
            import_receipt: None,
            import_status: String::new(),
            target: 0,
            port: 0,
        }
    }

    pub(crate) fn awaiting_import(&self) -> bool {
        self.import_receipt.is_some()
    }

    fn import_busy(&self) -> bool {
        self.importing || self.awaiting_import()
    }
}

fn prepare_import(
    root: &Path,
    source: &Path,
    mut arrangement: Arrangement,
    revision: u64,
) -> Result<Command> {
    editing::validate(&arrangement)?;
    let asset = import_sample(root, source)?;
    let name = (1..=arrangement.tracks.len().saturating_add(1))
        .map(|number| format!("Sampler {number}"))
        .find(|name| arrangement.tracks.iter().all(|track| &track.name != name))
        .ok_or_else(|| Error("No available sampler track name".into()))?;
    arrangement.tracks.push(Track {
        name,
        instrument: "daw.sampler".into(),
        sampler: Some(SamplerConfig {
            asset,
            root_key: 60,
        }),
        ..Track::default()
    });
    editing::validate(&arrangement)?;
    Ok(Command::SetArrangement {
        arrangement,
        expected_revision: revision,
    })
}

fn reconcile_target(displayed: &Arrangement, live: &Arrangement, index: usize) -> usize {
    displayed
        .tracks
        .get(index)
        .and_then(|selected| {
            live.tracks
                .iter()
                .position(|track| track.name == selected.name)
        })
        .unwrap_or(0)
}

fn cycle(index: usize, count: usize) -> usize {
    if count == 0 || index >= count - 1 {
        0
    } else {
        index + 1
    }
}

impl Workspace {
    pub(crate) fn refresh_instruments(&mut self, snap: &Snapshot) {
        self.instruments.target = reconcile_target(
            &self.snap.arrangement,
            &snap.arrangement,
            self.instruments.target,
        );
        if self.snap.midi_ports != snap.midi_ports {
            self.instruments.port = 0;
        }
        if let Some(result) = poll_pending(&mut self.instruments.import_receipt) {
            self.instruments.import_status = match result {
                Ok(()) => "Sampler track imported".into(),
                Err(error) => format!("Import not applied: {error}"),
            };
        }
    }

    fn import_wav(&mut self, cx: &mut Context<Self>) {
        if self.instruments.import_busy() || !self.current(cx) {
            return;
        }
        let source = self.instruments.sample_path.read(cx).text().to_owned();
        if source.is_empty() {
            self.instruments.import_status = "Enter a WAV file path".into();
            cx.notify();
            return;
        }
        let root = self.project_root.clone();
        let arrangement = self.snap.arrangement.clone();
        let revision = self.snap.revision;
        self.instruments.importing = true;
        self.instruments.import_status = "Validating and copying WAV...".into();
        let task = cx
            .background_executor()
            .spawn(async move { prepare_import(&root, Path::new(&source), arrangement, revision) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.instruments.importing = false;
                match result.and_then(|command| this.session.send(command)) {
                    Ok(receipt) => {
                        this.instruments.import_receipt = Some(receipt);
                        this.instruments.import_status =
                            "Waiting for import confirmation...".into();
                    }
                    Err(error) => {
                        this.instruments.import_status = format!("Import failed: {error}")
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn connect_midi(&mut self, cx: &mut Context<Self>) {
        let ports = self.snap.midi_ports.clone();
        if !self.current(cx) {
            return;
        }
        if ports != self.snap.midi_ports {
            self.status = "MIDI ports changed. Review the selected port and retry.".into();
            cx.notify();
            return;
        }
        if let Some(track) = self.snap.arrangement.tracks.get(self.instruments.target)
            && self.snap.midi_ports.get(self.instruments.port).is_some()
        {
            self.queue(
                Command::ConnectMidi {
                    port: self.instruments.port,
                    track: track.name.clone(),
                },
                cx,
            );
        }
    }

    pub(crate) fn instrument_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let target = self.instruments.target;
        let track = self.snap.arrangement.tracks.get(target);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child("Sampler")
            .child(self.instruments.sample_path.clone())
            .child(
                Button::new("import-wav", "Import WAV")
                    .size(ButtonSize::Sm)
                    .disabled(self.busy() || self.instruments.import_busy())
                    .on_click(cx.listener(|this, _, _, cx| this.import_wav(cx))),
            )
            .child(
                div()
                    .text_xs()
                    .child(self.instruments.import_status.clone()),
            )
            .child("MIDI / effects target")
            .child(div().min_w_0().child(track.map_or_else(
                || "No tracks".into(),
                |track| format!("{}: {}", target + 1, track.name),
            )))
            .child(
                Button::new("next-target", "Next track")
                    .size(ButtonSize::Xs)
                    .disabled(self.snap.arrangement.tracks.len() < 2)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.instruments.target =
                            cycle(this.instruments.target, this.snap.arrangement.tracks.len());
                        cx.notify();
                    })),
            )
            .when_some(track, |column, track| {
                column.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(self.edit_button(
                            "filter",
                            if track.fx.filter.is_some() {
                                "Filter on"
                            } else {
                                "Filter off"
                            },
                            Edit::Filter(target),
                            cx,
                        ))
                        .child(self.edit_button(
                            "delay",
                            if track.fx.delay.is_some() {
                                "Delay on"
                            } else {
                                "Delay off"
                            },
                            Edit::Delay(target),
                            cx,
                        ))
                        .child(self.edit_button(
                            "reverb",
                            if track.fx.reverb.is_some() {
                                "Reverb on"
                            } else {
                                "Reverb off"
                            },
                            Edit::Reverb(target),
                            cx,
                        )),
                )
            })
            .child("MIDI input")
            .child(
                Button::new("refresh-midi", "Refresh MIDI")
                    .size(ButtonSize::Xs)
                    .disabled(self.busy())
                    .on_click(
                        cx.listener(|this, _, _, cx| this.queue(Command::RefreshMidiPorts, cx)),
                    ),
            )
            .child(self.snap.midi_ports.get(self.instruments.port).map_or_else(
                || "No MIDI ports".into(),
                |port| {
                    format!(
                        "{} / {}: {port}",
                        self.instruments.port + 1,
                        self.snap.midi_ports.len()
                    )
                },
            ))
            .child(
                Button::new("next-port", "Next port")
                    .size(ButtonSize::Xs)
                    .disabled(self.snap.midi_ports.len() < 2 || self.busy())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.instruments.port =
                            cycle(this.instruments.port, this.snap.midi_ports.len());
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("connect-midi", "Connect")
                            .size(ButtonSize::Xs)
                            .disabled(
                                self.busy() || track.is_none() || self.snap.midi_ports.is_empty(),
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.connect_midi(cx))),
                    )
                    .child(
                        Button::new("disconnect-midi", "Disconnect")
                            .size(ButtonSize::Xs)
                            .disabled(self.busy() || self.snap.midi_input.is_none())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.queue(Command::DisconnectMidi, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .child(self.snap.midi_input.as_ref().map_or_else(
                        || "MIDI disconnected".into(),
                        |input| format!("Connected: {input}"),
                    )),
            )
    }
}
