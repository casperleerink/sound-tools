use crate::{
    Arrangement, BEAT, FRAME_RATE,
    dsp::{Delay, Filter, Gain, Reverb, Synth},
    sample_asset::SampleAsset,
    sampler::Sampler,
};
use serde::{Deserialize, Serialize};
use sound_core::{
    Error, Result,
    audio::{Event, EventData, Graph, GraphBuilder, NodeEvent, NodeId, Prepare, ProcessContext},
    clock::Transport,
    project::{Project, Record},
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MixerState {
    pub master_gain: f32,
}

impl Default for MixerState {
    fn default() -> Self {
        Self { master_gain: 1.0 }
    }
}

pub const BLOCK_FRAMES: usize = 128;

struct TrackNodes {
    instrument: NodeId,
    gain: NodeId,
}

pub struct Engine {
    graph: Graph,
    tracks: BTreeMap<String, TrackNodes>,
    sample_rate: f64,
    transport_revision: u64,
    playing: bool,
    reconstruct_notes: bool,
    live: Vec<NodeEvent>,
    arrangement: Arrangement,
    master: NodeId,
    pending: Vec<NodeEvent>,
}

pub const ARRANGEMENT_TOOL: &str = "daw.arrangement";
pub const MIXER_TOOL: &str = "daw.mixer";

impl Engine {
    pub fn build(project: &Project) -> Result<Self> {
        let arrangement: Arrangement = project
            .records()
            .get("arrangement")
            .ok_or_else(|| Error("Missing arrangement record".into()))
            .and_then(|record| {
                if record.tool != ARRANGEMENT_TOOL {
                    return Err(Error("Arrangement has the wrong tool".into()));
                }
                serde_json::from_value(record.state.clone())
                    .map_err(|error| Error(error.to_string()))
            })?;
        crate::arrangement::validate_arrangement(&arrangement)?;
        let mixer: MixerState = project
            .records()
            .get("mixer")
            .map(|record| {
                if record.tool != MIXER_TOOL {
                    return Err(Error("Mixer has the wrong tool".into()));
                }
                serde_json::from_value(record.state.clone())
                    .map_err(|error| Error(error.to_string()))
            })
            .transpose()?
            .unwrap_or_default();
        let settings = Prepare {
            sample_rate: FRAME_RATE,
            max_frames: BLOCK_FRAMES,
        };
        let mut builder = GraphBuilder::new(settings)?;
        let mut tracks = BTreeMap::new();
        for track in &arrangement.tracks {
            let instrument = match track.instrument.as_str() {
                "daw.synth" => builder.add(Synth::new()),
                "daw.sampler" => {
                    let config = track.sampler.as_ref().ok_or_else(|| {
                        Error("Sampler track needs a sample configuration".into())
                    })?;
                    let sample = SampleAsset::load(project.root(), &config.asset)?;
                    builder.add(Sampler::new(sample, config.root_key)?)
                }
                instrument => return Err(Error(format!("Unsupported instrument: {instrument}"))),
            };
            let mut source = instrument;
            let mut chains: Vec<Box<dyn sound_core::audio::Processor>> = Vec::new();
            if let Some(filter) = &track.fx.filter {
                chains.push(Box::new(Filter::new(filter.cutoff)));
            }
            if let Some(delay) = &track.fx.delay {
                chains.push(Box::new(Delay::new(
                    delay.seconds,
                    delay.feedback,
                    delay.mix,
                )));
            }
            if let Some(reverb) = &track.fx.reverb {
                chains.push(Box::new(Reverb::new(reverb.mix)));
            }
            for chain in chains {
                let node = builder.add_boxed(chain);
                builder.connect_stereo(source, node)?;
                source = node;
            }
            let solo = arrangement.tracks.iter().any(|track| track.soloed);
            let level = if track.muted || (solo && !track.soloed) {
                0.0
            } else {
                track.gain
            };
            let gain = builder.add(Gain::new(level, track.pan));
            builder.connect_stereo(source, gain)?;
            tracks.insert(track.name.clone(), TrackNodes { instrument, gain });
        }
        let master = builder.add(Gain::new(mixer.master_gain, 0.0));
        for nodes in tracks.values() {
            builder.connect_stereo(nodes.gain, master)?;
        }
        let device = builder.add(Gain::new(1.0, 0.0));
        builder.connect_stereo(master, device)?;
        let graph = builder.build(device)?;
        Ok(Self {
            graph,
            tracks,
            sample_rate: FRAME_RATE,
            transport_revision: 0,
            playing: false,
            reconstruct_notes: true,
            live: Vec::with_capacity(256),
            arrangement,
            master,
            pending: Vec::new(),
        })
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    pub fn queue_note(&mut self, track: &str, key: u8, velocity: u8, on: bool) {
        if key > 127 || velocity > 127 {
            return;
        }
        let on = on && velocity != 0;
        let solo = self.arrangement.tracks.iter().any(|track| track.soloed);
        if on
            && !self.arrangement.tracks.iter().any(|candidate| {
                candidate.name == track && !candidate.muted && (!solo || candidate.soloed)
            })
        {
            return;
        }
        let Some(nodes) = self.tracks.get(track) else {
            return;
        };
        let node = nodes.instrument;
        self.live.retain(|event| {
            event.node != node
                || !matches!(event.event.data, EventData::Custom { data, .. } if data[0] == f32::from(key))
        });
        if self.live.len() == 256 {
            self.all_notes_off();
            return;
        }
        self.live.push(NodeEvent {
            node,
            event: Event {
                frame: 0,
                data: EventData::Custom {
                    kind: if on { 1 } else { 2 },
                    data: [f32::from(key), f32::from(velocity) / 127.0, 0.0, 0.0],
                },
            },
        });
    }

    pub fn all_notes_off(&mut self) {
        self.live.clear();
        self.graph.reset();
    }

    pub fn apply_project(&mut self, project: &Project) -> Result<bool> {
        let (arrangement, mixer) = state_from_records(project.records())?;
        crate::arrangement::validate_arrangement(&arrangement)?;
        let same_topology = self.arrangement.tracks.len() == arrangement.tracks.len()
            && self
                .arrangement
                .tracks
                .iter()
                .zip(&arrangement.tracks)
                .all(|(old, next)| {
                    old.name == next.name
                        && old.instrument == next.instrument
                        && old.sampler == next.sampler
                        && old.fx == next.fx
                });
        if !same_topology {
            let mut engine = Self::build(project)?;
            engine.transport_revision = self.transport_revision;
            engine.playing = self.playing;
            *self = engine;
            return Ok(true);
        }
        if self
            .arrangement
            .tracks
            .iter()
            .zip(&arrangement.tracks)
            .any(|(old, next)| {
                old.clips != next.clips || old.muted != next.muted || old.soloed != next.soloed
            })
        {
            self.all_notes_off();
            self.reconstruct_notes = true;
        }
        self.pending.clear();
        let solo = arrangement.tracks.iter().any(|track| track.soloed);
        for track in &arrangement.tracks {
            let nodes = &self.tracks[&track.name];
            let gain = if track.muted || (solo && !track.soloed) {
                0.0
            } else {
                track.gain
            };
            for (id, value) in [(0, gain), (1, track.pan)] {
                self.pending.push(NodeEvent {
                    node: nodes.gain,
                    event: Event {
                        frame: 0,
                        data: EventData::Parameter { id, value },
                    },
                });
            }
        }
        self.pending.push(NodeEvent {
            node: self.master,
            event: Event {
                frame: 0,
                data: EventData::Parameter {
                    id: 0,
                    value: mixer.master_gain,
                },
            },
        });
        self.arrangement = arrangement;
        Ok(false)
    }

    fn notes_for_window(
        arrangement: &Arrangement,
        start: u64,
        frames: u64,
        reconstruct: bool,
    ) -> Vec<(String, Event)> {
        let mut events = Vec::new();
        let solo = arrangement.tracks.iter().any(|track| track.soloed);
        for track in &arrangement.tracks {
            for clip in &track.clips {
                if crate::validate_clip(clip).is_err() {
                    continue;
                }
                let clip_end = clip.start + clip.length;
                if clip_end < start || (clip.start >= start && clip.start - start >= frames) {
                    continue;
                }
                for note in &clip.notes {
                    let on = clip.start + note.start;
                    let off = on + note.length;
                    let elapsed = if reconstruct && on < start && start < off {
                        start - on
                    } else {
                        0
                    };
                    for (frame, kind, velocity, offset) in [
                        (
                            on + elapsed,
                            1,
                            f32::from(note.velocity) / 127.0,
                            elapsed as f32,
                        ),
                        (off, 2, 0.0, 0.0),
                    ] {
                        if kind == 1 && (track.muted || (solo && !track.soloed)) {
                            continue;
                        }
                        if let Some(relative) = frame.checked_sub(start)
                            && relative < frames
                            && let Ok(frame) = usize::try_from(relative)
                        {
                            events.push((
                                track.name.clone(),
                                Event {
                                    frame,
                                    data: EventData::Custom {
                                        kind,
                                        data: [f32::from(note.key), velocity, offset, 0.0],
                                    },
                                },
                            ));
                        }
                    }
                }
            }
        }
        events.sort_by_key(|(_, event)| {
            (
                event.frame,
                matches!(event.data, EventData::Custom { kind: 1, .. }),
            )
        });
        events
    }

    pub fn render_block(
        &mut self,
        transport: &mut Transport,
        arrangement: &Arrangement,
    ) -> Result<(f32, f32)> {
        self.render_block_into(transport, arrangement, &mut Vec::new(), &mut Vec::new())
    }

    pub fn render_block_into(
        &mut self,
        transport: &mut Transport,
        arrangement: &Arrangement,
        left: &mut Vec<f32>,
        right: &mut Vec<f32>,
    ) -> Result<(f32, f32)> {
        let start = transport.project_frame;
        let discontinuity =
            self.transport_revision != transport.revision || self.playing != transport.playing;
        let events = if transport.playing {
            Self::notes_for_window(
                arrangement,
                start,
                BLOCK_FRAMES as u64,
                discontinuity || self.reconstruct_notes,
            )
        } else {
            Vec::new()
        };
        let mut node_events: Vec<NodeEvent> = events
            .into_iter()
            .filter_map(|(name, event)| {
                self.tracks.get(&name).map(|nodes| NodeEvent {
                    node: nodes.instrument,
                    event,
                })
            })
            .collect();
        node_events.splice(0..0, self.pending.drain(..));
        if discontinuity {
            self.graph.reset();
            self.transport_revision = transport.revision;
            self.playing = transport.playing;
        }
        node_events.splice(0..0, self.live.drain(..));
        let context = ProcessContext {
            sample_rate: self.sample_rate,
            engine_frame: transport.engine_frame,
            project_frame: transport.project_frame,
            playing: transport.playing,
        };
        let output = self
            .graph
            .process(context, BLOCK_FRAMES, &node_events)
            .map_err(|error| Error(format!("Graph error: {error:?}")))?;
        self.reconstruct_notes = false;
        transport.advance(BLOCK_FRAMES);
        left.extend_from_slice(output.channel(0));
        right.extend_from_slice(output.channel(1));
        let last = BLOCK_FRAMES - 1;
        Ok((output.channel(0)[last], output.channel(1)[last]))
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }
}

pub fn state_from_records(records: &BTreeMap<String, Record>) -> Result<(Arrangement, MixerState)> {
    let arrangement = records
        .get("arrangement")
        .ok_or_else(|| Error("Missing arrangement record".into()))
        .and_then(|record| {
            serde_json::from_value::<Arrangement>(record.state.clone())
                .map_err(|error| Error(error.to_string()))
        })?;
    let mixer = records
        .get("mixer")
        .map(|record| {
            serde_json::from_value::<MixerState>(record.state.clone())
                .map_err(|error| Error(error.to_string()))
        })
        .transpose()?
        .unwrap_or_default();
    Ok((arrangement, mixer))
}

pub fn beat_frames(beat: f64) -> u64 {
    (beat * BEAT as f64) as u64
}

pub fn frames_to_beats(frames: u64) -> f64 {
    frames as f64 / BEAT as f64
}

pub fn note_events_in_range(
    arrangement: &Arrangement,
    start: u64,
    frames: u64,
) -> Vec<(String, Event)> {
    Engine::notes_for_window(arrangement, start, frames, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Clip, Note, Track};
    use sound_core::registry::Registry;
    use std::sync::Arc;

    #[test]
    fn note_events_window_extraction() {
        let arrangement = Arrangement {
            tracks: vec![Track {
                name: "a".into(),
                clips: vec![Clip {
                    start: 100,
                    length: 1000,
                    notes: vec![Note {
                        start: 0,
                        length: 200,
                        key: 60,
                        velocity: 100,
                    }],
                }],
                ..Track::default()
            }],
        };
        let events = Engine::notes_for_window(&arrangement, 50, 100, false);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1.frame, 50);
        assert!(matches!(
            events[0].1.data,
            EventData::Custom { kind: 1, .. }
        ));
        let events = Engine::notes_for_window(&arrangement, 0, 100, false);
        assert!(events.is_empty());
        let mut arrangement = arrangement;
        arrangement.tracks[0].clips[0].length = 200;
        let events = Engine::notes_for_window(&arrangement, 300, 128, false);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0].1.data,
            EventData::Custom { kind: 2, .. }
        ));
        assert_eq!(events[0].1.frame, 0);
    }

    #[test]
    fn discontinuity_events_restore_only_active_notes_and_keep_scheduled_offs() {
        let mut arrangement = Arrangement {
            tracks: vec![Track {
                clips: vec![Clip {
                    start: 100,
                    length: 200,
                    notes: vec![Note {
                        start: 0,
                        length: 200,
                        key: 60,
                        velocity: 127,
                    }],
                }],
                ..Track::default()
            }],
        };
        let events = Engine::notes_for_window(&arrangement, 200, 128, true);
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0].1,
            Event {
                frame: 0,
                data: EventData::Custom {
                    kind: 1,
                    data: [60.0, 1.0, 100.0, 0.0]
                },
            }
        );
        assert_eq!(
            events[1].1,
            Event {
                frame: 100,
                data: EventData::Custom {
                    kind: 2,
                    data: [60.0, 0.0, 0.0, 0.0]
                },
            }
        );
        assert_eq!(
            Engine::notes_for_window(&arrangement, 200, 128, false),
            events[1..]
        );
        let onset = Engine::notes_for_window(&arrangement, 100, 128, true);
        assert_eq!(onset.len(), 1);
        assert_eq!(
            onset[0].1.data,
            EventData::Custom {
                kind: 1,
                data: [60.0, 1.0, 0.0, 0.0]
            }
        );
        let end = Engine::notes_for_window(&arrangement, 300, 128, true);
        assert_eq!(end.len(), 1);
        assert!(matches!(end[0].1.data, EventData::Custom { kind: 2, .. }));
        assert!(Engine::notes_for_window(&arrangement, 200, 0, true).is_empty());
        arrangement.tracks[0].muted = true;
        assert_eq!(
            Engine::notes_for_window(&arrangement, 200, 128, true),
            events[1..]
        );
        arrangement.tracks[0].muted = false;
        arrangement.tracks.push(Track {
            name: "Solo".into(),
            soloed: true,
            ..Track::default()
        });
        assert_eq!(
            Engine::notes_for_window(&arrangement, 200, 128, true),
            events[1..]
        );
    }

    #[test]
    fn public_event_windows_handle_overflow_and_malformed_clips() {
        let mut arrangement = Arrangement {
            tracks: vec![Track {
                clips: vec![Clip {
                    start: u64::MAX - 1,
                    length: 1,
                    notes: vec![Note {
                        start: 0,
                        length: 1,
                        key: 60,
                        velocity: 127,
                    }],
                }],
                ..Track::default()
            }],
        };
        let events = note_events_in_range(&arrangement, u64::MAX - 1, 128);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].1.frame, 0);
        assert_eq!(events[1].1.frame, 1);
        assert_eq!(note_events_in_range(&arrangement, u64::MAX, 128).len(), 1);
        arrangement.tracks[0].clips[0].length = 2;
        assert!(note_events_in_range(&arrangement, u64::MAX - 1, 128).is_empty());
        arrangement.tracks[0].clips[0].start = 0;
        arrangement.tracks[0].clips[0].notes[0].start = u64::MAX;
        assert!(note_events_in_range(&arrangement, 0, u64::MAX).is_empty());
    }

    #[test]
    fn synth_retriggers_active_notes_on_resume_seek_and_rebuild() {
        active_notes_survive_discontinuities("daw.synth");
    }

    #[test]
    fn sampler_restores_active_notes_on_resume_seek_and_rebuild() {
        active_notes_survive_discontinuities("daw.sampler");
    }

    fn active_notes_survive_discontinuities(instrument: &str) {
        use crate::sample_asset::{
            import_sample,
            tests::{TestDirectory, float_wav},
        };
        let directory = TestDirectory::new();
        let mut registry = Registry::default();
        let tool = registry
            .register::<Arrangement>(ARRANGEMENT_TOOL, crate::validate_arrangement)
            .unwrap();
        let mut project =
            Project::create(&directory.0, Arc::new(registry), "Active notes").unwrap();
        let sampler = if instrument == "daw.sampler" {
            let source = directory.0.join("ramp.wav");
            let samples: Vec<_> = (0..4096).map(|frame| frame as f32 / 4096.0).collect();
            std::fs::write(&source, float_wav(1, 24_000, &samples)).unwrap();
            Some(crate::sampler::SamplerConfig {
                asset: import_sample(project.root(), &source).unwrap(),
                root_key: 72,
            })
        } else {
            None
        };
        let mut arrangement = Arrangement {
            tracks: vec![Track {
                instrument: instrument.into(),
                sampler,
                gain: 1.0,
                clips: vec![Clip {
                    start: 64,
                    length: 4096,
                    notes: vec![Note {
                        start: 16,
                        length: 2000,
                        key: 60,
                        velocity: 127,
                    }],
                }],
                ..Track::default()
            }],
        };
        project
            .insert("arrangement", tool, &arrangement, None)
            .unwrap();
        let mut engine = Engine::build(&project).unwrap();
        let mut transport = Transport::default();
        transport.seek(80);
        transport.play();
        let onset = render_left(&mut engine, &mut transport, &arrangement);
        assert!(onset.iter().any(|sample| sample.abs() > 0.001));
        transport.pause();
        assert_eq!(block_peak(&mut engine, &mut transport, &arrangement), 0.0);
        transport.play();
        assert_active_block(&mut engine, &mut transport, &arrangement, &onset);
        transport.seek(401);
        assert_active_block(&mut engine, &mut transport, &arrangement, &onset);
        transport.seek(200);
        assert_active_block(&mut engine, &mut transport, &arrangement, &onset);
        arrangement.tracks.push(Track {
            name: "Rebuild".into(),
            ..Track::default()
        });
        project.replace("arrangement", tool, &arrangement).unwrap();
        assert!(engine.apply_project(&project).unwrap());
        assert_active_block(&mut engine, &mut transport, &arrangement, &onset);
        let continuous = render_left(&mut engine, &mut transport, &arrangement);
        assert!(continuous.iter().any(|sample| sample.abs() > 0.001));
        if instrument == "daw.synth" {
            assert_ne!(continuous, onset);
        }
        transport.seek(2048);
        let ending = render_left(&mut engine, &mut transport, &arrangement);
        assert!(ending[..32].iter().any(|sample| sample.abs() > 0.001));
        if instrument == "daw.sampler" {
            assert!(ending[32..].iter().all(|sample| *sample == 0.0));
        }
        for _ in 0..100 {
            block_peak(&mut engine, &mut transport, &arrangement);
        }
        assert_eq!(block_peak(&mut engine, &mut transport, &arrangement), 0.0);
        transport.seek(2080);
        assert_eq!(block_peak(&mut engine, &mut transport, &arrangement), 0.0);
        transport.seek(79);
        let before = render_left(&mut engine, &mut transport, &arrangement);
        assert_eq!(before[0], 0.0);
        assert_eq!(&before[1..], &onset[..BLOCK_FRAMES - 1]);
        transport.pause();
        transport.seek(500);
        assert_eq!(block_peak(&mut engine, &mut transport, &arrangement), 0.0);
        transport.play();
        assert_active_block(&mut engine, &mut transport, &arrangement, &onset);
    }

    fn render_left(
        engine: &mut Engine,
        transport: &mut Transport,
        arrangement: &Arrangement,
    ) -> Vec<f32> {
        let mut left = Vec::new();
        engine
            .render_block_into(transport, arrangement, &mut left, &mut Vec::new())
            .unwrap();
        left
    }

    fn assert_active_block(
        engine: &mut Engine,
        transport: &mut Transport,
        arrangement: &Arrangement,
        onset: &[f32],
    ) {
        let elapsed = transport.project_frame - 80;
        let output = render_left(engine, transport, arrangement);
        assert!(output.iter().any(|sample| sample.abs() > 0.001));
        if arrangement.tracks[0].instrument == "daw.sampler" {
            let gain = std::f32::consts::FRAC_1_SQRT_2.powi(3);
            for (frame, sample) in output.iter().enumerate() {
                let expected = (elapsed as f32 + frame as f32) * 0.25 / 4096.0 * gain;
                assert!((sample - expected).abs() < 1e-6, "{sample} != {expected}");
            }
        } else {
            assert_eq!(output, onset);
        }
    }

    #[test]
    fn sampler_dispatch_loads_assets_and_rebuilds_on_config_changes() {
        use crate::{
            sample_asset::{
                import_sample,
                tests::{TestDirectory, float_wav},
            },
            sampler::SamplerConfig,
        };
        let directory = TestDirectory::new();
        let mut registry = Registry::default();
        let tool = registry
            .register::<Arrangement>(ARRANGEMENT_TOOL, |_| Ok(()))
            .unwrap();
        let mut project = Project::create(&directory.0, Arc::new(registry), "Sampler").unwrap();
        let source = directory.0.join("original.wav");
        std::fs::write(&source, float_wav(2, 48_000, &[0.5, -0.25].repeat(256))).unwrap();
        let asset = import_sample(project.root(), &source).unwrap();
        std::fs::remove_file(source).unwrap();
        let mut arrangement = Arrangement {
            tracks: vec![Track {
                instrument: "daw.sampler".into(),
                sampler: Some(SamplerConfig {
                    asset: asset.clone(),
                    root_key: 60,
                }),
                gain: 1.0,
                clips: vec![Clip {
                    start: 0,
                    length: 256,
                    notes: vec![Note {
                        start: 0,
                        length: 256,
                        key: 60,
                        velocity: 127,
                    }],
                }],
                ..Track::default()
            }],
        };
        project
            .insert("arrangement", tool, &arrangement, None)
            .unwrap();
        let mut engine = Engine::build(&project).unwrap();
        let mut transport = Transport::default();
        transport.play();
        let (left, right) = engine.render_block(&mut transport, &arrangement).unwrap();
        assert!(left > 0.1 && right < -0.05);
        assert!((left + 2.0 * right).abs() < 1e-6);
        arrangement.tracks[0].gain = 0.5;
        project.replace("arrangement", tool, &arrangement).unwrap();
        assert!(!engine.apply_project(&project).unwrap());
        arrangement.tracks[0].sampler.as_mut().unwrap().root_key = 72;
        project.replace("arrangement", tool, &arrangement).unwrap();
        assert!(engine.apply_project(&project).unwrap());
        let second =
            import_sample(project.root(), &directory.0.join("assets").join(asset)).unwrap();
        arrangement.tracks[0].sampler.as_mut().unwrap().asset = second;
        project.replace("arrangement", tool, &arrangement).unwrap();
        assert!(engine.apply_project(&project).unwrap());
        arrangement.tracks[0].sampler.as_mut().unwrap().asset = "missing.wav".into();
        project.replace("arrangement", tool, &arrangement).unwrap();
        assert!(engine.apply_project(&project).is_err());
        assert!(Engine::build(&project).is_err());
        arrangement.tracks[0].sampler = None;
        arrangement.tracks[0].instrument = "daw.synth".into();
        project.replace("arrangement", tool, &arrangement).unwrap();
        assert!(engine.apply_project(&project).unwrap());
        arrangement.tracks[0].instrument = "unsupported".into();
        project.replace("arrangement", tool, &arrangement).unwrap();
        assert!(Engine::build(&project).is_err());
        assert!(engine.apply_project(&project).is_err());
    }

    fn block_peak(
        engine: &mut Engine,
        transport: &mut Transport,
        arrangement: &Arrangement,
    ) -> f32 {
        let mut left = Vec::new();
        let mut right = Vec::new();
        engine
            .render_block_into(transport, arrangement, &mut left, &mut right)
            .unwrap();
        left.iter()
            .chain(&right)
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()))
    }

    #[test]
    fn live_notes_play_stopped_release_with_tails_and_reset_on_transport_changes() {
        let directory = crate::sample_asset::tests::TestDirectory::new();
        let mut registry = Registry::default();
        let tool = registry
            .register::<Arrangement>(ARRANGEMENT_TOOL, |_| Ok(()))
            .unwrap();
        let mut project = Project::create(&directory.0, Arc::new(registry), "Live MIDI").unwrap();
        let arrangement = Arrangement {
            tracks: vec![Track {
                name: "lead".into(),
                ..Track::default()
            }],
        };
        project
            .insert("arrangement", tool, &arrangement, None)
            .unwrap();
        let mut engine = Engine::build(&project).unwrap();
        let mut transport = Transport::default();
        engine.queue_note("lead", 60, 100, true);
        assert!(block_peak(&mut engine, &mut transport, &arrangement) > 0.01);
        assert_eq!(transport.project_frame, 0);
        assert_eq!(transport.engine_frame, BLOCK_FRAMES as u64);
        engine.queue_note("lead", 60, 0, true);
        assert!(block_peak(&mut engine, &mut transport, &arrangement) > 0.0);
        for _ in 0..100 {
            block_peak(&mut engine, &mut transport, &arrangement);
        }
        assert_eq!(block_peak(&mut engine, &mut transport, &arrangement), 0.0);
        for transition in 0..4 {
            engine.queue_note("lead", 60, 100, true);
            assert!(block_peak(&mut engine, &mut transport, &arrangement) > 0.01);
            match transition {
                0 => transport.play(),
                1 => transport.pause(),
                2 => transport.seek(48_000),
                _ => transport.stop(),
            }
            assert_eq!(block_peak(&mut engine, &mut transport, &arrangement), 0.0);
        }
        engine.queue_note("lead", 60, 100, true);
        engine.all_notes_off();
        assert_eq!(block_peak(&mut engine, &mut transport, &arrangement), 0.0);
        for (track, key, velocity) in [("missing", 60, 100), ("lead", 128, 100), ("lead", 60, 128)]
        {
            engine.queue_note(track, key, velocity, true);
        }
        assert_eq!(block_peak(&mut engine, &mut transport, &arrangement), 0.0);
    }

    #[test]
    fn live_notes_survive_gain_edits_but_not_mute_solo_or_graph_rebuilds() {
        let directory = crate::sample_asset::tests::TestDirectory::new();
        let mut registry = Registry::default();
        let tool = registry
            .register::<Arrangement>(ARRANGEMENT_TOOL, |_| Ok(()))
            .unwrap();
        let mut project =
            Project::create(&directory.0, Arc::new(registry), "Live MIDI edits").unwrap();
        let mut arrangement = Arrangement {
            tracks: vec![
                Track {
                    name: "lead".into(),
                    ..Track::default()
                },
                Track {
                    name: "other".into(),
                    ..Track::default()
                },
            ],
        };
        project
            .insert("arrangement", tool, &arrangement, None)
            .unwrap();
        let mut engine = Engine::build(&project).unwrap();
        let mut transport = Transport::default();
        engine.queue_note("lead", 60, 100, true);
        arrangement.tracks[0].gain = 0.5;
        project.replace("arrangement", tool, &arrangement).unwrap();
        assert!(!engine.apply_project(&project).unwrap());
        assert!(block_peak(&mut engine, &mut transport, &arrangement) > 0.01);
        engine.queue_note("lead", 60, 0, false);
        arrangement.tracks[0].gain = 0.75;
        project.replace("arrangement", tool, &arrangement).unwrap();
        engine.apply_project(&project).unwrap();
        for _ in 0..100 {
            block_peak(&mut engine, &mut transport, &arrangement);
        }
        assert_eq!(block_peak(&mut engine, &mut transport, &arrangement), 0.0);
        for solo in [false, true] {
            engine.queue_note("lead", 60, 100, true);
            assert!(block_peak(&mut engine, &mut transport, &arrangement) > 0.01);
            arrangement.tracks[0].muted = !solo;
            arrangement.tracks[1].soloed = solo;
            project.replace("arrangement", tool, &arrangement).unwrap();
            engine.apply_project(&project).unwrap();
            engine.queue_note("lead", 62, 100, true);
            assert_eq!(block_peak(&mut engine, &mut transport, &arrangement), 0.0);
            arrangement.tracks[0].muted = false;
            arrangement.tracks[1].soloed = false;
            project.replace("arrangement", tool, &arrangement).unwrap();
            engine.apply_project(&project).unwrap();
            assert_eq!(block_peak(&mut engine, &mut transport, &arrangement), 0.0);
        }
        engine.queue_note("lead", 60, 100, true);
        assert!(block_peak(&mut engine, &mut transport, &arrangement) > 0.01);
        engine.queue_note("lead", 62, 100, true);
        arrangement.tracks[0].fx.filter =
            Some(crate::arrangement::FilterSettings { cutoff: 500.0 });
        project.replace("arrangement", tool, &arrangement).unwrap();
        assert!(engine.apply_project(&project).unwrap());
        assert_eq!(block_peak(&mut engine, &mut transport, &arrangement), 0.0);
        engine.queue_note("lead", 60, 100, true);
        assert!(block_peak(&mut engine, &mut transport, &arrangement) > 0.001);
        arrangement.tracks.remove(0);
        project.replace("arrangement", tool, &arrangement).unwrap();
        assert!(engine.apply_project(&project).unwrap());
        engine.queue_note("lead", 60, 100, true);
        assert_eq!(block_peak(&mut engine, &mut transport, &arrangement), 0.0);
    }

    #[test]
    fn end_to_end_project_renders_non_silent_audio() {
        let root = std::env::temp_dir().join(format!("engine-test-{}", std::process::id()));
        let mut registry = Registry::default();
        let tool = registry
            .register::<Arrangement>(ARRANGEMENT_TOOL, |_| Ok(()))
            .unwrap();
        let registry = Arc::new(registry);
        let mut project = Project::create(&root, registry, "Test").unwrap();
        let arrangement = Arrangement {
            tracks: vec![Track {
                name: "lead".into(),
                clips: vec![Clip {
                    start: 0,
                    length: BEAT * 4,
                    notes: vec![Note {
                        start: 0,
                        length: BEAT,
                        key: 69,
                        velocity: 100,
                    }],
                }],
                ..Track::default()
            }],
        };
        project
            .insert("arrangement", tool, &arrangement, None)
            .unwrap();
        let mut engine = Engine::build(&project).unwrap();
        let mut transport = Transport::default();
        let mut silent_left = Vec::new();
        let mut silent_right = Vec::new();
        engine
            .render_block_into(
                &mut transport,
                &arrangement,
                &mut silent_left,
                &mut silent_right,
            )
            .unwrap();
        assert!(
            silent_left.iter().all(|sample| *sample == 0.0),
            "stopped transport must not trigger timeline notes"
        );
        transport.play();
        let mut peak = 0.0f32;
        for _ in 0..20 {
            let (left, right) = engine.render_block(&mut transport, &arrangement).unwrap();
            peak = peak.max(left.abs()).max(right.abs());
        }
        assert!(peak > 0.05, "expected audible output, peak {peak}");
        assert_eq!(transport.project_frame, 20 * BLOCK_FRAMES as u64);
        transport.pause();
        for _ in 0..100 {
            engine.render_block(&mut transport, &arrangement).unwrap();
        }
        let (left, right) = engine.render_block(&mut transport, &arrangement).unwrap();
        assert_eq!((left, right), (0.0, 0.0));
        assert_eq!(transport.project_frame, 20 * BLOCK_FRAMES as u64);
        std::fs::remove_dir_all(root).unwrap();
    }
}
