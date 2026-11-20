//! The transport in the title row: play, stop and record, where the piece plays, its tempo and
//! time signature, the switches of how it plays, and the level of what the device plays. What
//! moves as the piece plays is live, so playing builds nothing.

use std::cell::RefCell;
use std::collections::HashMap;

use anyhow::{Context as _, Result, anyhow, bail};
use rune::{Cursor, El, Icon, Key, KeyEvent, LiveText, TextEdit, Tx, el, icon, row, text};
use runtime::session::Drag;
use serde_json::{Value, json};
use sound_core::{
    Changes, FADER_TOP_DB, Peaks, Project, Scale, Tempo, TempoMap, Ticks, TimeSignature,
};
use sound_typescript::live::{Readout, TickReadout, text_of};

use super::State;
use super::keys::app_key_of;
use crate::controls::{Control, Target};

const TEMPO_STEP: &str = "Change the tempo";
const SIGNATURE_STEP: &str = "Change the time signature";
/// Points of a drag up per beat a minute faster; Shift is ten times finer.
const POINTS_PER_BPM: f64 = 2.0;
const SLOWEST: f64 = 10.0;
const FASTEST: f64 = 1000.0;
/// The tempo and the time signature say what plays at the playhead, so they live as it moves.
const POSITION_W: f32 = 44.0;
const TIME_W: f32 = 40.0;
const TEMPO_W: f32 = 32.0;
const SIGNATURE_W: f32 = 40.0;

pub(crate) struct Transport {
    position: LiveText,
    time: LiveText,
    tempo: LiveText,
    signature: LiveText,
    meter: Meter,
    /// The drag on the tempo, from its press: one undo step.
    dragging: Option<TempoDrag>,
    /// The readout the composer types into.
    typing: Option<Typed>,
    /// What its field says, by key, and the value it was made from.
    edits: RefCell<HashMap<&'static str, (TextEdit, String)>>,
    /// Whether the main tool has an action named `record`, which the Record button runs. The
    /// host says so once it knows the tools' actions.
    pub(crate) records: bool,
}

struct TempoDrag {
    drag: Drag,
    /// The tempo change it moves, the one in effect at the playhead when it was pressed.
    at: Ticks,
    /// Where it is, finer than it shows, and the travel it came from.
    bpm: f64,
    dy: f32,
    moved: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Typed {
    Tempo,
    Signature,
}

impl Typed {
    fn key(self) -> &'static str {
        match self {
            Typed::Tempo => "tempo-typed",
            Typed::Signature => "signature-typed",
        }
    }

    fn readout(self) -> &'static str {
        match self {
            Typed::Tempo => "tempo",
            Typed::Signature => "signature",
        }
    }
}

impl Transport {
    /// `output` is what the device plays, for the meter.
    pub(crate) fn new(output: Peaks) -> Self {
        Self {
            position: LiveText::new("1.1".to_string()),
            time: LiveText::new("0:00".to_string()),
            tempo: LiveText::new("120".to_string()),
            signature: LiveText::new("4 / 4".to_string()),
            meter: Meter::new(output),
            dragging: None,
            typing: None,
            edits: RefCell::default(),
            records: false,
        }
    }

    /// What the field `key` says now: `value` while nobody types in it, all of it chosen, so
    /// what is typed takes its place.
    fn typed(&self, key: &'static str, value: &str) -> TextEdit {
        let mut edits = self.edits.borrow_mut();
        let (edit, from) =
            (edits.entry(key)).or_insert_with(|| (TextEdit::new(value), value.to_string()));
        if !edit.focused {
            if from != value || edit.text != value {
                (*edit, *from) = (TextEdit::new(value), value.to_string());
            }
            (edit.anchor, edit.focus) = (0, edit.text.len());
        }
        edit.clone()
    }

    /// The field `key`, which Rune edits.
    fn typing(&mut self, key: &'static str, value: &str) -> &mut TextEdit {
        let entry = (self.edits.get_mut().entry(key))
            .or_insert_with(|| (TextEdit::new(value), value.to_string()));
        &mut entry.0
    }

