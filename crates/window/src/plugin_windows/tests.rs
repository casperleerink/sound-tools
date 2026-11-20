#![allow(clippy::unwrap_used)]

use std::collections::HashMap;

use plugin_host::Placement;
use rune::{Key as R, Mods};

use super::*;

/// What the host and the desktop were asked, in the order they were.
type Log = Rc<RefCell<Vec<String>>>;

/// A host of one plugin window, `WindowId::numbered(1)`, whose plugin uses the key `a` only.
struct FakeHost {
    log: Log,
    commands: RefCell<Vec<WindowCommand>>,
}

impl Host for FakeHost {
    fn settle(&self, app_is_active: bool) -> Vec<WindowCommand> {
        self.log
            .borrow_mut()
            .push(format!("settle active={app_is_active}"));
        self.commands.take()
    }

    fn moved(&self, window: WindowId, place: WindowPlace) {
        let (x, y) = place.position;
        let id = place.display.id;
        (self.log.borrow_mut()).push(format!("moved {} {x},{y} on {id:?}", window.number()));
    }

    fn closed(&self, window: WindowId) {
        self.log
            .borrow_mut()
            .push(format!("closed {}", window.number()));
    }

    fn key(&self, window: WindowId, keystroke: &Keystroke, direction: KeyDirection) -> bool {
        let line = format!("key {} {:?} {direction:?}", window.number(), keystroke.key);
        self.log.borrow_mut().push(line);
        keystroke.key == Key::Text("a".into())
    }

    fn close_all(&self) -> Vec<WindowId> {
        self.log.borrow_mut().push("close_all".into());
        vec![WindowId::numbered(1)]
    }
}

struct FakeDesktop {
    log: Log,
    displays: Vec<Display>,
    opened: Vec<Opening>,
    placements: HashMap<WindowId, WindowPlacement>,
}

impl Desktop for FakeDesktop {
    fn displays(&self) -> Vec<Display> {
        self.displays.clone()
    }

    fn is_active(&self) -> bool {
        true
    }

    fn open(&mut self, opening: Opening) {
        self.log
            .borrow_mut()
            .push(format!("open {}", opening.window.number()));
        self.opened.push(opening);
    }

    fn resize(&mut self, window: WindowId, (width, height): (f32, f32)) {
        let line = format!("resize {} {width}x{height}", window.number());
        self.log.borrow_mut().push(line);
    }

    fn focus(&mut self, window: WindowId) {
        self.log
            .borrow_mut()
            .push(format!("focus {}", window.number()));
    }

    fn close(&mut self, window: WindowId) {
        self.log
            .borrow_mut()
            .push(format!("close {}", window.number()));
    }

    fn placement(&self, window: WindowId) -> Option<WindowPlacement> {
        self.placements.get(&window).copied()
    }
}

const MAIN: Display = Display {
    id: Some(1),
    position: (0, 0),
    size: (1920, 1080),
    scale: 1.0,
};

/// 1440 by 900 points at twice the pixels, right of the main one, as macOS counts it: its
/// corner in points times its own scale.
const RETINA: Display = Display {
    id: Some(2),
    position: (3840, 0),
    size: (2880, 1800),
    scale: 2.0,
};

const ONE: WindowId = WindowId::numbered(1);

struct Desk {
    log: Log,
    host: FakeHost,
    windows: PluginWindows<FakeDesktop>,
    events: Rc<Events>,
}

/// Plugins that count `physical` pixels, as on Windows, or points.
fn desk(physical: bool) -> Desk {
    let log = Log::default();
    let desktop = FakeDesktop {
        log: log.clone(),
        displays: vec![MAIN, RETINA],
        opened: Vec::new(),
        placements: HashMap::new(),
    };
    let events = Rc::new(Events::default());
    Desk {
        host: FakeHost {
            log: log.clone(),
            commands: RefCell::default(),
        },
        windows: PluginWindows::new(desktop, events.clone(), physical),
        events,
        log,
    }
}

impl Desk {
    fn command(&mut self, command: WindowCommand, seconds: f64) {
        self.host.commands.borrow_mut().push(command);
        self.windows.settle(&self.host, seconds);
    }

    fn take_log(&self) -> Vec<String> {
        let log = self.log.take();
        log.into_iter()
            .filter(|line| !line.starts_with("settle"))
            .collect()
    }
}

fn open(placement: Option<Placement>, focus: bool) -> WindowCommand {
    WindowCommand::Open {
        window: ONE,
        title: "Six Sines — song".into(),
        size: WindowSize {
            width: 640,
            height: 480,
        },
        resizable: true,
        placement,
        focus,
    }
}

