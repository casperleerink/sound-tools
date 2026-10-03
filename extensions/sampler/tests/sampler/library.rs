//! Library instruments: a record that names one this machine lacks waits for its download,
//! which only starts where downloads are allowed, and plays once it is done.
//!
//! The files come from a folder laid out as GitHub serves them, through curl's `file://`.
//! The library is global to the process, so one test walks the whole way.

use sampler::library::{self, Status};
use sampler::{LibraryId, SamplerState};

use crate::support::{Harness, SAMPLE_RATE, id, note, write_wav};

const CELLO: &str = "vsco/cello-section-sustain";

#[test]
fn a_library_instrument_downloads_where_allowed_and_then_plays() {
    let entry = library::entry(CELLO).unwrap();
    let machine = tempfile::tempdir().unwrap();
    // The repository at its commit, with the instrument's SFZ file and one sample in a folder
    // whose name has a space, as VSCO's have.
    let repository = machine
        .path()
        .join("github")
        .join(entry.library.repository)
        .join(entry.library.commit);
    std::fs::create_dir_all(&repository).unwrap();
    std::fs::write(
        repository.join(entry.sfz),
        "<control> default_path=Strings\\Cello Section\\\n<region> sample=a.wav key=60 amp_veltrack=0",
    )
    .unwrap();
    library::set_source(&format!(
        "file://{}",
        machine.path().join("github").display()
    ));
    library::set_folder(machine.path().join("library"));

    // As in `--render`: the instrument is not here, and nothing downloads it.
    let mut harness = Harness::with_samples(&[]);
    let cello = SamplerState {
        library: Some(LibraryId::try_from(CELLO.to_string()).unwrap()),
        ..SamplerState::default()
    };
    harness.add_track(vec![note(0, 4_800, 60, 100)], cello);
    assert_eq!(library::status(entry), Status::Missing);
    let problems = harness.project.problems();
    assert_eq!(
        problems[0].message,
        "Cello section, sustain is not in the library of this machine, so the Sampler is silent. Open the project in the app to download it (69 MB)"
    );

    // As in the window: the Sampler starts the download and waits for it. Its sample is not
    // on the server yet, so it fails, says so, and is not tried again by itself.
    library::allow_downloads();
    let instrument = id("track/instrument");
    harness.project.rebind(&instrument).unwrap();
    library::wait_for_downloads();
    assert!(matches!(library::status(entry), Status::Failed(_)));
    assert_eq!(
        library::take_finished(harness.project.assets()),
        std::slice::from_ref(&instrument)
    );
    harness.project.rebind(&instrument).unwrap();
    let problem = &harness.project.problems()[0].message;
    assert!(
        problem.starts_with(
            "the download of Cello section, sustain failed, so the Sampler is silent: "
        ),
        "{problem}"
    );
    assert!(!matches!(
        library::status(entry),
        Status::Downloading { .. }
    ));

    // Download on the card tries again, and the Sampler runs again to wait for it.
    let sample = repository.join("Strings/Cello Section/a.wav");
    write_wav(&sample, SAMPLE_RATE, &vec![0.25; SAMPLE_RATE as usize]);
    library::download(entry);
    harness.project.rebind(&instrument).unwrap();
    library::wait_for_downloads();
    assert_eq!(library::status(entry), Status::Here);
    assert_eq!(
        library::take_finished(harness.project.assets()),
        std::slice::from_ref(&instrument)
    );
    harness.project.rebind(&instrument).unwrap();
    assert_eq!(harness.project.problems(), []);
    assert_eq!(harness.play(4_800)[2_400], 0.25);

    // Only the files the instrument needs came, under the commit, and nothing else.
    let here = machine
        .path()
        .join("library")
        .join(entry.library.id)
        .join(entry.library.commit);
    assert!(here.join("Strings/Cello Section/a.wav").exists());
    assert!(here.join(".cello-section-sustain.done").exists());
    assert!(library::take_finished(harness.project.assets()).is_empty());
}