    /// Whether one of its fields has the keys.
    pub(crate) fn has_keys(&self) -> bool {
        self.edits.borrow().values().any(|(edit, _)| edit.focused)
    }

    /// One frame `dt` seconds after the last, the piece at `playhead`: the readouts and the
    /// meter. Whether the meter still falls, and whether its clip light changed, which builds
    /// the view again.
    pub(crate) fn update(&mut self, project: &Project, playhead: Ticks, dt: f64) -> (bool, bool) {
        let clock = project.clock();
        let at = |readout| text_of(playhead.0 as f64, Some(Readout::Tick(readout)), Some(clock));
        self.position.set(at(TickReadout::Position));
        self.time.set(at(TickReadout::Time));
        let map = &project.project_file().tempo_map;
        self.tempo.set(bpm_text(map.change_at(playhead).bpm.bpm()));
        self.signature.set(signature_text(
            map.time_signatures().bar_at(playhead).signature,
        ));
        self.meter.read(dt as f32)
    }
}

fn bpm_text(bpm: f64) -> String {
    match bpm.fract() == 0.0 {
        true => format!("{bpm:.0}"),
        false => format!("{bpm:.1}"),
    }
}

fn signature_text(signature: TimeSignature) -> String {
    format!("{} / {}", signature.numerator(), signature.denominator())
}

impl State {
    pub(super) fn play_or_pause(&mut self) {
        let playing = self.playing;
        self.session.background(|project| match playing {
            true => project.engine().pause(),
            false => project.engine().play(),
        });
    }

    /// Stops and goes back to the start.
    fn stop(&mut self) {
        self.session.background(|project| project.engine().stop());
    }

    /// Runs the main tool's action `record`. HOST: the host runs a tool's action by name once
    /// it has them; until then there is none to run.
    fn record(&mut self) -> Result<()> {
        bail!("this tool has no action to record yet")
    }

    fn tempo_now(&self) -> (Ticks, f64) {
        let change = self
            .project()
            .project_file()
            .tempo_map
            .change_at(self.playhead);
        (change.tick, change.bpm.bpm())
    }

    /// The tempo the composer typed or an agent set: one undo step.
    fn set_tempo(&mut self, bpm: f64) {
        let (at, _) = self.tempo_now();
        let bpm = bpm.clamp(SLOWEST, FASTEST);
        self.session
            .edit(|project| project.commit(TEMPO_STEP, tempo_changed(project, at, bpm)));
    }

    fn drag_tempo(&mut self, d: &rune::Drag, tx: &mut Tx) {
        match d.phase {
            rune::Phase::Down => {
                let (at, bpm) = self.tempo_now();
                let drag = Drag::new(TEMPO_STEP);
                self.transport.dragging = Some(TempoDrag {
                    drag,
                    at,
                    bpm,
                    dy: 0.0,
                    moved: false,
                });
            }
            rune::Phase::Move => {
                let Some(dragging) = &mut self.transport.dragging else {
                    return;
                };
                let fine = tx.mods().shift;
                let per = POINTS_PER_BPM * if fine { 10.0 } else { 1.0 };
                dragging.bpm -= f64::from(d.dy - dragging.dy) / per;
                dragging.bpm = dragging.bpm.clamp(SLOWEST, FASTEST);
                (dragging.dy, dragging.moved) = (d.dy, true);
                let step = if fine { 10.0 } else { 1.0 };
                let (at, bpm) = (dragging.at, (dragging.bpm * step).round() / step);
                self.session.publish(&mut dragging.drag, |project, edit| {
                    project.publish(edit, tempo_changed(project, at, bpm))
                });
            }
            rune::Phase::Up => {
                let Some(dragging) = self.transport.dragging.take() else {
                    return;
                };
                match d.cancelled() {
                    true => self.session.cancel(dragging.drag),
                    false => self.session.finish(dragging.drag),
                }
                // A click that did not move types the tempo.
                if !dragging.moved && !d.cancelled() {
                    self.type_into(Typed::Tempo, tx);
                }
            }
        }
        self.handled();
    }

