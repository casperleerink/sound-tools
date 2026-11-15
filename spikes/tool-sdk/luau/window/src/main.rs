//! The window: owns the song (the records), runs the project's Luau tool inside this process, and
//! moves the values that change every frame (playhead, scroll, meters, time) itself.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use daw_model::{self as model, Options, Scenario, Song, WINDOW};
use rune::color::hex;
use rune::{AppSpec, Color, Cx, Drag, El, LineCap, Live, LiveText, Phase, Shape, TextStyle, Tx, Waker, canvas, el, text};
use rune_luau::mlua::{FromLua, LuaSerdeExt};
use rune_luau::{Data, HostNode, NodeArgs, Script, ScriptOptions, Shadows, Sources};
use serde_json::Value;

const PROJECT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../project");
const THEME: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../theme.json");
/// How far the timeline is built past each edge of the window, as the tool's `PAGE`.
const PAGE: f32 = 1200.0;

/// What a `live` node follows, set from the window's sources every frame.
enum Follow {
    X { source: String, scale: f32, add: f32, live: Live<[f32; 2]> },
    Level { source: String, w: f32, h: f32, live: Live<[f32; 4]> },
    Text { live: LiveText },
}

/// The live values tool nodes are bound to, by node key, and what they read from.
#[derive(Default)]
struct Lives {
    nodes: RefCell<HashMap<String, Follow>>,
    tick: Cell<f32>,
    scroll: Cell<f32>,
    levels: RefCell<Vec<[f32; 2]>>,
    drags: RefCell<HashMap<String, Live<[f32; 2]>>>,
}

impl Lives {
    fn source(&self, name: &str) -> f32 {
        match name {
            "tick" => self.tick.get(),
            "scroll" => -self.scroll.get(),
            _ => {
                let mut parts = name.split('/').skip(1);
                let index: usize = parts.next().and_then(|n| n.parse().ok()).unwrap_or(1);
                let side = usize::from(parts.next() == Some("right"));
                self.levels.borrow().get(index.saturating_sub(1)).map_or(0.0, |l| l[side])
            }
        }
    }

    fn set(&self, follow: &Follow) {
        match follow {
            Follow::X { source, scale, add, live } => _ = live.set([self.source(source) * scale + add, 0.0]),
            Follow::Level { source, w, h, live } => {
                let filled = self.source(source).clamp(0.0, 1.0) * h;
                live.set([0.0, h - filled, *w, filled]);
            }
            Follow::Text { live } => _ = live.set(model::position(self.tick.get() as u64)),
        }
    }

    fn update(&self) {
        for follow in self.nodes.borrow().values() {
            self.set(follow);
        }
    }
}

struct App {
    script: Script,
    options: Options,
    click: bool,
    song: Song,
    end: u64,
    playing: bool,
    zoom: f32,
    page: i64,
    edits: u64,
    frames: u64,
    last: f64,
    clicks: u64,
    lives: Rc<Lives>,
    requests: Rc<RefCell<Vec<(String, Data)>>>,
    /// Changes applied so far, and how many of them the last view showed.
    applied: Cell<u64>,
    shown: Cell<u64>,
    pending: Cell<Option<(Instant, u64)>>,
    latencies: RefCell<Vec<f64>>,
    log: Vec<String>,
}

fn colour(rgb: &str) -> Color {
    hex(rgb).unwrap_or(Color { r: 1.0, g: 0.0, b: 1.0, a: 1.0 })
}

fn text_style(theme: &Value, name: &str) -> TextStyle {
    let t = &theme["text"][name];
    let n = |k: &str| t[k].as_f64().unwrap_or(0.0) as f32;
    let style = TextStyle::new(n("size"), n("weight")).lh(n("lineHeight")).tracking(n("tracking"));
    if t["tabular"].as_bool() == Some(true) { style.features("'tnum' 1") } else { style }
}

