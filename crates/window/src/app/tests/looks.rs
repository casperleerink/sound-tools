//! The looks `--tree` warns of: a background flush with one edge of its frame but a little in
//! from the edges beside it, as the mixer's chosen name once stood, and a background over a
//! border around it, as the mixer once lay over the line above it.

use serde_json::json;

use super::{act, key, laid_out, new_project, opened, redraw, with_tools};
use crate::app::State;
use crate::tree::looks::{Seen, mistakes, paints};

/// Columns as the mixer's were: the name of the chosen one has a fill, the other a hover fill,
/// and both stand in from the sides of their column but touch its top. Beside them, a name
/// that reaches every edge of its column and one with room all round.
const COLUMNS: &str = r#"import { boolean, h, input, tool } from "./sdk";
export const columns = tool({
  name: "columns", title: "Columns", when: "Never", doc: "Four columns.", sound: () => input,
  state: { on: boolean({ default: false }) },
  view: () => (
    <row gap={2}>
      <col w={100} pad={[1, 0]} gap={1} fill="background" border="border-strong">
        <row key="chosen" h={28} fill="selected" />
        <box h={80} />
      </col>
      <col w={100} pad={[1, 0]} gap={1} fill="background" border="border-strong">
        <row key="hovered" h={28} hover="fill" />
        <box h={80} />
      </col>
      <col w={100} gap={1} fill="background" border="border-strong">
        <row key="edge" h={28} fill="selected" />
        <box h={80} />
      </col>
      <col w={100} pad={1} gap={1} fill="background" border="border-strong">
        <row key="room" h={28} fill="selected" hover="fill" />
        <box h={80} />
      </col>
    </row>
  ),
});
"#;

/// Lines that boxes without keys draw, and what lies over them: the space under the tracks as
/// it was, its line under the mixer that fills it; a card whose header has a fill on hover.
/// Then what is right: a button's line round its own words, a light that fills a pad while it
/// sounds, rows that scroll under the top of their list, and a line drawn over what fills it.
const LINES: &str = r#"import { boolean, h, input, tool } from "./sdk";
export const lines = tool({
  name: "lines", title: "Lines", when: "Never", doc: "Lines.", sound: () => input,
  state: { on: boolean({ default: false }) },
  view: () => (
    <col pad={2} gap={2}>
      <box w={300} h={60} border="border-strong" borderSides={[1, 0, 0, 0]}>
        <col key="mixer" w="full" h="full" fill="background" />
      </box>
      <col w={300} h={60} radius="lg" fill="surface" border="border-strong">
        <row key="header" h={32} radius="md" hover="fill" />
      </col>
      <row key="button" h={28} pad={[2, 0]} align="center" radius="md" border="border" hover="fill">
        <text>Play</text>
      </row>
      <box w={60} h={60} radius="md" fill="fill" border="text-secondary" clip>
        <box key="light" at={[0, 0]} w={60} h={60} fill="sound/24" inert />
      </box>
      <col w={300} h={60} border="border" borderSides={[1, 0, 1, 0]} scroll="y">
        <row key="row" w="full" h={28} fill="selected" />
        <row w="full" h={60} />
      </col>
      <box w={300} h={60}>
        <col key="under" w="full" h="full" fill="background" />
        <box inset={[0, 0, 0, null]} h={1} fill="border-strong" inert />
      </box>
    </col>
  ),
});
"#;

fn seen(window: &rune::Runtime<State>) -> Vec<String> {
    let shown: Vec<Seen> = (window.elements().into_iter())
        .map(|element| Seen {
            moving: element.moving || element.exiting,
            path: element.path,
            key: element.key,
            label: element.label,
            rect: element.rect,
            fill: element.fill.hex(),
        })
        .collect();
    mistakes(&shown, &paints(&window.state))
}

/// What the warnings say of the view of `tool`, by the key in it of what each names.
fn said_of(tool: &str, view: &str) -> Vec<(String, String)> {
    let (_scratch, folder) = with_tools(&[(tool, view)]);
    let prefix = format!("`t00/chain/{tool}:");
    let said = seen(&laid_out(opened(&folder)));
    (said.into_iter())
        .map(|line| {
            let key = (line.strip_prefix(&prefix))
                .and_then(|rest| rest.split_once('`'))
                .map_or_else(String::new, |(key, _)| key.to_string());
            (key, line)
        })
        .collect()
}

fn flush(tool_said: Vec<(String, String)>) -> Vec<(String, String)> {
    (tool_said.into_iter())
        .filter(|(_, line)| line.contains("touches the"))
        .collect()
}

#[test]
fn a_name_flush_with_the_top_of_its_column_but_in_from_its_sides_is_named_with_its_room() {
    let said = flush(said_of("columns", COLUMNS));
    let of = |key: &str| {
        (said.iter())
            .find(|(named, _)| named == key)
            .map(|(_, line)| line)
            .unwrap_or_else(|| panic!("{said:#?}"))
    };
    let chosen = of("chosen");
    assert!(
        chosen.contains("its background touches the top edge"),
        "{chosen}"
    );
    assert!(
        chosen.contains("4 points in from the left and right edges"),
        "{chosen}"
    );
    let hovered = of("hovered");
    assert!(
        hovered.contains("its hover background touches the top edge"),
        "{hovered}"
    );
}

#[test]
fn a_name_that_reaches_every_edge_or_has_room_all_round_is_not_named() {
    let said = flush(said_of("columns", COLUMNS));
    let named: Vec<&str> = said.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(named, ["chosen", "hovered"], "{said:#?}");
}