    /// The time signature in effect at the playhead becomes `beats / note`: one undo step.
    fn set_signature(&mut self, beats: u32, note: u32) -> Result<()> {
        let signature = TimeSignature::new(beats, note).map_err(|error| anyhow!("{error}"))?;
        let map = &self.project().project_file().tempo_map;
        let signatures = map.time_signatures();
        let run = signatures.change_at(self.playhead);
        let signatures = (signatures.with_signature_at(run.number, signature))
            .context("the time signature could not change there")?;
        let map = TempoMap::new(signatures, map.tempo_changes().to_vec())
            .map_err(|error| anyhow!("{error}"))?;
        let mut changes = Changes::new();
        changes.set_tempo_map(map);
        self.session
            .edit(|project| project.commit(SIGNATURE_STEP, changes));
        Ok(())
    }

    /// What the field of `typed` says lands: a number out of range goes to its nearest end,
    /// words that are no number leave it as it was.
    fn typed_in(&mut self, typed: Typed, tx: Option<&mut Tx>) {
        let words = self.transport.typing(typed.key(), "").text.clone();
        self.transport.typing = None;
        match typed {
            Typed::Tempo => {
                if let Ok(bpm) = words.trim().parse::<f64>() {
                    self.set_tempo(bpm);
                }
            }
            Typed::Signature => {
                if let Some((beats, note)) = parse_signature(&words) {
                    self.set_signature(beats, note).ok();
                }
            }
        }
        if let Some(tx) = tx {
            tx.focus(typed.readout(), true);
        }
    }

    /// The field of `typed` takes the place of its readout, and the keys.
    fn type_into(&mut self, typed: Typed, tx: &mut Tx) {
        self.transport.typing = Some(typed);
        tx.focus(typed.key(), true);
    }

    fn toggle(&mut self, act: Act) {
        let now = self.session.transport().clone();
        match act {
            Act::Click => self.session.set_click(!now.click),
            Act::Keys => self.session.set_computer_keys(!now.keys),
            Act::Automation => self.session.set_writes_automation(!now.automation),
            _ => {}
        }
    }

    /// Whether a recorder records now: the session hears it from the recorders.
    fn records_now(&self) -> bool {
        self.session.recording()
    }
}

/// The changes that give the tempo change at `at` the tempo `bpm`.
fn tempo_changed(project: &Project, at: Ticks, bpm: f64) -> Changes {
    let map = &project.project_file().tempo_map;
    let mut changes = Changes::new();
    if let Some(map) = (Tempo::from_bpm(bpm).ok()).and_then(|bpm| map.with_tempo_at(at, bpm)) {
        changes.set_tempo_map(map);
    }
    changes
}

/// `3/4`, `3 / 4` or `3 4`.
fn parse_signature(words: &str) -> Option<(u32, u32)> {
    let mut numbers = words
        .split(|c: char| c == '/' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(str::parse::<u32>);
    match (numbers.next(), numbers.next(), numbers.next()) {
        (Some(Ok(beats)), Some(Ok(note)), None) => Some((beats, note)),
        _ => None,
    }
}

/// What runs an action of a control of the transport.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
    Play,
    Stop,
    Record,
    Tempo,
    Signature,
    Click,
    Keys,
    Automation,
}

impl Act {
    pub(crate) fn run(self, state: &mut State, value: Option<Value>) -> Result<()> {
        match self {
            Act::Play => state.play_or_pause(),
            Act::Stop => state.stop(),
            Act::Record => state.record()?,
            Act::Tempo => {
                let bpm = value.as_ref().and_then(Value::as_f64);
                state.set_tempo(bpm.context("give the tempo as a number, such as 100")?);
            }
            Act::Signature => {
                let words = match &value {
                    Some(Value::String(words)) => words.clone(),
                    _ => String::new(),
                };
                let (beats, note) = parse_signature(&words)
                    .context("give the time signature as words, such as \"3/4\"")?;
                state.set_signature(beats, note)?;
            }
            Act::Click | Act::Keys | Act::Automation => state.toggle(self),
        }
        Ok(())
    }
}

