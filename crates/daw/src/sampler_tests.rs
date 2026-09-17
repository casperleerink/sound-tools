use super::*;
use crate::sample_asset::tests::float_wav;

fn sampler(samples: &[f32], channels: u16, rate: u32) -> Sampler {
    let asset = SampleAsset::decode_wav(&float_wav(channels, rate, samples)).unwrap();
    let mut sampler = Sampler::new(asset, 60).unwrap();
    sampler
        .prepare(Prepare {
            sample_rate: 48_000.0,
            max_frames: 128,
        })
        .unwrap();
    sampler
}

fn event(frame: usize, kind: u32, key: f32, velocity: f32) -> Event {
    Event {
        frame,
        data: EventData::Custom {
            kind,
            data: [key, velocity, 0.0, 0.0],
        },
    }
}

fn render(sampler: &mut Sampler, total: usize, block: usize, events: &[Event]) -> Vec<[f32; 2]> {
    let input = AudioBuffer::new(0, block);
    let mut output = AudioBuffer::new(2, block);
    let mut result = Vec::new();
    while result.len() < total {
        let start = result.len();
        let frames = block.min(total - start);
        output.set_frames(frames).unwrap();
        let events: Vec<_> = events
            .iter()
            .filter(|event| event.frame >= start && event.frame < start + frames)
            .map(|event| Event {
                frame: event.frame - start,
                data: event.data,
            })
            .collect();
        sampler.process(
            ProcessContext {
                sample_rate: 48_000.0,
                engine_frame: start as u64,
                project_frame: start as u64,
                playing: true,
            },
            &input,
            &mut output,
            &events,
        );
        result
            .extend((0..frames).map(|frame| [output.channel(0)[frame], output.channel(1)[frame]]));
    }
    result
}

#[test]
fn note_on_off_all_off_and_reset_are_sample_accurate() {
    let mut processor = sampler(&[0.5; 128], 1, 48_000);
    let events = [
        event(2, 1, 60.0, 0.5),
        event(5, 2, 61.0, 0.0),
        event(7, 2, 60.0, 0.0),
        event(9, 1, 60.0, 1.0),
        event(11, 3, 0.0, 0.0),
    ];
    let response = render(&mut processor, 16, 16, &events);
    assert_eq!(&response[..2], &[[0.0; 2]; 2]);
    assert_eq!(&response[2..7], &[[0.25; 2]; 5]);
    assert_eq!(&response[7..9], &[[0.0; 2]; 2]);
    assert_eq!(&response[9..11], &[[0.5; 2]; 2]);
    assert_eq!(&response[11..], &[[0.0; 2]; 5]);
    render(&mut processor, 1, 1, &[event(0, 1, 60.0, 1.0)]);
    processor.reset();
    assert_eq!(render(&mut processor, 8, 8, &[]), vec![[0.0; 2]; 8]);
}

#[test]
fn pitch_interpolation_and_stereo_preserve_channel_values() {
    let samples = [0.0, 0.0, 0.25, -0.25, 0.5, -0.5, 0.75, -0.75];
    let root = render(
        &mut sampler(&samples, 2, 48_000),
        5,
        5,
        &[event(0, 1, 60.0, 1.0)],
    );
    assert_eq!(
        root,
        vec![
            [0.0; 2],
            [0.25, -0.25],
            [0.5, -0.5],
            [0.75, -0.75],
            [0.0; 2]
        ]
    );
    let up = render(
        &mut sampler(&samples, 2, 48_000),
        3,
        3,
        &[event(0, 1, 72.0, 1.0)],
    );
    assert_eq!(up, vec![[0.0; 2], [0.5, -0.5], [0.0; 2]]);
    let down = render(
        &mut sampler(&samples, 2, 48_000),
        9,
        9,
        &[event(0, 1, 48.0, 1.0)],
    );
    assert_eq!(
        down,
        vec![
            [0.0; 2],
            [0.125, -0.125],
            [0.25, -0.25],
            [0.375, -0.375],
            [0.5, -0.5],
            [0.625, -0.625],
            [0.75, -0.75],
            [0.375, -0.375],
            [0.0; 2]
        ]
    );
    let resampled = render(
        &mut sampler(&samples, 2, 24_000),
        9,
        9,
        &[event(0, 1, 60.0, 1.0)],
    );
    assert_eq!(down, resampled);
    let asset = SampleAsset::decode_wav(&float_wav(2, 48_000, &samples)).unwrap();
    let mut custom_root = Sampler::new(asset, 72).unwrap();
    assert_eq!(
        render(&mut custom_root, 9, 9, &[event(0, 1, 60.0, 1.0)]),
        down
    );
}

