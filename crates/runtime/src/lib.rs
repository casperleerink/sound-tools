pub mod session;

use sound_core::{
    Error, Result,
    clock::Transport,
    project::Project,
    registry::{Registry, Tool},
};
use sound_daw::{
    arrangement::{Arrangement, BEAT, Clip, Note, Track, validate_arrangement},
    engine::{ARRANGEMENT_TOOL, BLOCK_FRAMES, Engine, MIXER_TOOL, MixerState, state_from_records},
};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

pub fn register_tools(registry: &mut Registry) -> Result<(Tool<Arrangement>, Tool<MixerState>)> {
    let arrangement_tool =
        registry.register::<Arrangement>(ARRANGEMENT_TOOL, validate_arrangement)?;
    let mixer_tool = registry.register::<MixerState>(MIXER_TOOL, |value| {
        if (-2.0..=2.0).contains(&value.master_gain) {
            Ok(())
        } else {
            Err(Error("Master gain must be -2..2".into()))
        }
    })?;
    Ok((arrangement_tool, mixer_tool))
}

pub fn open_or_create(
    root: &str,
    registry: Arc<Registry>,
    arrangement_tool: Tool<Arrangement>,
    mixer_tool: Tool<MixerState>,
) -> Result<Project> {
    let path = Path::new(&root);
    if path.join("project.json").exists() {
        Project::open(path, registry)
    } else {
        let mut project = Project::create(path, registry, "New Piece")?;
        let starter = Track {
            name: "Lead".into(),
            clips: vec![Clip {
                start: 0,
                length: BEAT * 4,
                notes: vec![
                    Note {
                        start: 0,
                        length: BEAT / 2,
                        key: 60,
                        velocity: 100,
                    },
                    Note {
                        start: BEAT / 2,
                        length: BEAT / 2,
                        key: 64,
                        velocity: 100,
                    },
                    Note {
                        start: BEAT,
                        length: BEAT / 2,
                        key: 67,
                        velocity: 100,
                    },
                    Note {
                        start: BEAT + BEAT / 2,
                        length: BEAT / 2,
                        key: 72,
                        velocity: 100,
                    },
                    Note {
                        start: BEAT * 2,
                        length: BEAT * 2,
                        key: 65,
                        velocity: 90,
                    },
                ],
            }],
            ..Track::default()
        };
        project.insert(
            "arrangement",
            arrangement_tool,
            &Arrangement {
                tracks: vec![starter],
            },
            None,
        )?;
        project.insert("mixer", mixer_tool, &MixerState::default(), None)?;
        Ok(project)
    }
}