/// The controls of the transport, for `--tree` and `--act`.
pub(crate) fn controls(state: &State) -> Vec<Control> {
    let project = state.project();
    let map = &project.project_file().tempo_map;
    let switches = state.session.transport();
    let control = |key: &str, label: &str, role: &str, act: Act, actions: &[&str]| Control {
        key: key.into(),
        label: label.into(),
        role: Some(role.into()),
        value: None,
        checked: None,
        expanded: None,
        actions: actions.iter().map(|action| action.to_string()).collect(),
        target: Target::Chrome(super::Act::Transport(act)),
    };
    let position = control("position", "Position", "timer", Act::Play, &[]);
    let mut controls = vec![
        control(
            "play",
            play_label(state.playing),
            "button",
            Act::Play,
            &["press"],
        ),
        control("stop", "Stop", "button", Act::Stop, &["press"]),
    ];
    if state.transport.records {
        controls.push(Control {
            checked: Some(state.records_now()),
            ..control("record", "Record", "button", Act::Record, &["press"])
        });
    }
    let clock = Some(project.clock());
    let tick = state.playhead.0 as f64;
    let at = |readout| json!(text_of(tick, Some(Readout::Tick(readout)), clock));
    controls.extend([
        Control {
            value: Some(at(TickReadout::Position)),
            ..position
        },
        Control {
            value: Some(at(TickReadout::Time)),
            ..control("time", "Time", "timer", Act::Play, &[])
        },
        Control {
            value: Some(json!(map.change_at(state.playhead).bpm.bpm())),
            ..control("tempo", "Tempo", "spinbutton", Act::Tempo, &["set"])
        },
        Control {
            value: Some(json!(signature_text(
                map.time_signatures().bar_at(state.playhead).signature
            ))),
            ..control(
                "signature",
                "Time signature",
                "spinbutton",
                Act::Signature,
                &["set"],
            )
        },
    ]);
    for (key, label, act, on) in SWITCHES {
        controls.push(Control {
            checked: Some(on(switches)),
            ..control(key, label, "switch", act, &["toggle"])
        });
    }
    controls.push(Control {
        value: Some(state.transport.meter.said()),
        ..control("level", "Output level", "meter", Act::Play, &[])
    });
    controls
}

fn play_label(playing: bool) -> &'static str {
    if playing { "Pause" } else { "Play" }
}

type Switch = (
    &'static str,
    &'static str,
    Act,
    fn(&runtime::session::TransportState) -> bool,
);

/// The switches of how the piece plays, in their order in the pill.
const SWITCHES: [Switch; 3] = [
    ("computer-keys", "Computer keys", Act::Keys, |on| on.keys),
    (
        "write-automation",
        "Write automation",
        Act::Automation,
        |on| on.automation,
    ),
    ("click", "Click", Act::Click, |on| on.click),
];