#[test]
fn elapsed_output_frames_convert_to_pitched_sample_positions() {
    let samples: Vec<_> = (0..1024).map(|frame| frame as f32 / 1024.0).collect();
    for output_rate in [24_000.0, 48_000.0, 96_000.0] {
        for key in [48.0, 60.0, 72.0] {
            let mut processor = sampler(&samples, 1, 48_000);
            processor
                .prepare(Prepare {
                    sample_rate: output_rate,
                    max_frames: 128,
                })
                .unwrap();
            let start = Event {
                frame: 0,
                data: EventData::Custom {
                    kind: 1,
                    data: [key, 1.0, 7.0, 0.0],
                },
            };
            let output = render(&mut processor, 8, 8, &[start, event(5, 2, key, 0.0)]);
            let step = 48_000.0 / output_rate * ((f64::from(key) - 60.0) / 12.0).exp2();
            for (frame, sample) in output[..5].iter().enumerate() {
                let expected = ((7 + frame) as f64 * step / 1024.0) as f32;
                assert_eq!(*sample, [expected; 2]);
                assert!(expected > 0.0);
            }
            assert_eq!(output[5..], [[0.0; 2]; 3]);
        }
    }
    for offset in [-1.0, f32::NAN, f32::INFINITY, 1024.0, f32::MAX] {
        let mut processor = sampler(&samples, 1, 48_000);
        let output = render(
            &mut processor,
            8,
            8,
            &[Event {
                frame: 0,
                data: EventData::Custom {
                    kind: 1,
                    data: [60.0, 1.0, offset, 0.0],
                },
            }],
        );
        assert_eq!(output, [[0.0; 2]; 8]);
    }
}

#[test]
fn rendering_is_block_size_independent() {
    let samples: Vec<_> = (0..1024)
        .map(|frame| (frame as f32 * 0.1).sin() * 0.5)
        .collect();
    let events = [
        event(3, 1, 61.0, 0.7),
        event(127, 1, 48.0, 0.4),
        event(256, 2, 61.0, 0.0),
        event(301, 1, 48.0, 0.6),
        event(500, 3, 0.0, 0.0),
        event(700, 1, 72.0, 1.0),
        event(800, 1, 72.0, 0.0),
    ];
    let expected = render(&mut sampler(&samples, 1, 44_100), 1024, 1, &events);
    for block in [7, 64, 128] {
        assert_eq!(
            render(&mut sampler(&samples, 1, 44_100), 1024, block, &events),
            expected
        );
    }
}

#[test]
fn voice_cap_retrigger_and_invalid_events() {
    let mut processor = sampler(&[1.0; 128], 1, 48_000);
    let events: Vec<_> = (0..40).map(|key| event(0, 1, key as f32, 1.0)).collect();
    assert_eq!(render(&mut processor, 1, 1, &events), vec![[32.0; 2]]);
    assert_eq!(
        processor.voices.iter().filter(|voice| voice.active).count(),
        32
    );
    processor.reset();
    let invalid = [
        event(0, 1, f32::NAN, 1.0),
        event(0, 1, -1.0, 1.0),
        event(0, 1, 128.0, 1.0),
        event(0, 1, 60.5, 1.0),
        event(0, 1, 60.0, f32::INFINITY),
        event(0, 1, 60.0, -0.1),
        event(0, 1, 60.0, 1.1),
    ];
    assert_eq!(render(&mut processor, 1, 1, &invalid), vec![[0.0; 2]]);
    let mut processor = sampler(&[0.25, 0.5, 0.75], 1, 48_000);
    let response = render(
        &mut processor,
        4,
        4,
        &[event(0, 1, 60.0, 1.0), event(2, 1, 60.0, 0.5)],
    );
    assert_eq!(response, vec![[0.25; 2], [0.5; 2], [0.125; 2], [0.25; 2]]);
}

#[test]
fn config_is_backward_compatible_and_validated() {
    let mut json = serde_json::to_value(crate::Track::default()).unwrap();
    json.as_object_mut().unwrap().remove("sampler");
    let mut track: crate::Track = serde_json::from_value(json).unwrap();
    assert!(track.sampler.is_none());
    assert!(crate::validate_track(&track).is_ok());
    track.instrument = "unknown".into();
    assert!(crate::validate_track(&track).is_err());
    track.instrument = "daw.sampler".into();
    assert!(crate::validate_track(&track).is_err());
    let config: SamplerConfig = serde_json::from_str(r#"{"asset":"sample.wav"}"#).unwrap();
    assert_eq!(config.root_key, 60);
    track.sampler = Some(config);
    assert!(crate::validate_track(&track).is_ok());
    track.sampler.as_mut().unwrap().root_key = 128;
    assert!(crate::validate_track(&track).is_err());
    track.sampler.as_mut().unwrap().root_key = 60;
    track.sampler.as_mut().unwrap().asset = "../external.wav".into();
    assert!(crate::validate_track(&track).is_err());
}
