//! The template of a new project (`crates/window/template`) plays a song as the DAW template's
//! tools did: two tracks of clips with a filter whose cutoff a lane moves, sends into a reverb
//! and into a return before the faders, and the master's limiter.

use std::path::Path;

use crate::shared_song::song_files;
use crate::support::{BAR, Harness, write};

/// The tools of the template, as a new project gets them.
fn template_tools(folder: &Path, under: &Path, files: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            template_tools(&path, under, files);
        } else if !name.contains(".test.") {
            let relative = path
                .strip_prefix(under)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            files.push((relative, std::fs::read_to_string(&path).unwrap()));
        }
    }
}

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
fn the_template_plays_a_song_as_the_daw_template_did() {
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let template = Path::new(env!("CARGO_MANIFEST_DIR")).join("../window/template");
    let mut files: Vec<(String, String)> = song_files(2, 2)
        .into_iter()
        .filter(|(path, _)| !path.starts_with("extensions/"))
        .collect();
    template_tools(&template.join("extensions"), &template, &mut files);
    let folder = tempfile::tempdir().unwrap();
    for (path, text) in files {
        write(folder.path(), &path, &text);
    }
    let mut harness = Harness::open(folder);
    assert_eq!(harness.project.problems(), []);
    harness.render(9_600);
    let played = levels(&harness.play_from_the_start(3 * BAR));
    // What the tools of `templates/daw` played, sample for sample, before the template had its own.
    assert_eq!(played, [(-12.2, -25.6), (-17.7, -32.0), (-21.4, -48.0)]);
}