/// 100 by 60 points into the display of twice the pixels, as the host keeps it.
fn on_the_retina() -> Placement {
    serde_json::from_str(r#"{"open":true,"x":100,"y":60,"display":2}"#).unwrap()
}

#[test]
fn a_window_opens_at_the_plugins_size_where_it_was_and_inactive_when_it_came_back_by_itself() {
    let mut points = desk(false);
    points.command(open(Some(on_the_retina()), false), 0.0);
    assert_eq!(
        points.windows.desktop.opened,
        [Opening {
            window: ONE,
            title: "Six Sines — song".into(),
            content: Content::Points(640.0, 480.0),
            resizable: true,
            position: Some((3840 + 200, 120)),
            focus: false,
        }]
    );
    assert_eq!(points.windows.titles(), ["Six Sines — song"]);
    // A display that is gone: the middle, which Rune picks.
    let mut physical = desk(true);
    let gone = serde_json::from_str(r#"{"open":true,"x":100,"y":60,"display":9}"#).unwrap();
    physical.command(open(Some(gone), true), 0.0);
    let opened = &physical.windows.desktop.opened[0];
    assert_eq!(
        (opened.content, opened.position, opened.focus),
        (Content::Physical(640, 480), None, true)
    );
}

/// Rune resizes in points; a plugin on Windows asks in physical pixels.
#[test]
fn a_resize_the_plugin_asks_for_is_in_points_at_one_and_two_times_the_pixels() {
    let resize = WindowCommand::Resize {
        window: ONE,
        size: WindowSize {
            width: 800,
            height: 600,
        },
    };
    for (physical, scale, said) in [
        (false, 2.0, "resize 1 800x600"),
        (true, 1.0, "resize 1 800x600"),
        (true, 2.0, "resize 1 400x300"),
        (true, 1.5, "resize 1 533.3333x400"),
    ] {
        let mut desk = desk(physical);
        desk.command(open(None, true), 0.0);
        let placement = WindowPlacement {
            scale,
            ..WindowPlacement::default()
        };
        desk.windows.desktop.placements.insert(ONE, placement);
        desk.take_log();
        desk.command(resize.clone(), 0.1);
        assert_eq!(desk.take_log(), [said], "{physical} at {scale}");
    }
}

#[test]
fn focus_and_close_reach_the_window_and_a_closed_one_is_no_longer_listed() {
    let mut desk = desk(false);
    desk.command(open(None, true), 0.0);
    desk.command(WindowCommand::Focus { window: ONE }, 0.1);
    desk.command(WindowCommand::Close { window: ONE }, 0.2);
    assert_eq!(desk.take_log(), ["open 1", "focus 1", "close 1"]);
    assert!(desk.windows.titles().is_empty());
}

fn at(position: (i32, i32), display: &Display) -> WindowPlacement {
    WindowPlacement {
        position,
        content_size: (640, 480),
        scale: display.scale,
        display: Some((display.position, display.size)),
        ..WindowPlacement::default()
    }
}

/// Rune has no moved callback: the places are read twice a second, and the host hears only
/// a change, so the store of places is not written for nothing.
#[test]
fn where_a_window_is_is_read_twice_a_second_and_told_only_when_it_changed() {
    let mut desk = desk(false);
    desk.command(open(None, true), 0.0);
    desk.take_log();
    let placements = &mut desk.windows.desktop.placements;
    placements.insert(ONE, at((300, 200), &MAIN));
    desk.windows.settle(&desk.host, 0.6);
    desk.windows.settle(&desk.host, 1.2);
    assert_eq!(desk.take_log(), ["moved 1 300,200 on Some(1)"]);
    let placements = &mut desk.windows.desktop.placements;
    placements.insert(ONE, at((4000, 100), &RETINA));
    desk.windows.settle(&desk.host, 1.3);
    assert!(desk.take_log().is_empty());
    desk.windows.settle(&desk.host, 1.8);
    assert_eq!(desk.take_log(), ["moved 1 4000,100 on Some(2)"]);
}

#[test]
fn a_display_the_list_does_not_have_is_told_with_no_id() {
    let plugged_in = Display {
        id: None,
        position: (-1280, 0),
        size: (1280, 1024),
        scale: 1.0,
    };
    let place = place_of(&at((-1000, 50), &plugged_in), &[MAIN, RETINA]).unwrap();
    assert_eq!(place.display, plugged_in);
    let place = place_of(&at((4000, 50), &RETINA), &[MAIN, RETINA]).unwrap();
    assert_eq!(place.display, RETINA);
    let nowhere = WindowPlacement::default();
    assert_eq!(place_of(&nowhere, &[MAIN]), None);
}

/// macOS counts a place in points times the scale of where it is, and turns one it is given
/// into points with the window's own scale, which is the main display's as it opens.
#[test]
fn a_place_on_a_display_of_twice_the_pixels_is_given_in_the_windows_own_scale() {
    let place = (3840 + 200, 120);
    assert_eq!(frame_at(place, &[MAIN, RETINA], 1.0), (2020, 60));
    assert_eq!(frame_at(place, &[MAIN, RETINA], 2.0), (4040, 120));
    assert_eq!(frame_at((300, 200), &[MAIN, RETINA], 1.0), (300, 200));
}

fn event(key: R, mods: Mods) -> KeyEvent {
    KeyEvent { key, mods }
}

#[test]
fn a_key_reaches_the_plugin_as_the_host_names_it() {
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    let text = |text: &str| Key::Text(text.into());
    for (key, mods, said) in [
        (R::Text("A".into()), shift, Some(text("A"))),
        (R::Text(" ".into()), Mods::default(), Some(Key::Space)),
        (R::Command('z'), Mods::default(), Some(text("z"))),
        (R::Enter, Mods::default(), Some(Key::Enter)),
        (R::Left, Mods::default(), Some(Key::Left)),
        (R::Function(5), Mods::default(), Some(Key::Function(5))),
        (R::ContextMenu, Mods::default(), None),
    ] {
        let keystroke = keystroke(&event(key, mods));
        assert_eq!(keystroke.as_ref().map(|it| &it.key), said.as_ref());
        if let Some(keystroke) = keystroke {
            assert_eq!(keystroke.modifiers.shift, mods.shift);
        }
    }
    // ⌘ on macOS and Ctrl elsewhere are the command key; the host's `control` is the other.
    let modifiers = keystroke(&event(R::Command('w'), crate::app::MAIN_MODIFIER))
        .unwrap()
        .modifiers;
    assert_eq!(
        modifiers,
        Modifiers {
            command: true,
            ..Modifiers::default()
        }
    );
}

/// The plugin uses `a`. Every other key, and every key up, goes to the main window, so the
/// computer keys play it and a note they started ends. A key held keeps saying it repeats.
#[test]
fn a_key_the_plugin_does_not_use_and_every_key_up_go_to_the_main_window() {
    let desk = desk(false);
    let typed = |key: &str, direction| {
        let event = event(R::Text(key.into()), Mods::default());
        super::key(&desk.host, &desk.events, ONE, &event, direction);
    };
    typed("a", (KeyDirection::Down, false));
    typed("a", (KeyDirection::Up, false));
    typed("q", (KeyDirection::Down, false));
    typed("q", (KeyDirection::Down, true));
    typed("q", (KeyDirection::Up, false));
    let text = |key: &str| event(R::Text(key.into()), Mods::default());
    let down = |repeat| Spare::Down {
        key: text("q"),
        repeat,
    };
    assert_eq!(
        desk.windows.take_spare_keys(),
        [
            Spare::Up(text("a")),
            down(false),
            down(true),
            Spare::Up(text("q"))
        ]
    );
    let told = desk.take_log();
    assert_eq!(told.len(), 5, "the plugin hears every key: {told:?}");
}

/// The plugin lets go of its view before the window goes, and the window stays open for the
/// next start.
#[test]
fn quitting_lets_every_plugin_go_of_its_view_before_its_window_closes() {
    let mut desk = desk(false);
    desk.command(open(None, true), 0.0);
    desk.take_log();
    desk.windows.close_all(&desk.host);
    assert_eq!(desk.take_log(), ["close_all", "close 1"]);
    assert!(desk.windows.titles().is_empty());
}

/// The place reads only before the window goes: the host hears it, then the close.
#[test]
fn a_window_the_composer_closes_says_where_it_was_then_that_it_closed() {
    let mut desk = desk(false);
    desk.command(open(None, true), 0.0);
    desk.take_log();
    let place = place_of(&at((40, 80), &MAIN), &[MAIN]);
    closed_by_composer(&desk.host, &desk.events, ONE, place);
    assert_eq!(desk.take_log(), ["moved 1 40,80 on Some(1)", "closed 1"]);
    desk.windows.settle(&desk.host, 0.1);
    assert!(desk.windows.titles().is_empty());
}
