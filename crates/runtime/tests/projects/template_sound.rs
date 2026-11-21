//! The DAW template plays a song as its tools of the old views did: two tracks of clips with a
//! filter whose cutoff a lane moves, sends into a reverb and into a return before the faders,
//! and the master's limiter.

use crate::shared_song::song_files;
use crate::support::{BAR, Harness, write};

/// The loudest sample and the root mean square of each bar, in dB to a tenth.
fn levels(samples: &[f32]) -> Vec<(f32, f32)> {
    let db = |amplitude: f32| (20.0 * amplitude.log10() * 10.0).round() / 10.0;
    (samples.chunks(BAR * 2))
        .map(|bar| {
            let peak = bar
                .iter()
                .fold(0.0_f32, |most, sample| most.max(sample.abs()));
            let mean = bar.iter().map(|sample| sample * sample).sum::<f32>() / bar.len() as f32;
            (db(peak), db(mean.sqrt()))
        })
        .collect()
}

#[test]
fn the_daw_template_plays_a_song_as_its_old_tools_did() {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let folder = tempfile::tempdir().unwrap();
    for (path, text) in song_files(2, 2) {
        write(folder.path(), &path, &text);
    }
    let mut harness = Harness::open(folder);
    assert_eq!(harness.project.problems(), []);
    harness.render(9_600);
    let played = levels(&harness.play_from_the_start(3 * BAR));
    // What the old tools played, sample for sample.
    assert_eq!(played, [(-12.2, -25.6), (-17.7, -32.0), (-21.4, -48.0)]);
}
