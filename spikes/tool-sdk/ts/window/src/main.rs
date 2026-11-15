//! The window for a tool Bun runs. It owns the records, sends them and every change to Bun, and
//! draws the tree Bun sends back. What moves every frame (playhead, scroll, meters, time, a drag)
//! is a `Live` value set here, never a round trip.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use daw_model::{self as model, ACROSS, BEAT, Moment, Options, Scenario, WINDOW};
use rune::{AppSpec, Color, Cx, El, LineCap, Live, LiveText, Phase, ScrollSpec, Shape, TextStyle, Tx, Waker, canvas, col, el, memo, row, text};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const PROJECT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../project");
/// As the tool's `PAGE`: the timeline is built a page each side of the one in view.
const PAGE: f32 = 1200.0;

// The tree as `sdk.ts` sends it.

#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
enum Node {
    Row(BoxNode),
    Col(BoxNode),
    #[serde(rename = "box")]
    Plain(BoxNode),
    Text(TextNode),
    Shapes(ShapesNode),
    Memo { key: String },
}

#[derive(Deserialize, Clone, Copy)]
#[serde(untagged)]
enum Size {
    Points(f32),
    Full(Full),
}
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Full {
    Full,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BoxNode {
    key: Option<String>,
    w: Option<Size>,
    h: Option<Size>,
    grow: Option<f32>,
    pad: Option<u32>,
    px: Option<u32>,
    py: Option<u32>,
    gap: Option<u32>,
    align: Option<String>,
    justify: Option<String>,
    at: Option<[f32; 2]>,
    inset: Option<[Option<f32>; 4]>,
    #[serde(default)]
    clip: bool,
    #[serde(default)]
    scroll: bool,
    #[serde(default)]
    inert: bool,
    fill: Option<String>,
    border: Option<String>,
    border_sides: Option<[f32; 4]>,
    radius: Option<String>,
    shadow: Option<String>,
    opacity: Option<f32>,
    x: Option<LiveX>,
    level: Option<String>,
    #[serde(default)]
    press: bool,
    drag: Option<DragSpec>,
    #[serde(default)]
    children: Vec<Node>,
}

#[derive(Deserialize, Clone)]
struct LiveX {
    live: String,
    #[serde(default = "one")]
    scale: f32,
    #[serde(default)]
    offset: f32,
}
fn one() -> f32 {
    1.0
}

#[derive(Deserialize, Clone, Copy)]
struct DragSpec {
    axis: Axis,
    min: f32,
    max: f32,
}
#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Axis {
    X,
    Y,
}

#[derive(Deserialize)]
struct TextNode {
    key: Option<String>,
    at: Option<[f32; 2]>,
    style: Option<String>,
    color: Option<String>,
    w: Option<f32>,
    #[serde(default)]
    truncate: bool,
    live: Option<String>,
    text: String,
}