/// The pill of the transport.
pub(super) fn view(state: &State) -> El {
    let theme = &state.theme;
    let transport = &state.transport;
    let playing = state.playing;
    let buttons = row()
        .gap(theme.space(1))
        .items_center()
        .child(
            round(
                state,
                "play",
                play_label(playing),
                if playing { &PAUSE } else { &PLAY },
            )
            .ink("green")
            .lit(playing.then_some("fill"))
            .keys(app_key_of(state, "play"))
            .build(|s: &mut State| s.play_or_pause()),
        )
        .child(round(state, "stop", "Stop", &STOP).build(|s: &mut State| s.stop()))
        .children(transport.records.then(|| {
            let on = state.records_now();
            round(state, "record", "Record", &RECORD)
                .ink(if on { "text-inverse" } else { "red" })
                .lit(on.then_some("red"))
                .checked(on)
                .build(|s: &mut State| {
                    if let Err(error) = s.record() {
                        s.session.report(error);
                    }
                })
        }));
    let switches = row().gap(theme.space(1)).items_center().children(SWITCHES.map(
        |(key, label, act, on)| {
            let on = on(state.session.transport());
            // Red while on, as what records.
            let lit = match (on, act) {
                (true, Act::Automation) => Some("red/20"),
                (true, _) => Some("selected"),
                (false, _) => None,
            };
            let (icon, help, keys) = match act {
                Act::Keys => (&KEYBOARD, "Play notes with the computer keys", app_key_of(state, "computer_keys")),
                Act::Automation => (
                    &AUTOMATION,
                    "Write automation: while it plays, moving a knob or a fader that has a lane records into the lane",
                    None,
                ),
                _ => (&METRONOME, "Click: a tick on every beat", None),
            };
            let mut button = round(state, key, label, icon)
                .small()
                .role("switch")
                .help(help)
                .keys(keys)
                .lit(lit)
                .checked(on);
            if on {
                button = button.ink(if act == Act::Automation { "red" } else { "text" });
            }
            button.build(move |s: &mut State| s.toggle(act))
        },
    ));
    row()
        .key("transport")
        .h(36.0)
        .pl(theme.space(1))
        .pr(theme.space(4))
        .gap(theme.space(3))
        .items_center()
        .radius(theme.radius("full").unwrap_or(18.0))
        .fill(theme.color("fill-strong"))
        .border(1.0, theme.color("border-strong"))
        .child(buttons)
        .child(readout(
            state,
            "position",
            &transport.position,
            POSITION_W,
            "text",
        ))
        .child(readout(
            state,
            "time",
            &transport.time,
            TIME_W,
            "text-muted",
        ))
        .child(
            row()
                .items_center()
                .child(typed_or(state, Typed::Tempo, tempo(state)))
                .child(word(state, "bpm")),
        )
        .child(typed_or(state, Typed::Signature, signature(state)))
        .child(switches)
        .child(transport.meter.view(state))
}

/// A number of the pill, as the piece plays.
fn readout(state: &State, key: &str, live: &LiveText, w: f32, colour: &str) -> El {
    let theme = &state.theme;
    text(String::new())
        .key(key)
        .font(theme.text("body").features("'tnum' 1"))
        .color(theme.color(colour))
        .w(w)
        .live_text(live)
}

fn word(state: &State, words: &'static str) -> El {
    let theme = &state.theme;
    text(words)
        .font(theme.text("body"))
        .color(theme.color("text-muted"))
        .pl(theme.space(1))
        .a11y_hidden()
}

/// The tempo: a drag up or down, finer with Shift, or a click or Enter to type it.
fn tempo(state: &State) -> El {
    let (_, bpm) = state.tempo_now();
    readout(state, "tempo", &state.transport.tempo, TEMPO_W, "text")
        .role("spinbutton")
        .label("Tempo")
        .a11y_value(bpm_text(bpm))
        .help("Tempo: drag up or down, Shift for finer, or click to type")
        .focusable(true)
        .cursor(Cursor::ResizeRow)
        .on_drag(|s: &mut State, d: &mut rune::Drag, tx: &mut Tx| {
            s.drag_tempo(d, tx);
        })
        .on_key(|s: &mut State, key: &KeyEvent, tx: &mut Tx| {
            type_on_enter(s, key, Typed::Tempo, tx)
        })
}

/// The time signature at the playhead: a click or Enter types it.
fn signature(state: &State) -> El {
    let map = &state.project().project_file().tempo_map;
    let now = signature_text(map.time_signatures().bar_at(state.playhead).signature);
    readout(
        state,
        "signature",
        &state.transport.signature,
        SIGNATURE_W,
        "text",
    )
    .role("spinbutton")
    .label("Time signature")
    .a11y_value(now)
    .help("Time signature: click to type, such as 3/4")
    .focusable(true)
    .cursor(Cursor::Text)
    .on_press_tx(|s: &mut State, tx: &mut Tx| {
        s.type_into(Typed::Signature, tx);
        s.handled();
    })
    .on_key(|s: &mut State, key: &KeyEvent, tx: &mut Tx| {
        type_on_enter(s, key, Typed::Signature, tx)
    })
}

