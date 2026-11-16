//! Can a plugin's editor, a native child `NSView`, live in a Rune window?
//!
//! The main window opens two kinds of plugin window at run time with `rune::open_window`:
//! a real CLAP plugin's editor (`--clap PATH`, Six Sines by default) and a stand-in `NSView`
//! that logs every mouse and key event it gets. Every line of the log goes to stdout.

mod clap;
mod stand_in;

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use raw_window_handle::RawWindowHandle;
use rune::tok;
use rune::{App, AppSpec, Cx, El, Key, KeyEvent, Tx, WindowHandle, WindowOptions, WindowSpec, col, el, row, text};

thread_local! {
    static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static START: Cell<Option<Instant>> = const { Cell::new(None) };
    static PLUGIN: RefCell<Option<clap::Plugin>> = const { RefCell::new(None) };
    static STAND_IN: RefCell<Option<objc2::rc::Retained<stand_in::EditorView>>> = const { RefCell::new(None) };
}

pub fn log(line: impl Into<String>) {
    let start = START.with(|s| *s.get().get_or_insert_with(|| {
        let now = Instant::now();
        s.set(Some(now));
        now
    }));
    let line = format!("[{:7.3}] {}", start.elapsed().as_secs_f64(), line.into());
    println!("{line}");
    LOG.with(|l| l.borrow_mut().push(line));
}

fn color(hex: &str) -> rune::Color {
    rune::color::hex(hex).unwrap_or(rune::Color::BLACK)
}

fn log_len() -> usize {
    LOG.with(|l| l.borrow().len())
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Clap,
    StandIn,
}

impl Kind {
    fn key(self) -> &'static str {
        match self {
            Kind::Clap => "clap",
            Kind::StandIn => "stand-in",
        }
    }
}

/// How tall Rune's own strip under the stand-in view is, in points.
const STRIP: f32 = 48.0;
const STAND_IN_SIZE: (f64, f64) = (480.0, 320.0);

fn clap_path() -> PathBuf {
    let args = rune::extra_args();
    let given = args.iter().position(|a| a == "--clap").and_then(|i| args.get(i + 1));
    match given {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Library/Audio/Plug-Ins/CLAP/Six Sines.clap"),
    }
}

// The main window.

struct Main {
    seen: usize,
    windows: Vec<(Kind, WindowHandle)>,
}

fn main_view(_: &Main, _: &mut Cx<Main>) -> El {
    let t = tok::theme(true);
    let button = |key: &str, label: &str, kind: Kind| {
        el().key(key).role("button").label(label).focusable(true).padding(10.0).radius(6.0).fill(tok::accent(0, true))
            .on_press_tx(move |s: &mut Main, tx: &mut Tx| open_plugin_window(s, kind, tx))
            .child(text(label).color(t.text))
    };
    let grow = el().key("grow").role("button").label("Grow stand-in").focusable(true).padding(10.0).radius(6.0).fill(t.control_off)
        .on_press(|s: &mut Main| grow_stand_in(s))
        .child(text("Grow stand-in").color(t.text));
    let lines = LOG.with(|l| {
        let l = l.borrow();
        l.iter().rev().take(24).rev().map(|line| text(line).color(t.text).font(tok::type_::LABEL)).collect::<Vec<_>>()
    });
    col().w_full().h_full().padding(16.0).gap(12.0).fill(t.bg)
        .child(row().gap(8.0).child(button("open-clap", "Open CLAP editor", Kind::Clap)).child(button("open-stand-in", "Open stand-in", Kind::StandIn)).child(grow))
        .child(col().gap(2.0).children(lines))
}

fn main_poll(s: &mut Main, _: f64, tx: &mut Tx) -> bool {
    tx.wake_after(Duration::from_millis(100));
    let len = log_len();
    let changed = len != s.seen;
    s.seen = len;
    changed
}

fn main_key(_: &mut Main, event: &KeyEvent, _: &mut Tx) -> bool {
    log(format!("APP main window on_key: {}", describe(event)));
    false
}

fn describe(event: &KeyEvent) -> String {
    let key = match &event.key {
        Key::Text(t) => format!("{t:?}"),
        other => format!("{other:?}"),
    };
    format!("{key} mods={:?}", event.mods)
}