#[derive(Deserialize)]
struct ShapesNode {
    key: Option<String>,
    at: Option<[f32; 2]>,
    w: f32,
    h: f32,
    units: Option<[f32; 2]>,
    lines: Vec<Lines>,
}
#[derive(Deserialize)]
struct Lines {
    color: String,
    width: f32,
    points: Vec<f32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum FromTool {
    Tree(Tree),
    Change(Vec<Op>),
    Error(String),
}
#[derive(Deserialize)]
struct Tree {
    root: Node,
    memos: HashMap<String, Fresh>,
    drop: Vec<String>,
}
#[derive(Deserialize)]
struct Fresh {
    v: u64,
    node: Node,
}
type Op = (Vec<Step>, Value);
#[derive(Deserialize, Serialize, Clone)]
#[serde(untagged)]
enum Step {
    Index(usize),
    Field(String),
}

// Design tokens, read from theme.json as the window opens.

#[derive(Deserialize)]
struct Theme {
    color: HashMap<String, String>,
    text: HashMap<String, TextToken>,
    space: HashMap<String, f32>,
    radius: HashMap<String, f32>,
    shadow: HashMap<String, ShadowToken>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TextToken {
    size: f32,
    line_height: f32,
    tracking: f32,
    weight: f32,
    #[serde(default)]
    tabular: bool,
}
#[derive(Deserialize)]
struct ShadowToken {
    y: f32,
    blur: f32,
    color: String,
}

const MISSING: Color = Color { r: 1.0, g: 0.0, b: 1.0, a: 1.0 };

impl Theme {
    fn load() -> Theme {
        let path = format!("{PROJECT}/theme.json");
        let json = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        serde_json::from_str(&json).unwrap_or_else(|e| panic!("{path}: {e}"))
    }
    /// A token, `token/percent`, or `#rrggbb[aa]`.
    fn color(&self, name: &str) -> Color {
        let (token, percent) = match name.split_once('/') {
            Some((token, percent)) => (token, percent.parse::<f32>().ok()),
            None => (name, None),
        };
        let hex = if token.starts_with('#') { Some(token) } else { self.color.get(token).map(String::as_str) };
        let colour = hex.and_then(rune::color::hex).unwrap_or(MISSING);
        percent.map_or(colour, |p| colour.alpha(p / 100.0))
    }
    fn text(&self, name: &str) -> TextStyle {
        let Some(t) = self.text.get(name) else { return TextStyle::new(12.0, 400.0) };
        let style = TextStyle::new(t.size, t.weight).lh(t.line_height).tracking(t.tracking);
        if t.tabular { style.features("'tnum' 1") } else { style }
    }
    fn space(&self, step: u32) -> f32 {
        self.space.get(&step.to_string()).copied().unwrap_or(0.0)
    }
}

/// The live values elements bind by key, kept across trees so a binding survives a rebuild.
struct Lives {
    x: RefCell<HashMap<String, (Live<[f32; 2]>, LiveX)>>,
    level: RefCell<HashMap<String, (Live<[f32; 4]>, String, [f32; 2])>>,
    drag: RefCell<HashMap<String, Live<[f32; 2]>>>,
    time: LiveText,
}

impl Lives {
    fn update(&self, m: &Moment, seconds: f64) {
        let value = |name: &str| match name {
            "playhead" => m.playhead as f32,
            "scroll" => m.scroll_x,
            meter => (meter.strip_prefix("meter-").and_then(|i| i.parse().ok()))
                .filter(|_| m.playing)
                .map_or(0.0, |i| model::meter(i, seconds)),
        };
        for (live, spec) in self.x.borrow().values() {
            live.set([spec.offset + spec.scale * value(&spec.live), 0.0]);
        }
        for (live, name, [w, h]) in self.level.borrow().values() {
            let filled = value(name).clamp(0.0, 1.0) * h;
            live.set([0.0, h - filled, *w, filled]);
        }
        self.time.set(model::position(m.playhead));
    }
}

/// The Bun process that runs the tool.
struct Tool {
    stdin: ChildStdin,
    messages: Receiver<FromTool>,
    waker: Arc<Mutex<Option<Waker>>>,
    bytes_in: Arc<AtomicU64>,
    bytes_out: u64,
    _child: Child,
}

impl Tool {
    /// Starts Bun with the tool and waits for its first tree, so the first frame has one.
    fn start(state: &Value) -> (Tool, Tree) {
        let mut child = Command::new("bun")
            .args(["../host.ts", "song.tsx"])
            .current_dir(PROJECT)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("bun on the PATH");
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let bytes_in = Arc::new(AtomicU64::new(0));
        let mut tool = Tool {
            stdin: child.stdin.take().unwrap(),
            messages: channel().1,
            waker: Arc::default(),
            bytes_in: bytes_in.clone(),
            bytes_out: 0,
            _child: child,
        };
        tool.send(&json!({ "init": state }));
        let first = loop {
            let line = lines.next().expect("the tool quit before drawing").unwrap();
            bytes_in.fetch_add(line.len() as u64 + 1, Ordering::Relaxed);
            match serde_json::from_str(&line) {
                Ok(FromTool::Tree(tree)) => break tree,
                Ok(FromTool::Error(e)) => panic!("the tool failed to draw: {e}"),
                Ok(FromTool::Change(_)) => {}
                Err(e) => panic!("not a message: {e}"),
            }
        };
        let (send, receive) = channel();
        tool.messages = receive;
        let waker = tool.waker.clone();
        std::thread::spawn(move || {
            for line in lines.map_while(Result::ok) {
                bytes_in.fetch_add(line.len() as u64 + 1, Ordering::Relaxed);
                match serde_json::from_str(&line) {
                    Ok(message) => {
                        if send.send(message).is_err() {
                            break;
                        }
                        if let Some(wake) = waker.lock().unwrap().as_ref() {
                            wake();
                        }
                    }
                    Err(e) => eprintln!("not a message: {e}"),
                }
            }
        });
        (tool, first)
    }