fn script_options(theme: &Value, lives: &Rc<Lives>, requests: &Rc<RefCell<Vec<(String, Data)>>>) -> ScriptOptions {
    let colours = theme["color"].clone();
    let shadows = theme["shadow"].clone();
    let lua_theme = theme.clone();
    let queue = requests.clone();
    let (l1, l2, l3) = (lives.clone(), lives.clone(), lives.clone());
    let text_theme = theme.clone();
    ScriptOptions::new()
        .palette(Rc::new(move |name: &str, _dark: bool| colours[name].as_str().and_then(hex)))
        .shadows(Rc::new(move |name: &str, _dark: bool| {
            let s = &shadows[name];
            let n = |k: &str| s[k].as_f64().unwrap_or(0.0) as f32;
            s.is_object().then(|| vec![rune::Shadow { x: 0.0, y: n("y"), blur: n("blur"), spread: 0.0, color: colour(s["color"].as_str().unwrap_or("#000000")), inset: false }])
        }) as Shadows)
        .on_log(|level, line| eprintln!("luau {level:?}: {line}"))
        .setup(move |host| {
            let lua = host.lua();
            let module = lua.create_table()?;
            let queue = queue.clone();
            module.raw_set(
                "change",
                lua.create_function(move |lua, (path, value): (String, rune_luau::mlua::Value)| {
                    queue.borrow_mut().push((path, Data::from_lua(value, lua)?));
                    Ok(())
                })?,
            )?;
            module.raw_set("theme", lua.to_value(&lua_theme)?)?;
            host.register_module("@app/host", module)
        })
        // A handler's change requests land once it returns, in the same transaction.
        .around(|state, tx, run| {
            run(tx);
            if let Some(app) = state.downcast_mut::<App>() {
                app.apply_requests();
            }
        })
        .node(HostNode::new("live", move |a| {
            let key = a.string("key").ok_or("live needs a key")?.to_owned();
            let follow = if let Some(source) = a.string("x") {
                let (scale, add) = (a.number("scale").unwrap_or(1.0) as f32, a.number("add").unwrap_or(0.0) as f32);
                let live = match l1.nodes.borrow().get(&key) {
                    Some(Follow::X { live, .. }) => live.clone(),
                    _ => Live::new([0.0; 2]),
                };
                Follow::X { source: source.to_owned(), scale, add, live }
            } else if let Some(source) = a.string("level") {
                let (w, h) = (a.number("w").unwrap_or(4.0) as f32, a.number("h").unwrap_or(0.0) as f32);
                let live = match l1.nodes.borrow().get(&key) {
                    Some(Follow::Level { live, .. }) => live.clone(),
                    _ => Live::new([0.0, h, w, 0.0]),
                };
                Follow::Level { source: source.to_owned(), w, h, live }
            } else {
                return Err("live needs x or level".into());
            };
            l1.set(&follow);
            let node = el().key(key.clone()).inert();
            let node = match &follow {
                Follow::X { live, .. } => node.live_offset(live),
                Follow::Level { w, h, live, .. } => node.pin(Some(0.0), Some(0.0), None, None).size(*w, *h).live_rect(live),
                Follow::Text { .. } => node,
            };
            let node = node.children(std::mem::take(&mut a.children));
            l1.nodes.borrow_mut().insert(key, follow);
            Ok(node)
        }))
        .node(HostNode::new("liveText", move |a| {
            let key = a.string("key").ok_or("liveText needs a key")?.to_owned();
            let live = match l2.nodes.borrow().get(&key) {
                Some(Follow::Text { live }) => live.clone(),
                _ => LiveText::new(model::position(0)),
            };
            let style = text_style(&text_theme, a.string("style").unwrap_or("base"));
            let node = text(model::position(l2.tick.get() as u64)).key(key.clone()).live_text(&live).font(style);
            let node = match a.color("color") {
                Some(c) => node.color(c),
                None => node,
            };
            l2.nodes.borrow_mut().insert(key, Follow::Text { live });
            Ok(node)
        }))
        .node(HostNode::new("lines", |a| lines(a)))
        .node(HostNode::new("drag", move |a| {
            let key = a.string("key").ok_or("drag needs a key")?.to_owned();
            let path = a.string("path").ok_or("drag needs a path")?.to_owned();
            let (value, min, max, length) = (a.number("value").unwrap_or(0.0), a.number("min").unwrap_or(0.0), a.number("max").unwrap_or(1.0), a.number("length").unwrap_or(1.0) as f32);
            let live = l3.drags.borrow_mut().entry(key.clone()).or_insert_with(|| Live::new([0.0; 2])).clone();
            live.snap([0.0, 0.0]);
            let top = (1.0 - ((value - min) / (max - min)).clamp(0.0, 1.0) as f32) * length;
            let moved = live.clone();
            Ok(el().key(key).live_offset(&live).children(std::mem::take(&mut a.children)).on_drag(move |s: &mut App, d: &mut Drag, _tx: &mut Tx| {
                let dy = d.dy.clamp(-top, length - top);
                match d.phase {
                    Phase::Down => {}
                    Phase::Move => _ = moved.snap([0.0, dy]),
                    Phase::Up => {
                        let fraction = 1.0 - (top + dy) / length;
                        s.requests.borrow_mut().push((path.clone(), Data::Number(min + (max - min) * fraction as f64)));
                        s.apply_requests();
                    }
                }
            }))
        }))
}

