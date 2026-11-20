//! Feeds the views bind: numbers as live values, lists drawn by `shapes`, all without Bun.

use std::time::{Duration, Instant};

use super::*;

const SCOPE: &str = "song/t00/chain/scope";

/// A tool that plays what it hears through the analyzers `an0`… inside it, then a compressor,
/// and shows their feeds: a number, a spectrum of each analyzer, and the compressor's curve.
fn scope(analyzers: usize) -> String {
    let mut chain: Vec<String> = (0..analyzers).map(|at| format!("an{at}")).collect();
    chain.push("comp".into());
    let ends = std::iter::once(r#"{ port: "audio" }"#.to_string())
        .chain(
            chain
                .iter()
                .map(|child| format!(r#"{{ child: "{child}" }}"#)),
        )
        .chain(std::iter::once(r#"{ port: "audio" }"#.to_string()))
        .collect::<Vec<_>>();
    let connections = (ends.windows(2))
        .map(|pair| format!("{{ from: {}, to: {} }}", pair[0], pair[1]))
        .collect::<Vec<_>>()
        .join(", ");
    let spectra = (0..analyzers)
        .map(|at| {
            format!(
                r#"<shapes key="spectrum-{at}" w={{512}} h={{64}} units={{[512, [0, -90]]}} lines={{[{{ color: "sound", width: 1, points: live.feed("spectrum", "an{at}"), fill: "sound/24" }}]}} />"#
            )
        })
        .collect::<String>();
    format!(
        r#"import {{ h, live, tool }} from "./sdk";
export const scope = tool({{
  name: "scope", title: "Scope", when: "Never", doc: "Shows what it hears.", state: {{}},
  behaviour: () => ({{ connections: [{connections}] }}),
  view: () => (
    <col>
      <text key="loudness" live={{live.feed("loudness", "an0")}} digits={{1}} />
      {spectra}
      <shapes key="curve" w={{120}} h={{120}} units={{[120, [0, -60]]}} lines={{[{{ color: "text", width: 1.5, points: live.feed("transfer", "comp") }}]}} />
    </col>
  ),
}});
"#
    )
}

/// The project with the scope as the first effect of the first track, shown as the main view.
fn scoped(analyzers: usize) -> (tempfile::TempDir, State) {
    scoped_as(&scope(analyzers), analyzers)
}

/// [`scoped`] with the tool `code`.
fn scoped_as(code: &str, analyzers: usize) -> (tempfile::TempDir, State) {
    let (scratch, folder) = with_tools(&[("scope", code)]);
    let chain = folder.join("state/song/t00/chain");
    std::fs::remove_file(chain.join("scope.json")).unwrap();
    std::fs::create_dir(chain.join("scope")).unwrap();
    let mut files = vec![
        (
            "instance.json".to_string(),
            json!({ "tool": "scope", "state": {} }),
        ),
        (
            "comp.json".to_string(),
            json!({ "tool": "compressor", "state": { "threshold_db": -30.0 } }),
        ),
    ];
    for at in 0..analyzers {
        files.push((
            format!("an{at}.json"),
            json!({ "tool": "analyzer", "state": {} }),
        ));
    }
    for (name, record) in files {
        std::fs::write(chain.join("scope").join(name), record.to_string()).unwrap();
    }
    let mut state = State::open(Options {
        folder,
        headless: true,
        play: None,
        main: Some(InstanceId::new(SCOPE).unwrap()),
        act: None,
    })
    .unwrap();
    frame(&mut state, 0.0);
    (scratch, state)
}

fn read(state: &State, name: &str) -> Option<Value> {
    state
        .feeds
        .read_now()
        .get(&format!("{SCOPE}/{name}"))
        .cloned()
}

#[test]
fn a_number_and_lists_of_devices_inside_move_while_it_plays_and_bun_runs_no_view() {
    let (_scratch, mut state) = scoped(1);
    let runs = state.tools.as_ref().unwrap().view_runs();
    play(&mut state, 0.0, 1.0);
    let loudness = read(&state, "an0 loudness").and_then(|read| read.as_f64());
    assert!(loudness.is_some_and(|lufs| lufs > -60.0), "{loudness:?}");
    // A list is asked for a number a point of the width it is drawn at, two pixels a point; the
    // analyzer has 96 bands whatever it is asked.
    assert_eq!(read(&state, "an0 spectrum"), Some(json!({ "points": 96 })));
    assert_eq!(
        read(&state, "comp transfer"),
        Some(json!({ "points": 120 }))
    );
    let mut sizes = state.lives.pixels();
    sizes.sort();
    assert_eq!(sizes, [(240, 240), (1024, 128)]);
    // What no element binds is not read.
    assert_eq!(read(&state, "an0 note"), None);
    assert!(state.feeds.drew() > 0);
    assert_eq!(state.tools.as_ref().unwrap().view_runs(), runs);
}

#[test]
fn a_list_that_rests_is_not_drawn_again_until_its_record_changes() {
    let (_scratch, mut state) = scoped(0);
    build(&state);
    frame(&mut state, 1.0 / 60.0);
    frame(&mut state, 2.0 / 60.0);
    assert_eq!(state.feeds.drew(), 0);
    let record = state
        .project()
        .root()
        .join("state/song/t00/chain/scope/comp.json");
    let lower = json!({ "tool": "compressor", "state": { "threshold_db": -50.0 } });
    std::fs::write(&record, lower.to_string()).unwrap();
    (state.session)
        .background(|project| project.apply_outside_changes(std::slice::from_ref(&record)))
        .unwrap();
    frame(&mut state, 3.0 / 60.0);
    assert!(state.feeds.drew() > 0);
}

#[test]
fn a_list_in_a_plane_reads_and_draws_only_the_part_the_plane_shows() {
    // The compressor's curve from -60 to 0 dB in, 1200 units long in a plane that shows 300.
    let code = r#"import { h, live, tool } from "./sdk";
export const scope = tool({
  name: "scope", title: "Scope", when: "Never", doc: "Shows its curve.", state: {},
  behaviour: () => ({ connections: [{ from: { port: "audio" }, to: { child: "comp" } }, { from: { child: "comp" }, to: { port: "audio" } }] }),
  view: () => (
    <box key="curve" w={300} h={60} plane={{ x: 1, y: 60 }}>
      <shapes key="long" place={[0, 0, 1200, 1]} units={[[-60, 0], [0, -60]]} lines={[{ color: "text", width: 1, points: live.feed("transfer", "comp") }]} />
    </box>
  ),
});
"#;
    let (_scratch, mut state) = scoped_as(code, 0);
    // Above the loudest it hears it changes nothing: a level out is the level in.
    let record = state
        .project()
        .root()
        .join("state/song/t00/chain/scope/comp.json");
    let flat = json!({ "tool": "compressor", "state": { "threshold_db": 0.0 } });
    std::fs::write(&record, flat.to_string()).unwrap();
    (state.session)
        .background(|project| project.apply_outside_changes(std::slice::from_ref(&record)))
        .unwrap();
    let mut window = laid_out(state);
    redraw(&mut window);
    let seen = |window: &rune::Runtime<State>| {
        let [(lists, rect)] = <[_; 1]>::try_from(window.state.lives.lists()).unwrap();
        let curve = &lists[0];
        let ends = [curve[0], curve[curve.len() - 1]].map(|db| db.round());
        (curve.len(), ends, rect, window.state.lives.pixels()[0])
    };
    // A quarter of it shows: a point a number of the 300 that show, the first quarter of its
    // levels, and pixels as wide as that.
    assert_eq!(
        seen(&window),
        (
            300,
            [-60.0, -45.0],
            Some([0.0, 0.0, 300.0, 60.0]),
            (600, 120)
        )
    );
    let state = &window.state;
    state
        .planes
        .set_view("curve", Some(600.0), None, None)
        .unwrap();
    state
        .lives
        .move_planes(state, &["curve".to_string()], false);
    assert_eq!(
        seen(&window),
        (
            300,
            [-30.0, -15.0],
            Some([600.0, 0.0, 300.0, 60.0]),
            (600, 120)
        )
    );
}

/// The window's part of a frame of a playing piece with eight spectra 512 points wide: its feeds,
/// its live values and Rune's frame, not the engine, which a window plays on its own thread.
#[test]
fn eight_spectra_512_points_wide_draw_within_two_milliseconds_a_frame() {
    let (_scratch, state) = scoped(8);
    let mut window = laid_out(state);
    redraw(&mut window);
    let runs = window.state.tools.as_ref().unwrap().view_runs();
    window
        .state
        .session
        .background(|project| project.engine().play());
    let mut seconds = window.state.last;
    let mut times: Vec<Duration> = Vec::new();
    for _ in 0..120 {
        seconds += 1.0 / 60.0;
        frame(&mut window.state, seconds);
        let started = Instant::now();
        window.state.feeds.next_frame(1.0 / 60.0);
        window.state.lives.update(&window.state, 1.0 / 60.0);
        window.frame(1.0 / 60.0);
        // Only frames in which every spectrum moved and drew again.
        if window.state.feeds.drew() == 8 {
            times.push(started.elapsed());
        }
    }
    assert!(
        times.len() > 10,
        "{} frames drew every spectrum",
        times.len()
    );
    assert_eq!(window.state.lives.pixels().len(), 9);
    assert_eq!(window.state.tools.as_ref().unwrap().view_runs(), runs);
    // The least: other work on this machine only adds to a frame.
    let least = times.into_iter().min().unwrap();
    eprintln!("eight spectra: {least:?} a frame");
    // Drawing pixels is about nine times slower in a test build, which is not optimized.
    let unoptimized = if cfg!(debug_assertions) { 10 } else { 1 };
    assert!(least < Duration::from_millis(2 * unoptimized), "{least:?}");
}