fn type_on_enter(state: &mut State, key: &KeyEvent, typed: Typed, tx: &mut Tx) -> bool {
    if key.key != Key::Enter {
        return false;
    }
    state.type_into(typed, tx);
    state.handled();
    true
}

/// `readout`, or the field it is typed in while the composer types: Enter keeps what it
/// says, Escape leaves the readout as it was.
fn typed_or(state: &State, typed: Typed, readout: El) -> El {
    if state.transport.typing != Some(typed) {
        return readout;
    }
    let theme = &state.theme;
    let now = match typed {
        Typed::Tempo => bpm_text(state.tempo_now().1),
        Typed::Signature => {
            let map = &state.project().project_file().tempo_map;
            signature_text(map.time_signatures().bar_at(state.playhead).signature)
        }
    };
    let key = typed.key();
    let edit = state.transport.typed(key, &now);
    let colour = theme.color("text");
    let w = if typed == Typed::Tempo {
        TEMPO_W
    } else {
        SIGNATURE_W
    };
    rune::text_input(&edit, theme.text("body").features("'tnum' 1"))
        .single_line()
        .key(key)
        .label(if typed == Typed::Tempo {
            "Tempo"
        } else {
            "Time signature"
        })
        .color(colour)
        .caret(colour, theme.color("focus/40"))
        .w(w)
        .on_edit(rune::edit_access(move |s: &mut State| {
            s.transport.typing(key, "")
        }))
        .on_submit(move |s: &mut State, tx: &mut Tx| {
            s.typed_in(typed, Some(tx));
            s.handled();
        })
        .on_input_key(move |s: &mut State, key: &KeyEvent, tx: &mut Tx| {
            if key.key != Key::Escape {
                return false;
            }
            s.transport.typing = None;
            tx.focus(typed.readout(), true);
            s.handled();
            true
        })
}

/// A round button of the pill: an icon, its label, a tooltip with its keys.
struct Round<'a> {
    state: &'a State,
    key: &'a str,
    label: &'a str,
    icon: &'static Icon,
    ink: &'a str,
    lit: Option<&'a str>,
    role: &'static str,
    help: Option<&'a str>,
    /// The key that does it, as the keymap has it now.
    keys: Option<String>,
    checked: Option<bool>,
    side: f32,
}

fn round<'a>(state: &'a State, key: &'a str, label: &'a str, icon: &'static Icon) -> Round<'a> {
    Round {
        state,
        key,
        label,
        icon,
        ink: "text",
        lit: None,
        role: "button",
        help: None,
        keys: None,
        checked: None,
        side: 28.0,
    }
}

impl<'a> Round<'a> {
    fn ink(self, ink: &'a str) -> Self {
        Self { ink, ..self }
    }
    fn lit(self, lit: Option<&'a str>) -> Self {
        Self { lit, ..self }
    }
    fn role(self, role: &'static str) -> Self {
        Self { role, ..self }
    }
    fn help(self, help: &'a str) -> Self {
        Self {
            help: Some(help),
            ..self
        }
    }
    fn keys(self, keys: Option<String>) -> Self {
        Self { keys, ..self }
    }
    fn checked(self, on: bool) -> Self {
        Self {
            checked: Some(on),
            ..self
        }
    }
    fn small(self) -> Self {
        Self { side: 24.0, ..self }
    }