/// `lines`: {x1, y1, x2, y2, ...}, x scaled by `scaleX`, at least a point long.
fn lines(a: &mut NodeArgs) -> Result<El, String> {
    let scale = a.number("scaleX").unwrap_or(1.0) as f32;
    let width = a.number("width").unwrap_or(1.0) as f32;
    let colour = a.color("color").ok_or("lines needs a color")?;
    let Some(Data::List(points)) = a.data("points") else { return Err("lines needs points".into()) };
    let n = |d: &Data| d.as_f64().unwrap_or(0.0) as f32;
    let mut shape = Shape::new();
    for p in points.chunks_exact(4) {
        let (x1, y1, x2, y2) = (n(&p[0]) * scale, n(&p[1]), n(&p[2]) * scale, n(&p[3]));
        let x2 = if y1 == y2 { x2.max(x1 + 1.0) } else { x2 };
        shape = shape.line([x1, y1], [x2, y2], width, LineCap::Butt, colour);
    }
    Ok(canvas(shape).inert())
}

fn song_data(song: &Song, playing: bool) -> Data {
    let map = |pairs: Vec<(&str, Data)>| Data::Map(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect());
    let int = |n: u64| Data::Integer(n as i64);
    let tracks = song.tracks.iter().map(|t| {
        let clips = t.clips.iter().map(|c| {
            let notes = c.notes.iter().map(|n| map(vec![("start", int(n.start)), ("length", int(n.length)), ("pitch", int(n.pitch.into())), ("velocity", int(n.velocity.into()))])).collect();
            map(vec![("name", Data::String(c.name.clone())), ("start", int(c.start)), ("length", int(c.length)), ("notes", Data::List(notes))])
        });
        map(vec![
            ("name", Data::String(t.name.clone())),
            ("colour", int(t.colour.into())),
            ("mute", Data::Bool(t.mute)),
            ("solo", Data::Bool(t.solo)),
            ("armed", Data::Bool(t.armed)),
            ("gain_db", Data::Number(t.gain_db.into())),
            ("clips", Data::List(clips.collect())),
        ])
    });
    map(vec![
        ("transport", map(vec![("playing", Data::Bool(playing))])),
        ("view", map(vec![("zoom", Data::Number(model::ACROSS.into())), ("page", int(0))])),
        ("tracks", Data::List(tracks.collect())),
        ("master", map(vec![("gain_db", Data::Number(0.0))])),
    ])
}

impl App {
    /// Applies what the tool asked for, and sends each changed field back.
    fn apply_requests(&mut self) {
        let requests = std::mem::take(&mut *self.requests.borrow_mut());
        for (path, value) in requests {
            if self.apply(&path, &value) {
                self.log.push(format!("{path}={value:?}"));
                self.applied.set(self.applied.get() + 1);
                self.script.call("set", [Data::String(path), value]).ok();
            }
        }
    }