fn grow_stand_in(s: &mut Main) {
    if let Some((_, handle)) = s.windows.iter().find(|(k, _)| *k == Kind::StandIn) {
        log("grow stand-in window to 640x480 (animated Rune resize)");
        handle.resize((640.0, 480.0 + STRIP));
    }
}

// The plugin windows.

struct PluginWindow {
    kind: Kind,
    handle: Option<WindowHandle>,
    frames: u64,
    last_placement: Option<rune::WindowPlacement>,
    /// The editor size the window was given, in points.
    size: (u32, u32),
    resizable: bool,
}

fn placement_file(kind: Kind) -> PathBuf {
    std::env::temp_dir().join(format!("rune-plugin-window-{}.txt", kind.key()))
}

fn saved_position(kind: Kind) -> Option<(i32, i32)> {
    let text = std::fs::read_to_string(placement_file(kind)).ok()?;
    let mut parts = text.split_whitespace().map(|p| p.parse::<i32>());
    Some((parts.next()?.ok()?, parts.next()?.ok()?))
}

fn open_plugin_window(s: &mut Main, kind: Kind, _: &mut Tx) {
    if s.windows.iter().any(|(k, _)| *k == kind) {
        log(format!("{kind:?} window already open"));
        return;
    }
    // CLAP asks for: create, get size, set parent, show.
    let (size, resizable) = match kind {
        Kind::Clap => match prepare_clap() {
            Ok(prepared) => prepared,
            Err(error) => {
                log(format!("CLAP did not open: {error}"));
                return;
            }
        },
        Kind::StandIn => ((STAND_IN_SIZE.0 as u32, STAND_IN_SIZE.1 as u32), true),
    };
    let height = size.1 as f32 + if kind == Kind::StandIn { STRIP } else { 0.0 };
    let position = saved_position(kind);
    log(format!("open {kind:?} window: content {}x{} pt, saved position {position:?}", size.0, height));
    let spec = AppSpec::<PluginWindow> {
        poll: Some(plugin_poll),
        on_key: Some(plugin_key),
        ..AppSpec::new(match kind { Kind::Clap => "CLAP editor", Kind::StandIn => "Stand-in editor" }, (size.0 as f32, height), || unreachable_state(), plugin_view)
    };
    let app = App::new(spec).on_close(plugin_closed);
    let options = WindowOptions { min_size: Some((0.0, 0.0)), resizable, position, ..Default::default() };
    let spec = WindowSpec::from_app(kind.key(), app)
        // Rune's default puts content under the title bar, where the editor would sit under the
        // window's buttons.
        .chrome(rune::Chrome { full_size_content: false, ..rune::Chrome::DEFAULT })
        .options(options)
        .state(PluginWindow { kind, handle: None, frames: 0, last_placement: None, size, resizable })
        .on_created(move |handle| {
            attach(kind, &handle, size);
            HANDLES.with(|h| h.borrow_mut().push((kind, handle)));
        });
    let handle = rune::open_window(spec);
    s.windows.push((kind, handle));
}

/// `AppSpec::init` is never called: `WindowSpec::state` gives the state.
fn unreachable_state() -> PluginWindow {
    PluginWindow { kind: Kind::StandIn, handle: None, frames: 0, last_placement: None, size: (0, 0), resizable: false }
}

fn prepare_clap() -> Result<((u32, u32), bool), String> {
    PLUGIN.with(|p| {
        let mut p = p.borrow_mut();
        if p.is_none() {
            let path = clap_path();
            log(format!("load CLAP {}", path.display()));
            *p = Some(clap::Plugin::load(&path)?);
        }
        let plugin = p.as_mut().ok_or("no plugin")?;
        plugin.create()?;
        let size = plugin.size().unwrap_or((600, 400));
        let resizable = plugin.can_resize();
        log(format!("CLAP gui created: get_size {}x{} (points on macOS), can_resize {resizable}", size.0, size.1));
        Ok((size, resizable))
    })
}

fn ns_view(handle: &WindowHandle) -> Option<NonNull<c_void>> {
    handle
        .with_raw_handle(|raw| match raw.as_raw() {
            RawWindowHandle::AppKit(appkit) => Some(appkit.ns_view),
            _ => None,
        })
        .ok()
        .flatten()
}