#[test]
fn a_background_over_the_line_of_a_box_without_a_key_is_named_with_the_side() {
    let said = said_of("lines", LINES);
    let of = |key: &str| -> Vec<&str> {
        (said.iter())
            .filter(|(named, _)| named == key)
            .map(|(_, line)| line.as_str())
            .collect()
    };
    let mixer = of("mixer");
    assert_eq!(mixer.len(), 1, "{said:#?}");
    assert!(
        mixer[0].contains("its background lies over the top line of the box at")
            && mixer[0].contains("all along it"),
        "{}",
        mixer[0]
    );
    let header = of("header").join("\n");
    for side in ["top", "right", "left"] {
        assert!(
            header.contains(&format!("its hover background lies over the {side} line")),
            "{header}"
        );
    }
    assert!(
        header.contains("pokes over the rounded top left, top right corners"),
        "{header}"
    );
}

#[test]
fn a_control_round_its_own_content_a_light_a_scrolled_row_or_a_line_drawn_over_is_not_named() {
    let said = said_of("lines", LINES);
    let named: Vec<&str> = said.iter().map(|(key, _)| key.as_str()).collect();
    assert!(
        named.iter().all(|key| ["mixer", "header"].contains(key)),
        "{said:#?}"
    );
}

/// Frames until what opened or moved has come to rest: what still moves is not checked.
fn settle(window: &mut rune::Runtime<State>) {
    redraw(window);
    for _ in 0..4 {
        window.frame(0.5);
    }
}

/// The template's pages as the composer opens them: the song with every effect's card and each
/// instrument's and a return, a track's lanes, the master and a return chosen, the mixer with a
/// strip chosen, the note editor, the open menus, the gallery.
#[test]
fn the_template_has_no_mistakes_in_its_looks() {
    let scratch = new_project();
    let folder = scratch.path().join("song");
    let song = folder.join("state/song");
    let effects = [
        "eq",
        "filter",
        "saturator",
        "vocoder",
        "pitch-correction",
        "compressor",
        "gate",
        "limiter",
        "modulation",
        "delay",
        "reverb",
        "utility",
        "analyzer",
    ];
    for name in effects {
        let record = json!({ "tool": name, "state": {} }).to_string();
        std::fs::write(song.join(format!("t00/chain/{name}.json")), record).unwrap();
    }
    let order = json!({ "tool": "chain", "state": { "order": effects } }).to_string();
    std::fs::write(song.join("t00/chain/instance.json"), order).unwrap();
    for (track, instrument) in [
        ("t01", "wavetable"),
        ("t02", "sampler"),
        ("t03", "drum-pad"),
    ] {
        if track != "t01" {
            super::copy_folder(&song.join("t01"), &song.join(track)).unwrap();
        }
        let record = json!({ "tool": instrument, "state": {} }).to_string();
        std::fs::write(song.join(format!("{track}/synth.json")), record).unwrap();
    }
    let mut window = laid_out(opened(&folder));
    act(
        &mut window.state,
        json!({ "key": "add-track-kind", "action": "open" }),
    );
    act(
        &mut window.state,
        json!({ "key": "add-track-kind-Return track", "action": "press" }),
    );
    act(
        &mut window.state,
        json!({ "key": "track-t00", "action": "press" }),
    );
    settle(&mut window);
    let mut said = Vec::new();
    let mut look = |window: &rune::Runtime<State>, scene: &str| {
        said.extend(
            seen(window)
                .into_iter()
                .map(|line| format!("{scene}: {line}")),
        );
    };
    look(&window, "song");
    let steps = [
        ("lanes", json!({ "key": "lanes-t00", "action": "press" })),
        (
            "wavetable",
            json!({ "key": "track-t01", "action": "press" }),
        ),
        ("sampler", json!({ "key": "track-t02", "action": "press" })),
        ("drum pad", json!({ "key": "track-t03", "action": "press" })),
        ("master", json!({ "key": "master", "action": "press" })),
        ("return", json!({ "key": "track-ret-a", "action": "press" })),
        (
            "note editor",
            json!({ "key": "clip-t00/bass", "action": "press", "value": { "clicks": 2 } }),
        ),
        ("mixer", json!({ "action": "song.mixer" })),
        (
            "chosen strip",
            json!({ "key": "strip-name-t01", "action": "press" }),
        ),
    ];
    for (scene, step) in steps {
        act(&mut window.state, step);
        settle(&mut window);
        look(&window, scene);
    }
    act(&mut window.state, json!({ "action": "song.mixer" }));
    act(
        &mut window.state,
        json!({ "key": "track-t00", "action": "press" }),
    );
    let menus = [
        json!({ "key": "add-track-kind", "action": "open" }),
        json!({ "key": "add-effect", "action": "open" }),
        json!({ "key": "snap", "action": "open" }),
        json!({ "key": "colour-t00", "action": "open" }),
        json!({ "key": "slot-instrument-title", "action": "open" }),
        json!({ "key": "slot-compressor-sidechain", "action": "open" }),
        json!({ "key": "t00/chain/eq:bands[0].shape", "action": "open" }),
        json!({ "key": "t00/synth:cutoff_hz-value", "action": "open" }),
        json!({ "key": "t00/synth:card-more", "action": "press" }),
        json!({ "key": "track-t00", "action": "menu" }),
        json!({ "key": "clip-t00/bass", "action": "menu" }),
    ];
    for menu in menus {
        let scene = format!("menu of {}", menu["key"]);
        act(&mut window.state, menu);
        settle(&mut window);
        look(&window, &scene);
        key(&mut window, rune::Key::Escape);
    }
    window.state.main = Some(sound_core::InstanceId::new("gallery").unwrap());
    settle(&mut window);
    look(&window, "gallery");
    assert!(said.is_empty(), "{said:#?}");
}