    /// Whether the window takes the change: only fields the song has, of the right type.
    fn apply(&mut self, path: &str, value: &Data) -> bool {
        let parts: Vec<&str> = path.split('/').collect();
        match (parts.as_slice(), value) {
            (["transport", "playing"], Data::Bool(on)) => {
                self.playing = *on;
                true
            }
            (["master", "gain_db"], v) => v.as_f64().is_some(),
            (["tracks", n, field], v) => {
                let Some(track) = n.parse::<usize>().ok().and_then(|n| self.song.tracks.get_mut(n.wrapping_sub(1))) else { return false };
                match (*field, v) {
                    ("mute", Data::Bool(b)) => track.mute = *b,
                    ("solo", Data::Bool(b)) => track.solo = *b,
                    ("armed", Data::Bool(b)) => track.armed = *b,
                    ("gain_db", v) if v.as_f64().is_some() => track.gain_db = v.as_f64().unwrap_or(0.0) as f32,
                    _ => return false,
                }
                true
            }
            _ => false,
        }
    }

    /// Tells the tool what the window changed itself.
    fn set(&self, path: String, value: Data) {
        self.applied.set(self.applied.get() + 1);
        self.script.call("set", [Data::String(path), value]).ok();
    }
}

fn init() -> App {
    let args = rune::extra_args();
    let click = args.windows(2).any(|w| w[0] == "--scenario" && w[1] == "click");
    let options = Options::parse(args.iter().cloned());
    let song = model::song(options.tracks, options.clips);
    let theme: Value = serde_json::from_str(&std::fs::read_to_string(THEME).expect("theme.json")).expect("theme.json is JSON");
    let lives = Rc::new(Lives::default());
    *lives.levels.borrow_mut() = vec![[0.0; 2]; options.tracks + 1];
    let requests = Rc::new(RefCell::new(Vec::new()));
    let sources = Sources::from_dir(PROJECT).expect("project folder");
    let script = Script::with_options("song", sources, script_options(&theme, &lives, &requests));
    if let Err(e) = script.watch(PROJECT) {
        eprintln!("not watching {PROJECT}: {e}");
    }
    let playing = options.scenario.playing();
    script.call("load", [song_data(&song, playing)]).ok();
    App {
        script,
        options,
        click,
        end: song.end(),
        song,
        playing,
        zoom: model::ACROSS,
        page: 0,
        edits: 0,
        frames: 0,
        last: f64::NEG_INFINITY,
        clicks: 0,
        lives,
        requests,
        applied: Cell::new(0),
        shown: Cell::new(0),
        pending: Cell::new(None),
        latencies: RefCell::new(Vec::new()),
        log: Vec::new(),
    }
}

/// Which clip edit number `n` moves, as `model::apply_edit` picks it.
fn edited(song: &Song, n: u64) -> Option<(usize, usize)> {
    let clips: usize = song.tracks.iter().map(|t| t.clips.len()).sum();
    let mut index = (n as usize * 7) % clips.max(1);
    for (t, track) in song.tracks.iter().enumerate() {
        if index < track.clips.len() {
            return Some((t, index));
        }
        index -= track.clips.len();
    }
    None
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() { 0.0 } else { sorted[((sorted.len() - 1) as f64 * p).round() as usize] }
}