fn attach(kind: Kind, handle: &WindowHandle, size: (u32, u32)) {
    let Some(view) = ns_view(handle) else {
        log("no NSView in the raw handle");
        return;
    };
    log(format!("{kind:?} window created, number {:?}, Rune NSView {:p}: {}", handle.window_number(), view.as_ptr(), stand_in::describe_parent(view)));
    match kind {
        Kind::Clap => PLUGIN.with(|p| {
            let mut p = p.borrow_mut();
            let Some(plugin) = p.as_mut() else { return };
            // SAFETY: the view lives until `on_close`, which destroys the editor first.
            match unsafe { plugin.set_parent(view) }.and_then(|()| plugin.show()) {
                Ok(()) => log("CLAP set_parent + show ok"),
                Err(error) => log(format!("CLAP attach failed: {error}")),
            }
        }),
        Kind::StandIn => {
            let child = stand_in::attach(view, (f64::from(size.0), f64::from(size.1)));
            STAND_IN.with(|s| *s.borrow_mut() = Some(child));
        }
    }
    log(format!("after attach: {}", stand_in::describe_subviews(view)));
    stand_in::float_above_main(view);
}

fn plugin_view(s: &PluginWindow, _: &mut Cx<PluginWindow>) -> El {
    // Bright and moving, so any frame where Rune draws over the editor shows in a capture.
    let phase = (s.frames % 120) as f32 / 120.0;
    let under = el().w_full().grow(1.0).fill(color(if s.frames % 2 == 0 { "#ff00ff" } else { "#00ffff" }));
    let strip = row().w_full().h(STRIP).padding(8.0).gap(12.0).fill(color("#1a1a1f"))
        .child(el().w(200.0 * phase + 8.0).h(STRIP - 16.0).fill(color("#33e666")))
        .child(text(format!("Rune frame {}", s.frames)).color(color("#ffffff")));
    let root = match s.kind {
        Kind::Clap => col().w_full().h_full().child(under),
        Kind::StandIn => col().w_full().h_full().child(under).child(strip),
    };
    root.on_key_up(|s: &mut PluginWindow, event: &KeyEvent, _: &mut Tx| {
        log(format!("APP {:?} window on_key_up: {}", s.kind, describe(event)));
        false
    })
}

fn plugin_poll(s: &mut PluginWindow, seconds: f64, tx: &mut Tx) -> bool {
    s.frames += 1;
    if s.frames % 120 == 0 {
        let presents = s.handle.as_ref().map(|h| h.surface_stats().totals.presents);
        log(format!("{:?} poll {} at {seconds:.2}s, Rune presents {presents:?}", s.kind, s.frames));
    }
    // Redraw every display frame, so Rune presents over and over under the editor.
    tx.wake_after(Duration::from_millis(16));
    if s.handle.is_none() {
        s.handle = HANDLES.with(|h| h.borrow().iter().find(|(k, _)| *k == s.kind).map(|(_, h)| h.clone()));
    }
    if s.kind == Kind::Clap {
        clap_poll(s);
    }
    // No move or resize callback in Rune: read the placement every frame (it is a few
    // AppKit getters) and act on a change.
    check_placement(s);
    true
}

/// What a move or resize callback would give: polled twice a second.
fn check_placement(s: &mut PluginWindow) {
    let Some(placement) = s.handle.as_ref().and_then(WindowHandle::placement) else { return };
    if s.last_placement.map(|p| (p.position, p.content_size)) != Some((placement.position, placement.content_size)) {
        let presents = s.handle.as_ref().map(|h| { let stats = h.surface_stats(); (stats.totals.presents, stats.per_second.presents) });
        log(format!(
            "{:?} placement: frame at {:?} px, content {:?} px, scale {}; Rune presents (total, last second) {presents:?}",
            s.kind, placement.position, placement.content_size, placement.scale
        ));
        if let Some(view) = s.handle.as_ref().and_then(ns_view) {
            log(format!("{:?} views: {}", s.kind, stand_in::describe_subviews(view)));
        }
        let content = (
            (f64::from(placement.content_size.0) / placement.scale).round() as u32,
            (f64::from(placement.content_size.1) / placement.scale).round() as u32,
        );
        if s.kind == Kind::Clap && s.resizable && s.last_placement.is_some() && content != s.size {
            clap_user_resized(s, content);
        }
        if s.kind == Kind::StandIn {
            log(format!("stand-in child: {}", STAND_IN.with(|v| v.borrow().as_ref().map(|v| stand_in::frame_of(v)).unwrap_or_default())));
        }
        s.last_placement = Some(placement);
    }
}