    fn build(self, press: impl Fn(&mut State) + 'static) -> El {
        let theme = &self.state.theme;
        let hover = theme.color("fill");
        let mut e = row()
            .key(self.key)
            .role(self.role)
            .label(self.label)
            .help(self.help.unwrap_or(self.label))
            .focusable(true)
            .size(self.side, self.side)
            .center()
            .radius(self.side / 2.0)
            .hover(hover)
            .press(theme.color("fill-strong"))
            .on_press(move |s: &mut State| {
                press(s);
                s.handled();
            })
            .child(
                icon(self.icon, if self.side > 24.0 { 16.0 } else { 14.0 })
                    .color(theme.color(self.ink))
                    .a11y_hidden(),
            );
        if let Some(keys) = &self.keys {
            e = e.help_keys([keys.as_str()]);
        }
        if let Some(lit) = self.lit {
            e = e.fill(theme.color(lit));
        }
        if let Some(on) = self.checked {
            e = e.checked(on);
        }
        e
    }
}

/// The level of what the device plays, left over right: the bars fall 20 dB a second, and the
/// peak line stays 1.5 s before it falls the same way. A light at the loud end says a sample
/// went over 0 dB until a click on it.
struct Meter {
    output: Peaks,
    /// In dB, left and right.
    now: [f32; 2],
    peak: [f32; 2],
    held: [f32; 2],
    clipped: bool,
    /// Per channel: the green, yellow and red parts of the bar, and the peak line.
    parts: [[rune::Live<[f32; 4]>; 4]; 2],
}

const FALL_DB_PER_SECOND: f32 = 20.0;
const HOLD_SECONDS: f32 = 1.5;
/// Under this a level is silence.
const FLOOR_DB: f32 = -96.0;
/// Where green ends and yellow begins.
const HOT_DB: f32 = -6.0;
const BAR: f32 = 3.0;
const BAR_GAP: f32 = 2.0;
const LENGTH: f32 = 40.0;
const LIGHT: f32 = 3.0;

fn place(db: f32) -> f32 {
    Scale::Fader.position(f32::NEG_INFINITY, FADER_TOP_DB, db)
}

impl Meter {
    fn new(output: Peaks) -> Self {
        Self {
            output,
            now: [f32::NEG_INFINITY; 2],
            peak: [f32::NEG_INFINITY; 2],
            held: [0.0; 2],
            clipped: false,
            parts: std::array::from_fn(|_| std::array::from_fn(|_| rune::Live::new([0.0; 4]))),
        }
    }

    /// One reading `seconds` after the last. Whether it still moves, and whether the light
    /// changed.
    fn read(&mut self, seconds: f32) -> (bool, bool) {
        let peaks = self.output.take().map(|amplitude| 20.0 * amplitude.log10());
        let fall = FALL_DB_PER_SECOND * seconds;
        let was_clipped = self.clipped;
        for channel in 0..2 {
            let peak = peaks[channel];
            let now = &mut self.now[channel];
            *now = peak.max(*now - fall);
            let held = &mut self.peak[channel];
            if peak >= *held {
                (*held, self.held[channel]) = (peak, 0.0);
            } else {
                self.held[channel] += seconds;
                let falling = (self.held[channel] - HOLD_SECONDS).clamp(0.0, seconds);
                *held = (*held - FALL_DB_PER_SECOND * falling).max(*now);
            }
            self.clipped |= peak > 0.0;
            for value in [&mut self.now[channel], &mut self.peak[channel]] {
                if value.is_nan() || *value < FLOOR_DB {
                    *value = f32::NEG_INFINITY;
                }
            }
            self.show(channel);
        }
        let rests = (self.now.iter().chain(&self.peak)).all(|db| *db == f32::NEG_INFINITY);
        (!rests, self.clipped != was_clipped)
    }

    fn show(&self, channel: usize) {
        let y = channel as f32 * (BAR + BAR_GAP);
        let (level, hot, unity) = (place(self.now[channel]), place(HOT_DB), place(0.0));
        // Nothing at all where a part is empty: a rect with no width still draws a hairline.
        let span = |from: f32, to: f32| match to > from {
            true => [from * LENGTH, y, (to - from) * LENGTH, BAR],
            false => [0.0; 4],
        };
        let [green, yellow, red, line] = &self.parts[channel];
        green.set(span(0.0, level.min(hot)));
        yellow.set(span(hot, level.clamp(hot, unity)));
        red.set(span(unity, level.max(unity)));
        let peak = place(self.peak[channel]);
        line.set(match self.peak[channel] == f32::NEG_INFINITY {
            true => [0.0; 4],
            false => [(peak * LENGTH - 1.0).max(0.0), y, 1.0, BAR],
        });
    }