    fn send(&mut self, message: &Value) {
        let mut line = message.to_string();
        line.push('\n');
        self.bytes_out += line.len() as u64;
        let _ = self.stdin.write_all(line.as_bytes());
    }
}

struct State {
    options: Options,
    click: bool,
    /// The records and what the tool is shown of the view: `{song, playing, view: {zoom, page}}`.
    state: Value,
    end: u64,
    tool: Tool,
    theme: Theme,
    root: Rc<Node>,
    memos: HashMap<String, (u64, Rc<Node>)>,
    lives: Lives,
    zoom: f32,
    page: i64,
    edits: u64,
    frames: u64,
    trees: u64,
    builds: Cell<u64>,
    /// When the last frame's update was: held to 60 a second, as in `rune-daw`.
    last: f64,
    next_click: f64,
    clicks: usize,
    pressed: Option<Instant>,
    click_ms: Vec<f64>,
}

impl State {
    fn press(&mut self, key: &str) {
        self.pressed.get_or_insert_with(Instant::now);
        self.tool.send(&json!({ "press": key }));
    }

    /// Applies `ops` to the records, and tells the tool unless they are its own.
    fn set(&mut self, ops: Vec<Op>, echo: bool) {
        for (path, value) in &ops {
            let mut at = Some(&mut self.state);
            for step in path {
                at = at.and_then(|v| match step {
                    Step::Index(i) => v.get_mut(*i),
                    Step::Field(f) => v.get_mut(f.as_str()),
                });
            }
            if let Some(at) = at {
                *at = value.clone();
            }
        }
        if echo {
            self.tool.send(&json!({ "set": ops }));
        }
    }