fn clap_poll(s: &mut PluginWindow) {
    let wanted = PLUGIN.with(|p| p.borrow_mut().as_mut().and_then(clap::Plugin::poll));
    if let Some(wanted) = wanted
        && wanted != s.size
    {
        log(format!("CLAP request_resize {}x{}: resize window instantly", wanted.0, wanted.1));
        s.size = wanted;
        if let Some(handle) = &s.handle {
            handle.resize_instant((wanted.0 as f32, wanted.1 as f32));
        }
    }
}

fn clap_user_resized(s: &mut PluginWindow, content: (u32, u32)) {
    let settled = PLUGIN.with(|p| p.borrow_mut().as_mut().and_then(|p| p.resize(content)));
    log(format!("user resized CLAP window to {}x{} pt, plugin settled on {settled:?}", content.0, content.1));
    if let Some(settled) = settled {
        s.size = settled;
        if settled != content && let Some(handle) = &s.handle {
            handle.resize_instant((settled.0 as f32, settled.1 as f32));
        }
    }
}

fn plugin_key(s: &mut PluginWindow, event: &KeyEvent, _: &mut Tx) -> bool {
    let responder = s.handle.as_ref().and_then(ns_view).map(stand_in::first_responder).unwrap_or_default();
    log(format!("APP {:?} window on_key: {} (first responder {responder})", s.kind, describe(event)));
    false
}

fn plugin_closed(s: &mut PluginWindow, _: &mut Tx) {
    let placement = s.handle.as_ref().and_then(WindowHandle::placement);
    log(format!("{:?} on_close; placement still readable: {placement:?}", s.kind));
    if let Some(placement) = placement {
        // `WindowOptions::position` places the content's top-left on macOS, not the frame's
        // as Rune's docs say, so the content's corner is what comes back right.
        let corner = placement.content_position;
        let text = format!("{} {}", corner.0, corner.1);
        match std::fs::write(placement_file(s.kind), text) {
            Ok(()) => log(format!("saved content corner {corner:?} to {}", placement_file(s.kind).display())),
            Err(error) => log(format!("could not save the position: {error}")),
        }
    }
    match s.kind {
        Kind::Clap => PLUGIN.with(|p| {
            if let Some(plugin) = p.borrow_mut().as_mut() {
                plugin.destroy();
                log("CLAP gui destroyed");
            }
        }),
        Kind::StandIn => STAND_IN.with(|v| {
            if let Some(view) = v.borrow_mut().take() {
                view.removeFromSuperview();
                log("stand-in removed");
            }
        }),
    }
    MAIN_WINDOWS.with(|w| w.borrow_mut().push(s.kind));
    HANDLES.with(|h| h.borrow_mut().retain(|(k, _)| *k != s.kind));
}

thread_local! {
    /// Windows that closed, for the main window to forget.
    static MAIN_WINDOWS: RefCell<Vec<Kind>> = const { RefCell::new(Vec::new()) };
    /// What `on_created` gives, for the window's own state to place and resize it.
    static HANDLES: RefCell<Vec<(Kind, WindowHandle)>> = const { RefCell::new(Vec::new()) };
}

fn main_poll_with_closes(s: &mut Main, seconds: f64, tx: &mut Tx) -> bool {
    let closed = MAIN_WINDOWS.with(|w| std::mem::take(&mut *w.borrow_mut()));
    s.windows.retain(|(k, _)| !closed.contains(k));
    main_poll(s, seconds, tx)
}

fn main() {
    log("start");
    let spec = AppSpec::<Main> {
        poll: Some(main_poll_with_closes),
        on_key: Some(main_key),
        ..AppSpec::new("Rune plugin window spike", (720.0, 520.0), || Main { seen: 0, windows: Vec::new() }, main_view)
    };
    App::new(spec).run();
}