    /// The level for an agent: the louder channel in dB, `-inf` in silence.
    fn said(&self) -> Value {
        let db = self.now[0].max(self.now[1]);
        match db == f32::NEG_INFINITY {
            true => json!("-inf"),
            false => json!((db * 10.0).round() / 10.0),
        }
    }

    fn view(&self, state: &State) -> El {
        let theme = &state.theme;
        let colours = ["green", "yellow", "red", "text"];
        let mut bars = el()
            .size(LENGTH + BAR_GAP + LIGHT, BAR * 2.0 + BAR_GAP)
            .a11y_hidden();
        for (channel, parts) in self.parts.iter().enumerate() {
            let y = channel as f32 * (BAR + BAR_GAP);
            bars = bars.child(
                el().absolute(0.0, y)
                    .size(LENGTH, BAR)
                    .radius(1.0)
                    .fill(theme.color("fill")),
            );
            for (part, colour) in parts.iter().zip(colours) {
                bars = bars.child(
                    el().absolute(0.0, 0.0)
                        .live_rect(part)
                        .fill(theme.color(colour)),
                );
            }
        }
        let light = el()
            .key("clip-light")
            .absolute(LENGTH + BAR_GAP, 0.0)
            .size(LIGHT, BAR * 2.0 + BAR_GAP)
            .radius(1.0)
            .fill(theme.color(if self.clipped { "red" } else { "fill" }))
            .on_background_press(|s: &mut State| {
                s.transport.meter.clipped = false;
                s.handled();
            });
        // In a well, as a display: its bars show against it while nothing plays.
        row()
            .key("level")
            .role("meter")
            .label("Output level")
            .help("What the device plays")
            .h(16.0)
            .px(theme.space(1))
            .items_center()
            .radius(theme.radius("sm").unwrap_or(4.0))
            .fill(theme.color("well"))
            .child(bars.child(light))
    }
}

macro_rules! lucide {
    ($name:ident, $key:literal, $body:literal) => {
        static $name: Icon = Icon {
            name: $key,
            view: 24.0,
            body: concat!(
                r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
                $body,
                "</g>"
            ),
        };
    };
}

lucide!(
    PLAY,
    "transport-play",
    r#"<polygon points="6 3 20 12 6 21 6 3"/>"#
);
lucide!(
    PAUSE,
    "transport-pause",
    r#"<rect x="14" y="4" width="4" height="16" rx="1"/><rect x="6" y="4" width="4" height="16" rx="1"/>"#
);
lucide!(
    STOP,
    "transport-stop",
    r#"<rect width="18" height="18" x="3" y="3" rx="2"/>"#
);
lucide!(
    RECORD,
    "transport-record",
    r#"<circle cx="12" cy="12" r="10"/>"#
);
lucide!(
    KEYBOARD,
    "transport-keys",
    r#"<rect width="20" height="16" x="2" y="4" rx="2"/><path d="M6 8h4"/><path d="M14 8h.01"/><path d="M18 8h.01"/><path d="M2 12h20"/><path d="M6 12v4"/><path d="M10 12v4"/><path d="M14 12v4"/><path d="M18 12v4"/>"#
);
lucide!(
    AUTOMATION,
    "transport-automation",
    r#"<path d="M5.25 15.44 10.75 8.56"/><path d="M13.25 8.56 18.75 15.44"/><circle cx="4" cy="17" r="2"/><circle cx="12" cy="7" r="2"/><circle cx="20" cy="17" r="2"/>"#
);
lucide!(
    METRONOME,
    "transport-click",
    r#"<path d="M9.5 3h5l4 18h-13z"/><path d="m12 21 2.5-13"/>"#
);