    /// Handles what the tool sent; true when a new tree came.
    fn drain(&mut self) -> bool {
        let mut drawn = false;
        while let Ok(message) = self.tool.messages.try_recv() {
            match message {
                FromTool::Tree(tree) => {
                    for key in tree.drop {
                        self.memos.remove(&key);
                    }
                    for (key, fresh) in tree.memos {
                        self.memos.insert(key, (fresh.v, Rc::new(fresh.node)));
                    }
                    self.root = Rc::new(tree.root);
                    self.trees += 1;
                    for live in self.lives.drag.borrow().values() {
                        live.set([0.0, 0.0]);
                    }
                    if let Some(at) = self.pressed.take() {
                        self.click_ms.push(at.elapsed().as_secs_f64() * 1000.0);
                    }
                    drawn = true;
                }
                FromTool::Change(ops) => self.set(ops, false),
                FromTool::Error(e) => eprintln!("tool: {e}"),
            }
        }
        drawn
    }
}

fn song_json(song: &model::Song) -> Value {
    let tracks: Vec<Value> = (song.tracks.iter())
        .map(|t| {
            let clips: Vec<Value> = (t.clips.iter())
                .map(|c| {
                    let notes: Vec<Value> = (c.notes.iter())
                        .map(|n| json!({ "start": n.start, "length": n.length, "pitch": n.pitch, "velocity": n.velocity }))
                        .collect();
                    json!({ "name": c.name, "start": c.start, "length": c.length, "notes": notes })
                })
                .collect();
            json!({ "name": t.name, "colour": format!("#{:06x}", t.colour), "clips": clips,
                    "mute": t.mute, "solo": t.solo, "armed": t.armed, "gain_db": t.gain_db })
        })
        .collect();
    json!({ "tracks": tracks })
}

/// `model::apply_edit` on the records: edit `n` moves one clip a beat later.
fn edit_op(state: &Value, n: u64) -> Option<Op> {
    let tracks = state["song"]["tracks"].as_array()?;
    let total: usize = tracks.iter().map(|t| t["clips"].as_array().map_or(0, Vec::len)).sum();
    let mut index = (n as usize * 7).checked_rem(total)?;
    for (t, track) in tracks.iter().enumerate() {
        let clips = track["clips"].as_array()?;
        if index < clips.len() {
            let start = clips[index]["start"].as_u64()? + BEAT;
            let path = ["song", "tracks"].map(|f| Step::Field(f.into())).into_iter()
                .chain([Step::Index(t), Step::Field("clips".into()), Step::Index(index), Step::Field("start".into())]);
            return Some((path.collect(), json!(start)));
        }
        index -= clips.len();
    }
    None
}

fn init() -> State {
    let args = rune::extra_args();
    let options = Options::parse(args.iter().cloned());
    let click = args.windows(2).any(|w| w[0] == "--scenario" && w[1] == "click");
    let song = model::song(options.tracks, options.clips);
    let state = json!({
        "song": song_json(&song),
        "playing": options.scenario.playing(),
        "view": { "zoom": ACROSS, "page": 0 },
    });
    let (tool, first) = Tool::start(&state);
    let memos = first.memos.into_iter().map(|(k, f)| (k, (f.v, Rc::new(f.node)))).collect();
    State {
        options,
        click,
        state,
        end: song.end(),
        tool,
        theme: Theme::load(),
        root: Rc::new(first.root),
        memos,
        lives: Lives { x: RefCell::default(), level: RefCell::default(), drag: RefCell::default(), time: LiveText::new(model::position(0)) },
        zoom: ACROSS,
        page: 0,
        edits: 0,
        frames: 0,
        trees: 1,
        builds: Cell::new(0),
        last: f64::NEG_INFINITY,
        next_click: 1.0,
        clicks: 0,
        pressed: None,
        click_ms: Vec::new(),
    }
}

fn setup(s: &mut State, waker: Waker) {
    *s.tool.waker.lock().unwrap() = Some(waker);
}

fn percentile(samples: &[f64], p: f64) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted.get(((sorted.len() as f64 - 1.0) * p).round() as usize).copied().unwrap_or(0.0)
}

fn poll(s: &mut State, seconds: f64, tx: &mut Tx) -> bool {
    const INTERVAL: f64 = 1.0 / 60.0;
    let drawn = s.drain();
    let idle = s.options.scenario == Scenario::Idle && !s.click;
    if !idle && seconds - s.last < INTERVAL * 0.95 {
        tx.wake_after(Duration::from_secs_f64(INTERVAL - (seconds - s.last)));
        return drawn;
    }
    s.last = seconds;
    s.frames += 1;
    let m = model::moment(s.options.scenario, seconds, s.end);
    let mut ops: Vec<Op> = Vec::new();
    let view = |field: &str| vec![Step::Field("view".into()), Step::Field(field.into())];
    if m.zoom != s.zoom {
        s.zoom = m.zoom;
        ops.push((view("zoom"), json!(m.zoom)));
    }
    let page = (m.scroll_x / PAGE).floor() as i64;
    if page != s.page {
        s.page = page;
        ops.push((view("page"), json!(page)));
    }
    if s.options.scenario == Scenario::Edit {
        while s.edits < model::edits(seconds) {
            ops.extend(edit_op(&s.state, s.edits));
            s.edits += 1;
        }
    }
    if !ops.is_empty() {
        s.set(ops, true);
    }
    if s.click && seconds >= s.next_click {
        s.next_click += 0.25;
        let key = format!("solo-{}", s.clicks % s.options.tracks.max(1));
        s.clicks += 1;
        s.press(&key);
    }
    s.lives.update(&m, seconds);
    match s.options.seconds {
        Some(limit) if seconds >= limit.as_secs_f64() => {
            println!(
                "{{\"app\":\"ts\",\"scenario\":\"{}\",\"tracks\":{},\"clips\":{},\"seconds\":{seconds:.2},\"frames\":{},\"view_renders\":{},\"window_builds\":{},\"click_ms_p50\":{:.2},\"click_ms_p95\":{:.2},\"bytes\":{}}}",
                if s.click { "click" } else { s.options.scenario.name() },
                s.options.tracks,
                s.options.clips,
                s.frames,
                s.trees,
                s.builds.get(),
                percentile(&s.click_ms, 0.5),
                percentile(&s.click_ms, 0.95),
                s.tool.bytes_out + s.tool.bytes_in.load(Ordering::Relaxed),
            );
            tx.quit();
        }
        Some(limit) if idle => tx.wake_after(limit.saturating_sub(Duration::from_secs_f64(seconds))),
        _ if !idle => tx.wake_after(Duration::from_millis(16)),
        _ => {}
    }
    tx.instant();
    drawn
}

fn view(s: &State, _: &mut Cx<State>) -> El {
    s.builds.set(s.builds.get() + 1);
    build(s, &s.root)
}

fn build(s: &State, node: &Node) -> El {
    match node {
        Node::Row(b) => boxed(s, row(), b),
        Node::Col(b) => boxed(s, col(), b),
        Node::Plain(b) => boxed(s, el(), b),
        Node::Text(t) => label(s, t),
        Node::Shapes(shapes) => drawing(s, shapes),
        Node::Memo { key } => match s.memos.get(key) {
            Some((version, node)) => memo(key.clone(), *version, || build(s, node)),
            None => el(),
        },
    }
}

fn boxed(s: &State, mut e: El, b: &BoxNode) -> El {
    let t = &s.theme;
    if let Some(key) = &b.key {
        e = e.key(key.clone());
    }
    e = match b.w {
        Some(Size::Points(w)) => e.w(w),
        Some(Size::Full(_)) => e.w_full(),
        None => e,
    };
    e = match b.h {
        Some(Size::Points(h)) => e.h(h),
        Some(Size::Full(_)) => e.h_full(),
        None => e,
    };
    if let Some(g) = b.grow {
        e = e.grow(g);
    }
    if let Some(p) = b.pad {
        e = e.padding(t.space(p));
    }
    if let Some(p) = b.px {
        e = e.px(t.space(p));
    }
    if let Some(p) = b.py {
        e = e.py(t.space(p));
    }
    if let Some(g) = b.gap {
        e = e.gap(t.space(g));
    }
    if b.align.as_deref() == Some("center") {
        e = e.items_center();
    }
    match b.justify.as_deref() {
        Some("center") => e = e.justify_center(),
        Some("between") => e = e.justify_between(),
        _ => {}
    }
    if let Some([x, y]) = b.at {
        e = e.absolute(x, y);
    }
    if let Some([left, top, right, bottom]) = b.inset {
        e = e.pin(left, top, right, bottom);
    }
    if b.clip {
        e = e.clip_soft(0.0);
    }
    if b.scroll {
        e = e.scroll(ScrollSpec { stick_bottom: false });
    }
    if b.inert {
        e = e.inert();
    }
    if let Some(c) = &b.fill {
        e = e.fill(t.color(c));
    }
    if let Some(c) = &b.border {
        e = match b.border_sides {
            Some(sides) => e.border_sides(sides, t.color(c)),
            None => e.border(1.0, t.color(c)),
        };
    }
    if let Some(r) = b.radius.as_ref().and_then(|r| t.radius.get(r)) {
        e = e.radius(*r);
    }
    if let Some(shadow) = b.shadow.as_ref().and_then(|name| t.shadow.get(name)) {
        e = e.shadow(shadow.y, shadow.blur, t.color(&shadow.color));
    }
    if let Some(o) = b.opacity {
        e = e.opacity(o);
    }
    if let Some(key) = &b.key {
        if let Some(spec) = &b.x {
            let mut x = s.lives.x.borrow_mut();
            let entry = x.entry(key.clone()).or_insert_with(|| (Live::new([0.0; 2]), spec.clone()));
            entry.1 = spec.clone();
            e = e.live_offset(&entry.0);
        }
        if let (Some(name), Some(Size::Points(w)), Some(Size::Points(h))) = (&b.level, b.w, b.h) {
            let mut level = s.lives.level.borrow_mut();
            let entry = level.entry(key.clone()).or_insert_with(|| (Live::new([0.0, h, w, 0.0]), name.clone(), [w, h]));
            (entry.1, entry.2) = (name.clone(), [w, h]);
            e = e.pin(Some(0.0), Some(0.0), None, None).live_rect(&entry.0);
        }
        if b.press {
            let key = key.clone();
            e = e.on_press(move |s: &mut State| s.press(&key));
        }
        if let Some(spec) = b.drag {
            let live = s.lives.drag.borrow_mut().entry(key.clone()).or_insert_with(|| Live::new([0.0; 2])).clone();
            let key = key.clone();
            e = e.live_offset(&live).on_drag(move |s: &mut State, d: &mut rune::Drag, _: &mut Tx| {
                let along = if spec.axis == Axis::X { d.dx } else { d.dy }.clamp(spec.min, spec.max);
                let moved = if spec.axis == Axis::X { [along, 0.0] } else { [0.0, along] };
                match d.phase {
                    Phase::Down => {}
                    Phase::Move => {
                        live.set(moved);
                    }
                    Phase::Up if d.cancelled() => {
                        live.set([0.0; 2]);
                    }
                    Phase::Up => {
                        live.set(moved);
                        s.tool.send(&json!({ "drag": { "key": key, "dx": moved[0], "dy": moved[1] } }));
                    }
                }
            });
        }
    }
    e.children(b.children.iter().map(|c| build(s, c)))
}

fn label(s: &State, t: &TextNode) -> El {
    let mut e = text(t.text.clone())
        .font(s.theme.text(t.style.as_deref().unwrap_or("sm")))
        .color(s.theme.color(t.color.as_deref().unwrap_or("text")));
    if let Some(key) = &t.key {
        e = e.key(key.clone());
    }
    if let Some(w) = t.w {
        e = e.w(w);
    }
    if t.truncate {
        e = e.truncate();
    }
    if let Some([x, y]) = t.at {
        e = e.absolute(x, y);
    }
    if t.live.as_deref() == Some("time") {
        e = e.live_text(&s.lives.time);
    }
    e
}

fn drawing(s: &State, d: &ShapesNode) -> El {
    let mut shape = Shape::new();
    for lines in &d.lines {
        let colour = s.theme.color(&lines.color);
        for seg in lines.points.chunks_exact(4) {
            shape = shape.line([seg[0], seg[1]], [seg[2], seg[3]], lines.width, LineCap::Butt, colour);
        }
    }
    if let Some([across, down]) = d.units {
        shape = shape.view_box(across, down);
    }
    let mut e = canvas(shape).size(d.w, d.h);
    if let Some(key) = &d.key {
        e = e.key(key.clone());
    }
    if let Some([x, y]) = d.at {
        e = e.absolute(x, y);
    }
    e
}

fn main() {
    rune::run(AppSpec::<State> {
        poll: Some(poll),
        setup: Some(setup),
        ..AppSpec::new("Tool SDK spike: TypeScript", WINDOW, init, view)
    });
}