pub fn write_wav(path: &Path, left: &[f32], right: &[f32], sample_rate: u32) -> Result<()> {
    let mut data = Vec::with_capacity(left.len() * 4);
    for (l, r) in left.iter().zip(right) {
        data.extend_from_slice(&l.to_le_bytes());
        data.extend_from_slice(&r.to_le_bytes());
    }
    let mut wav = Vec::with_capacity(44 + data.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&((36 + data.len()) as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&3u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 8).to_le_bytes());
    wav.extend_from_slice(&8u16.to_le_bytes());
    wav.extend_from_slice(&32u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
    wav.extend_from_slice(&data);
    sound_core::project::atomic_write(path, &wav)
}

pub fn render_wav(project: &Project, seconds: f64, out: &str) -> Result<()> {
    if !seconds.is_finite() || !(0.0..=3600.0).contains(&seconds) {
        return Err(Error(
            "Render duration must be finite and between zero and 3600 seconds".into(),
        ));
    }
    let mut engine = Engine::build(project)?;
    let (arrangement, _mixer) = state_from_records(project.records())?;
    let mut transport = Transport::default();
    transport.play();
    let frames = (seconds * 48_000.0).round() as usize;
    let blocks = frames.div_ceil(BLOCK_FRAMES);
    let mut left = Vec::with_capacity(blocks * BLOCK_FRAMES);
    let mut right = Vec::with_capacity(blocks * BLOCK_FRAMES);
    for _ in 0..blocks {
        engine.render_block_into(&mut transport, &arrangement, &mut left, &mut right)?;
    }
    left.truncate(frames);
    right.truncate(frames);
    write_wav(Path::new(out), &left, &right, 48_000)?;
    println!("rendered {seconds}s to {out}");
    Ok(())
}

pub fn inspect(project: &Project) {
    let manifest = project.manifest();
    println!("project: {}", manifest.name);
    let clock = &manifest.clock;
    println!(
        "tempo: {} bpm {}/{}",
        clock.tempo[0].bpm, clock.numerator, clock.denominator
    );
    for (id, record) in project.records() {
        println!("record {id}: {}", record.tool);
    }
    for connection in &manifest.connections {
        println!(
            "connection {}:{} -> {}:{}",
            connection.source.instance,
            connection.source.port,
            connection.target.instance,
            connection.target.port
        );
    }
}

pub fn play_device(project: &mut Project, seconds: f64) -> Result<()> {
    use sound_core::device::DeviceOutput;
    if !seconds.is_finite() || !(0.0..=3600.0).contains(&seconds) {
        return Err(Error(
            "Playback duration must be between zero and 3600 seconds".into(),
        ));
    }
    let mut output = DeviceOutput::open()?;
    let mut engine = Engine::build(project)?;
    let (mut arrangement, _) = state_from_records(project.records())?;
    let mut transport = Transport::default();
    transport.play();
    let mut left = Vec::with_capacity(BLOCK_FRAMES);
    let mut right = Vec::with_capacity(BLOCK_FRAMES);
    for _ in 0..output.free_frames() / BLOCK_FRAMES {
        left.clear();
        right.clear();
        engine.render_block_into(&mut transport, &arrangement, &mut left, &mut right)?;
        output.write(&left, &right)?;
    }
    output.start()?;
    println!("Output: {} at {} Hz", output.name(), output.sample_rate());
    let start = Instant::now();
    let mut poll = Instant::now();
    while start.elapsed().as_secs_f64() < seconds {
        if poll.elapsed() >= Duration::from_millis(50) {
            match project.poll() {
                Ok(true) => {
                    engine.apply_project(project)?;
                    arrangement = state_from_records(project.records())?.0;
                }
                Ok(false) => {}
                Err(error) => eprintln!("Project edit rejected: {error}"),
            }
            poll = Instant::now();
        }
        for _ in 0..output.free_frames() / BLOCK_FRAMES {
            left.clear();
            right.clear();
            engine.render_block_into(&mut transport, &arrangement, &mut left, &mut right)?;
            output.write(&left, &right)?;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    println!(
        "Playback ended; {} underrun frames, {} callback frames",
        output.underruns(),
        output.frames_served()
    );
    Ok(())
}

pub fn watch(project: &mut Project, seconds: f64) -> Result<()> {
    let mut engine = Engine::build(project)?;
    let (mut arrangement, _mixer) = state_from_records(project.records())?;
    let mut transport = Transport::default();
    transport.play();
    let mut window_start = Instant::now();
    let mut peak = 0.0f32;
    loop {
        std::thread::sleep(Duration::from_millis(50));
        if project.poll()? {
            match state_from_records(project.records()) {
                Ok((next_arrangement, _)) if validate_arrangement(&next_arrangement).is_ok() => {
                    engine = Engine::build(project)?;
                    arrangement = next_arrangement;
                    println!("project edit applied at frame {}", transport.project_frame);
                }
                Ok(_) => eprintln!("invalid arrangement edit ignored"),
                Err(error) => eprintln!("invalid project edit ignored: {error}"),
            }
        }
        let (l, r) = engine.render_block(&mut transport, &arrangement)?;
        peak = peak.max(l.abs()).max(r.abs());
        if window_start.elapsed() >= Duration::from_secs_f64(seconds) {
            println!(
                "window {:.1}s peak {peak:.4} frame {}",
                window_start.elapsed().as_secs_f64(),
                transport.project_frame
            );
            window_start = Instant::now();
            peak = 0.0;
        }
    }
}