fn poll(s: &mut App, seconds: f64, tx: &mut Tx) -> bool {
    const INTERVAL: f64 = 1.0 / 60.0;
    let before = s.applied.get();
    let ran = s.script.poll(seconds, tx);
    let still = s.options.scenario == Scenario::Idle && !s.click;
    if !still && seconds - s.last < INTERVAL * 0.95 {
        tx.wake_after(Duration::from_secs_f64(INTERVAL - (seconds - s.last)));
        return ran;
    }
    s.last = seconds;
    s.frames += 1;
    let m = model::moment(s.options.scenario, seconds, s.end);
    if m.zoom != s.zoom {
        s.zoom = m.zoom;
        s.set("view/zoom".into(), Data::Number(m.zoom.into()));
    }
    let page = (m.scroll_x / PAGE).floor() as i64;
    if page != s.page {
        s.page = page;
        s.set("view/page".into(), Data::Integer(page));
    }
    if s.options.scenario == Scenario::Edit {
        while s.edits < model::edits(seconds) {
            let at = edited(&s.song, s.edits);
            model::apply_edit(&mut s.song, s.edits);
            if let Some((t, c)) = at {
                let start = s.song.tracks[t].clips[c].start;
                s.set(format!("tracks/{}/clips/{}/start", t + 1, c + 1), Data::Integer(start as i64));
            }
            s.edits += 1;
        }
    }
    // Every 250 ms a solo flips, as a press on `solo-<n>` asks: the time until it shows is measured.
    if s.click && seconds >= s.clicks as f64 * 0.25 + 0.5 {
        let n = s.clicks as usize % s.song.tracks.len();
        let solo = s.song.tracks[n].solo;
        s.pending.set(Some((Instant::now(), s.applied.get() + 1)));
        s.requests.borrow_mut().push((format!("tracks/{}/solo", n + 1), Data::Bool(!solo)));
        s.apply_requests();
        s.clicks += 1;
    }
    s.lives.tick.set(if s.playing { m.playhead as f32 } else { 0.0 });
    s.lives.scroll.set(m.scroll_x);
    if s.playing {
        let levels: Vec<[f32; 2]> = (0..=s.song.tracks.len()).map(|i| [model::meter(i, seconds), model::meter(i, seconds + 0.05)]).collect();
        *s.lives.levels.borrow_mut() = levels;
    }
    s.lives.update();
    match s.options.seconds {
        Some(limit) if seconds >= limit.as_secs_f64() => {
            let mut lat = s.latencies.borrow().clone();
            lat.sort_by(f64::total_cmp);
            let scenario = if s.click { "click" } else { s.options.scenario.name() };
            println!(
                "{{\"app\":\"luau\",\"scenario\":\"{scenario}\",\"tracks\":{},\"clips\":{},\"seconds\":{seconds:.2},\"frames\":{},\"view_renders\":{},\"click_ms_p50\":{:.2},\"click_ms_p95\":{:.2},\"bytes\":0}}",
                s.options.tracks,
                s.options.clips,
                s.frames,
                s.script.metrics().renders,
                percentile(&lat, 0.5),
                percentile(&lat, 0.95),
            );
            tx.quit();
        }
        Some(limit) if still => tx.wake_after(limit.saturating_sub(Duration::from_secs_f64(seconds))),
        _ if !still => tx.wake_after(Duration::from_millis(16)),
        _ => {}
    }
    tx.instant();
    ran || s.applied.get() != before
}

fn view(s: &App, cx: &mut Cx<App>) -> El {
    s.shown.set(s.applied.get());
    s.script.view(cx)
}

fn setup(s: &mut App, waker: Waker) {
    s.script.set_waker(waker);
}

fn main() {
    rune::App::new(AppSpec { setup: Some(setup), poll: Some(poll), ..AppSpec::new("Luau DAW spike", WINDOW, init, view) })
        .on_frame_drawn(|rt| {
            let s = &rt.state;
            if let Some((at, change)) = s.pending.get()
                && s.shown.get() >= change
            {
                s.latencies.borrow_mut().push(at.elapsed().as_secs_f64() * 1e3);
                s.pending.set(None);
            }
        })
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    #[test]
    fn a_press_on_solo_asks_the_window_and_lights_the_toggle() {
        let mut rt = Runtime::new(init(), view, WINDOW, Env { system_dark: true, scale: 2.0 });
        rt.frame(0.0);
        let (x, y) = rt.find("solo-1").expect("solo-1 is on screen");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        // Colours spring to their new value, so let the fade settle.
        for _ in 0..60 {
            rt.frame(1.0 / 60.0);
        }
        assert_eq!(rt.state.log, ["tracks/2/solo=Bool(true)"]);
        assert!(rt.state.song.tracks[1].solo);
        let solo = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("solo-1")).expect("solo-1");
        assert_eq!(solo.fill, colour("#7ee0a0"));
    }

    #[test]
    fn a_drag_on_a_fader_sets_its_gain_on_release() {
        let mut rt = Runtime::new(init(), view, WINDOW, Env { system_dark: true, scale: 2.0 });
        rt.frame(0.0);
        let before = rt.state.song.tracks[0].gain_db;
        let (x, y) = rt.find("fader-0-thumb").expect("fader-0-thumb is on screen");
        rt.pointer_down(x, y);
        rt.pointer_move(x, y + 30.0);
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.song.tracks[0].gain_db, before, "no change while held");
        rt.pointer_up(x, y + 30.0);
        rt.frame(1.0 / 60.0);
        assert!(rt.state.song.tracks[0].gain_db < before - 5.0);
    }
}
