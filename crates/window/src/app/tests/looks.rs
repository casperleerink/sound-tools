//! The looks `--tree` warns of: a background flush with one edge of its frame but a little in
//! from the edges beside it, as the mixer's chosen name once stood.

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

fn seen(window: &rune::Runtime<State>) -> Vec<String> {
    let elements = window.elements().into_iter();
    let shown: Vec<Seen> = elements
        .filter(|element| !element.moving && !element.exiting)
        .map(|element| Seen {
            path: element.path,
            key: element.key,
            label: element.label,
            rect: element.rect,
            fill: element.fill.hex(),
        })
        .collect();
    mistakes(&shown, &paints(&window.state))
}

fn columns() -> Vec<String> {
    let (_scratch, folder) = with_tools(&[("columns", COLUMNS)]);
    seen(&laid_out(opened(&folder)))
}

#[test]
fn a_name_flush_with_the_top_of_its_column_but_in_from_its_sides_is_named_with_its_room() {
    let said = columns();
    let of = |key: &str| {
        said.iter()
            .find(|line| line.starts_with(&format!("`t00/chain/columns:{key}`")))
    };
    let chosen = of("chosen").unwrap_or_else(|| panic!("{said:#?}"));
    assert!(
        chosen.contains("its background touches the top edge"),
        "{chosen}"
    );
    assert!(
        chosen.contains("4 points in from the left and right edges"),
        "{chosen}"
    );
    let hovered = of("hovered").unwrap_or_else(|| panic!("{said:#?}"));
    assert!(
        hovered.contains("its hover background touches the top edge"),
        "{hovered}"
    );
}

#[test]
fn a_name_that_reaches_every_edge_or_has_room_all_round_is_not_named() {
    let said = columns();
    assert_eq!(said.len(), 2, "{said:#?}");
    assert!(
        !said
            .iter()
            .any(|line| line.contains(":edge`") || line.contains(":room`")),
        "{said:#?}"
    );
}

/// The template's pages as the composer opens them: the song with every effect's card and each
/// instrument's, the mixer with a strip chosen, the note editor, the open menus, the gallery.
#[test]
fn the_template_has_no_background_flush_with_one_edge_and_in_from_the_next() {
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
        (
            "wavetable",
            json!({ "key": "track-t01", "action": "press" }),
        ),
        ("sampler", json!({ "key": "track-t02", "action": "press" })),
        ("drum pad", json!({ "key": "track-t03", "action": "press" })),
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
        redraw(&mut window);
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
        json!({ "key": "t00/synth:card-more", "action": "press" }),
        json!({ "key": "track-t00", "action": "menu" }),
        json!({ "key": "clip-t00/bass", "action": "menu" }),
    ];
    for menu in menus {
        let scene = format!("menu of {}", menu["key"]);
        act(&mut window.state, menu);
        redraw(&mut window);
        look(&window, &scene);
        key(&mut window, rune::Key::Escape);
    }
    window.state.main = Some(sound_core::InstanceId::new("gallery").unwrap());
    redraw(&mut window);
    look(&window, "gallery");
    assert!(said.is_empty(), "{said:#?}");
}
