//! The stage: the div of a page's view that says `stage`, in a window of its own that the
//! composer moves, sizes and puts full screen on any screen. Closed, it shows in the page.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, App, Bounds, Context, Entity, EventEmitter, MouseButton, MouseDownEvent, Render,
    TitlebarOptions, WeakEntity, Window, WindowBounds, WindowControlArea, WindowOptions, div,
    point, prelude::*, px, size,
};
use serde::{Deserialize, Serialize};
use sound_ui::ActiveTheme;
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};

use super::TypeScriptCard;

/// Where the div that says `stage` draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum StageDiv {
    /// In the stage's window, so the page leaves it out.
    Away,
    /// Here, in the stage's window.
    Here,
    /// In the page, the window closed, under a button that opens it again.
    InPage,
}

/// Where a page's stage was on this machine: kept by the project, not in its history.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(super) struct StagePlace {
    closed: bool,
    window: Option<StageWindow>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StageWindow {
    /// The screen, by its id that lasts.
    display: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    shown: Shown,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Shown {
    Windowed,
    Maximized,
    Fullscreen,
}

impl StageWindow {
    fn new(bounds: WindowBounds, display: String) -> Self {
        let (shown, bounds) = match bounds {
            WindowBounds::Windowed(bounds) => (Shown::Windowed, bounds),
            WindowBounds::Maximized(bounds) => (Shown::Maximized, bounds),
            WindowBounds::Fullscreen(bounds) => (Shown::Fullscreen, bounds),
        };
        Self {
            display,
            x: bounds.origin.x.into(),
            y: bounds.origin.y.into(),
            width: bounds.size.width.into(),
            height: bounds.size.height.into(),
            shown,
        }
    }

    fn bounds(&self) -> WindowBounds {
        let bounds = Bounds::new(
            point(px(self.x), px(self.y)),
            size(px(self.width), px(self.height)),
        );
        match self.shown {
            Shown::Windowed => WindowBounds::Windowed(bounds),
            Shown::Maximized => WindowBounds::Maximized(bounds),
            Shown::Fullscreen => WindowBounds::Fullscreen(bounds),
        }
    }
}

fn places_file(root: &Path) -> PathBuf {
    root.join(".sound-tools").join("stages.json")
}

/// The places of the stages of a project, by the page they belong to.
fn read_places(root: &Path) -> HashMap<String, StagePlace> {
    (std::fs::read(places_file(root)).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

impl TypeScriptCard {
    /// Opens the stage's window when its div came, unless it was closed last, and closes it
    /// when its div went.
    pub(crate) fn place_stage(&mut self, stage: Option<usize>, cx: &mut Context<Self>) {
        let came = self.stage.is_none() && stage.is_some();
        self.stage = stage;
        if stage.is_none() {
            self.close_stage(cx);
            return;
        }
        if !came {
            cx.emit(StageChanged);
            return;
        }
        let root = self.session.read(cx).project().root().to_path_buf();
        self.stage_place = (read_places(&root).remove(&self.id.to_string())).unwrap_or_default();
        self.open_stage_window(cx);
    }

    /// Where the page draws the stage's div.
    pub(super) fn stage_in_page(&self) -> StageDiv {
        match self.stage_place.closed {
            true => StageDiv::InPage,
            false => StageDiv::Away,
        }
    }

    pub(super) fn open_stage(&mut self, cx: &mut Context<Self>) {
        self.stage_place.closed = false;
        self.save_stage(cx);
        self.open_stage_window(cx);
        cx.notify();
    }

    fn open_stage_window(&mut self, cx: &mut Context<Self>) {
        let card = cx.entity();
        // Not inside this update: the window's first draw reads the card.
        cx.defer(move |cx| {
            let place = {
                let card = card.read(cx);
                if card.stage.is_none() || card.stage_window.is_some() || card.stage_place.closed {
                    return;
                }
                card.stage_place.window.clone()
            };
            let options = stage_options(place.as_ref(), cx);
            match cx.open_window(options, |window, cx| {
                cx.new(|cx| Stage::new(&card, window, cx))
            }) {
                Ok(window) => card.update(cx, |card, _| card.stage_window = Some(window)),
                Err(error) => eprintln!("error: the stage did not open: {error:#}"),
            }
        });
    }

    /// The composer closed its window: it shows in the page until they open it again.
    fn stage_closed(&mut self, cx: &mut Context<Self>) {
        self.stage_window = None;
        self.stage_place.closed = true;
        self.save_stage(cx);
        cx.notify();
    }

    fn stage_moved(&mut self, bounds: WindowBounds, display: String, cx: &mut Context<Self>) {
        self.stage_place.window = Some(StageWindow::new(bounds, display));
        self.save_stage(cx);
    }

    fn save_stage(&self, cx: &App) {
        let root = self.session.read(cx).project().root();
        let mut places = read_places(root);
        places.insert(self.id.to_string(), self.stage_place.clone());
        let path = places_file(root);
        let written = (path.parent().map_or(Ok(()), std::fs::create_dir_all))
            .and_then(|()| std::fs::write(&path, serde_json::to_vec(&places)?));
        if let Err(error) = written {
            eprintln!("error: where the stage is was not kept: {error}");
        }
    }

    pub(super) fn close_stage(&mut self, cx: &mut App) {
        if let Some(window) = self.stage_window.take()
            && let Err(error) = window.update(cx, |_, window, _| window.remove_window())
        {
            // It was closed already.
            drop(error);
        }
    }

    /// The div its view shows on the stage, from the feeds as last read.
    fn draw_stage(&self, window: &Window, cx: &App) -> Option<AnyElement> {
        let slot = self.stage?;
        let state = self.read_state()?;
        let Some(Ok(table)) = self.live.read(cx).tree(self.card).cloned() else {
            return None;
        };
        let mut drawing = self.drawing(state, &table, window, cx);
        drawing.stage = StageDiv::Here;
        drawing.out_of_page = true;
        let shown = drawing.draw(slot);
        let spaces = drawing.spaces;
        self.keep_spaces(spaces);
        Some(shown)
    }
}

/// Where it was last, on a screen still there; else a window in the middle of the main screen.
fn stage_options(place: Option<&StageWindow>, cx: &App) -> WindowOptions {
    let kept = place.and_then(|place| {
        let display = (cx.displays().into_iter())
            .find(|display| (display.uuid()).is_ok_and(|uuid| uuid.to_string() == place.display))?;
        Some((display.id(), place.bounds()))
    });
    let (display_id, window_bounds) = match kept {
        Some((display, bounds)) => (Some(display), bounds),
        None => (
            None,
            WindowBounds::Windowed(Bounds::centered(None, size(px(960.), px(540.)), cx)),
        ),
    };
    WindowOptions {
        window_bounds: Some(window_bounds),
        display_id,
        // Over what it shows: GPUI keeps a window's frame and opens one by what it shows, so a
        // bar of its own would grow the window each time it opens again.
        titlebar: Some(TitlebarOptions {
            title: Some("Stage".into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        focus: false,
        window_min_size: Some(size(px(160.), px(90.))),
        // It plays to an audience while the app has the focus: every frame, not a few.
        inactive_frame_interval: None,
        ..Default::default()
    }
}

/// The button over the stage's div in the page, which opens its window again.
pub(super) fn open_stage_button(card: WeakEntity<TypeScriptCard>) -> AnyElement {
    div()
        .absolute()
        .top(px(8.))
        .right(px(8.))
        .child(
            Button::new("open-stage", "Open in a window")
                .size(ButtonSize::Xs)
                .variant(ButtonVariant::Subtle)
                .on_click(move |_, _, cx| {
                    if let Err(error) = card.update(cx, |card, cx| card.open_stage(cx)) {
                        // The page is gone.
                        drop(error);
                    }
                }),
        )
        .into_any_element()
}

/// What the stage shows changed while nothing else of the page did.
pub(crate) struct StageChanged;

impl EventEmitter<StageChanged> for TypeScriptCard {}

/// The window of the stage: black, and the div over all of it. It holds its card weakly, so a
/// card that goes takes its stage along.
pub(crate) struct Stage(WeakEntity<TypeScriptCard>);

impl Stage {
    fn new(card: &Entity<TypeScriptCard>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe(card, |_, _, cx| cx.notify()).detach();
        cx.subscribe(card, |_, _, _: &StageChanged, cx| cx.notify())
            .detach();
        cx.observe_release(card, |_, _, cx| cx.notify()).detach();
        cx.observe_window_bounds(window, |stage: &mut Self, window, cx| {
            let bounds = window.window_bounds();
            let display = (window.display(cx)).and_then(|display| display.uuid().ok());
            if let (Some(card), Some(display)) = (stage.0.upgrade(), display) {
                card.update(cx, |card, cx| {
                    card.stage_moved(bounds, display.to_string(), cx)
                });
            }
        })
        .detach();
        let closed = card.downgrade();
        window.on_window_should_close(cx, move |_, cx| {
            if let Err(error) = closed.update(cx, |card, cx| card.stage_closed(cx)) {
                // The page is gone, and the stage with it.
                drop(error);
            }
            true
        });
        Self(card.downgrade())
    }
}

impl Render for Stage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(card) = self.0.upgrade() else {
            window.remove_window();
            return div().into_any_element();
        };
        let card = card.read(cx);
        // What moves on the stage draws the page again too, which reads the feeds for it.
        if card.parts(cx).body.animates {
            window.request_animation_frame();
        }
        let shown = card.draw_stage(window, cx);
        // Its top edge moves it, as a title bar does, but for full screen.
        let bar = (!window.is_fullscreen()).then(|| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .w_full()
                .h(px(28.))
                .window_control_area(WindowControlArea::Drag)
                .on_mouse_down(MouseButton::Left, |event: &MouseDownEvent, window, _| {
                    if event.click_count == 2 {
                        window.titlebar_double_click();
                    } else {
                        window.start_window_move();
                    }
                })
        });
        div()
            .size_full()
            .bg(gpui::black())
            .text_size(px(12.))
            .text_color(cx.theme().gray_950)
            .flex()
            .flex_col()
            .children(shown)
            .children(bar)
            .into_any_element()
    }
}
