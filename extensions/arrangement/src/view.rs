//! The arrangement view: track headers, a bar ruler, clips with a miniature of their notes,
//! and the playhead. Read-only for now: it seeks and selects.
//!
//! Three views, so that a moving playhead repaints almost nothing:
//! - [`ArrangementView`] is what the window shows. It only stacks the other two.
//! - [`Timeline`] draws everything that changes with the project, the scroll and the zoom on
//!   one canvas, and only what is visible. GPUI keeps its painted frame while it is not
//!   notified, so playback does not run this code.
//! - `PlayheadLine` draws one line on top, every frame while the project plays.
//!
//! All positions come from [`layout`]. The timeline keeps the [`Scene`] it painted for its
//! mouse listeners, so a click hits exactly what is on screen.

pub mod layout;

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, BorderStyle, Bounds, ContentMask, Context, DispatchPhase, Entity, FocusHandle, Focusable,
    FontWeight, Hitbox, HitboxBehavior, Hsla, MouseButton, MouseDownEvent, PinchEvent, Pixels,
    Point, ScrollWheelEvent, SharedString, StyleRefinement, Subscription, TextAlign, TextRun,
    TruncateFrom, Window, canvas, div, fill, point, prelude::*, px, quad, size,
};
use sound_core::{Instance, InstanceId, ProjectEvent, Ticks, TimeSignature};
use sound_notes::Clip;
use sound_ui::{ActiveTheme, Playhead, Session, Theme, Views, typography};

use crate::{ArrangementState, Colour, TrackState, end, tracks};
use layout::{Extent, HEADER_WIDTH, RULER_HEIGHT, Rect, TRACK_HEIGHT, Viewport, snap};

/// Registers the view of the `arrangement` tool.
pub fn register(views: &mut Views) {
    views.register(ArrangementView::new);
}

pub struct ArrangementView {
    timeline: Entity<Timeline>,
    playhead_line: Entity<PlayheadLine>,
}

impl ArrangementView {
    pub fn new(
        session: Entity<Session>,
        arrangement: Instance<ArrangementState>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let playhead = session.read(cx).playhead().clone();
        let timeline = cx.new(|cx| Timeline::new(session, arrangement, cx));
        let playhead_line = cx.new(|cx| PlayheadLine::new(playhead, &timeline, cx));
        Self {
            timeline,
            playhead_line,
        }
    }

    pub fn timeline(&self) -> &Entity<Timeline> {
        &self.timeline
    }
}

impl Render for ArrangementView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let timeline = self.timeline.clone();
        div()
            .size_full()
            .relative()
            // Cached: a frame that only moves the playhead reuses what the timeline painted.
            .child(timeline.cached(StyleRefinement::default().size_full()))
            .child(self.playhead_line.clone())
    }
}

/// Track colours are design tokens. The match is exhaustive, so a new colour cannot be
/// left without one.
fn accent(colour: Colour, theme: &Theme) -> Hsla {
    match colour {
        Colour::Blue => theme.blue,
        Colour::Sapphire => theme.sapphire,
        Colour::Sky => theme.sky,
        Colour::Teal => theme.teal,
        Colour::Green => theme.green,
        Colour::Yellow => theme.yellow,
        Colour::Peach => theme.peach,
        Colour::Red => theme.red,
        Colour::Maroon => theme.maroon,
        Colour::Mauve => theme.mauve,
        Colour::Pink => theme.pink,
        Colour::Lavender => theme.lavender,
        Colour::Rosewater => theme.rosewater,
        Colour::Flamingo => theme.flamingo,
    }
}

struct TrackRow {
    y: f32,
    name: SharedString,
    accent: Hsla,
}

/// A clip as it is on screen, in the coordinates of [`layout`].
pub struct ClipShape {
    pub clip: Instance<Clip>,
    pub rect: Rect,
    start: Ticks,
    notes: Vec<Rect>,
    accent: Hsla,
    selected: bool,
}

/// What one paint shows: only the visible rows, clips and bars. Later clips are on top.
pub struct Scene {
    pub viewport: Viewport,
    pub clips: Vec<ClipShape>,
    rows: Vec<TrackRow>,
    bars: Vec<(u64, f32)>,
}

impl Scene {
    /// The clip on top at a position in the timeline area.
    pub fn clip_at(&self, x: f32, y: f32) -> Option<&ClipShape> {
        self.clips
            .iter()
            .rev()
            .find(|shape| shape.rect.contains(x, y))
    }
}

pub struct Timeline {
    session: Entity<Session>,
    arrangement: Instance<ArrangementState>,
    /// Zoom and scroll, kept inside the content by [`Self::set_viewport`]. Scroll and pinch go
    /// on from here, not from what was painted: several events may arrive between two frames.
    viewport: Viewport,
    /// The viewport of the last paint, for the playhead line. Filled by paint, like a GPUI
    /// scroll handle. It differs from `viewport` only while the window or the project changed
    /// size under it.
    painted: Rc<Cell<Viewport>>,
    /// The size of the timeline area at the last paint, which the scroll limits depend on.
    painted_size: Rc<Cell<(f32, f32)>>,
    /// The tracks in display order and the end of the last clip. Finding them walks every
    /// clip, so they are kept between the project events that can change them and are not
    /// read again per paint. Nothing else of the project is kept.
    order: Vec<Instance<TrackState>>,
    end: Ticks,
    order_is_stale: bool,
    selected_clip: Option<InstanceId>,
    focus_handle: FocusHandle,
    _project_events: Subscription,
}

impl Timeline {
    fn new(
        session: Entity<Session>,
        arrangement: Instance<ArrangementState>,
        cx: &mut Context<Self>,
    ) -> Self {
        let project_events = cx.subscribe(&session, |timeline, _, event, cx| {
            let shown = |id: &InstanceId| {
                id == timeline.arrangement.id() || id.is_inside(timeline.arrangement.id())
            };
            let changed = match event {
                ProjectEvent::Created(id) | ProjectEvent::Changed(id) => shown(id),
                ProjectEvent::Deleted(id) => {
                    if timeline.selected_clip.as_ref() == Some(id) {
                        timeline.selected_clip = None;
                    }
                    shown(id)
                }
                // The time signature places the bars.
                ProjectEvent::ProjectFileChanged => true,
                ProjectEvent::ProblemsChanged => false,
            };
            if changed {
                // Read again at the next render, once for all events of a group.
                timeline.order_is_stale = true;
                cx.notify();
            }
        });
        Self {
            session,
            arrangement,
            viewport: Viewport::default(),
            painted: Rc::default(),
            painted_size: Rc::default(),
            order: Vec::new(),
            end: Ticks(0),
            order_is_stale: true,
            selected_clip: None,
            focus_handle: cx.focus_handle(),
            _project_events: project_events,
        }
    }

    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// Sets zoom and scroll, kept inside the content for the size that was last painted.
    pub fn set_viewport(&mut self, viewport: Viewport, cx: &mut Context<Self>) {
        self.refresh_order(cx);
        let (width, height) = self.painted_size.get();
        let viewport = self.clamped(viewport, width, height, cx);
        if self.viewport != viewport {
            self.viewport = viewport;
            cx.notify();
        }
    }

    fn clamped(&self, viewport: Viewport, width: f32, height: f32, cx: &App) -> Viewport {
        let extent = Extent {
            end: self.end,
            tracks: self.order.len(),
        };
        viewport.clamped(extent, self.time_signature(cx), width, height)
    }

    fn refresh_order(&mut self, cx: &App) {
        if !self.order_is_stale {
            return;
        }
        let project = self.session.read(cx).project();
        let tracks = tracks(project, self.arrangement.id());
        self.order = tracks.into_iter().map(|(track, _)| track).collect();
        self.end = end(project, self.arrangement.id()).unwrap_or_default();
        self.order_is_stale = false;
    }

    pub fn selected_clip(&self) -> Option<&InstanceId> {
        self.selected_clip.as_ref()
    }

    pub fn select_clip(&mut self, clip: Option<InstanceId>, cx: &mut Context<Self>) {
        if self.selected_clip != clip {
            self.selected_clip = clip;
            cx.notify();
        }
    }

    fn time_signature(&self, cx: &App) -> TimeSignature {
        let project = self.session.read(cx).project();
        project.project_file().tempo_map.time_signature()
    }

    /// Everything to paint into a timeline area of this size, read from the project now.
    fn scene(&self, width: f32, height: f32, cx: &App) -> Scene {
        let project = self.session.read(cx).project();
        let theme = cx.theme();
        let time_signature = self.time_signature(cx);
        // Clamped again for this size: the window may have grown since the last scroll.
        let viewport = self.clamped(self.viewport, width, height, cx);
        let visible_ticks = viewport.visible_ticks(width);

        let mut scene = Scene {
            viewport,
            clips: Vec::new(),
            rows: Vec::new(),
            bars: viewport.ruler_bars(time_signature, width).collect(),
        };
        for index in viewport.visible_tracks(height, self.order.len()) {
            let Some(track) = self.order.get(index) else {
                break;
            };
            // Gone since the order was read: the render after its event leaves it out.
            let Some(state) = project.state(track) else {
                continue;
            };
            let accent = accent(state.colour, theme);
            scene.rows.push(TrackRow {
                y: viewport.y_of(index),
                name: state.name.clone().into(),
                accent,
            });
            let first = scene.clips.len();
            for (clip, state) in project.children::<Clip>(track.id()) {
                if state.start >= visible_ticks.end || state.end() <= visible_ticks.start {
                    continue;
                }
                let rect = viewport.clip_rect(index, state);
                scene.clips.push(ClipShape {
                    notes: viewport.miniature(state, rect).collect(),
                    selected: self.selected_clip.as_ref() == Some(clip.id()),
                    start: state.start,
                    clip,
                    rect,
                    accent,
                });
            }
            // The order of `clips()`, by start and then by id, for the few that are visible:
            // it decides which of two overlapping clips is on top.
            scene.clips[first..]
                .sort_by(|a, b| (a.start, a.clip.id()).cmp(&(b.start, b.clip.id())));
        }
        scene
    }

    /// The position of a mouse event in the coordinates of [`layout`].
    fn timeline_position(bounds: Bounds<Pixels>, position: Point<Pixels>) -> (f32, f32) {
        (
            f32::from(position.x - bounds.left()) - HEADER_WIDTH,
            f32::from(position.y - bounds.top()) - RULER_HEIGHT,
        )
    }

    fn on_mouse_down(&mut self, x: f32, y: f32, scene: &Scene, cx: &mut Context<Self>) {
        if x < 0.0 {
            return;
        }
        if y < 0.0 {
            let tick = snap(scene.viewport.tick_at(x));
            self.session
                .update(cx, |session, _| session.engine().seek(tick));
            return;
        }
        let clip = scene.clip_at(x, y).map(|shape| shape.clip.id().clone());
        self.select_clip(clip, cx);
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, x: f32, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(px(32.));
        let viewport = if event.modifiers.secondary() {
            let factor = (f64::from(f32::from(delta.y)) * 0.01).exp();
            self.viewport.zoomed(factor, x.max(0.0))
        } else {
            self.viewport
                .scrolled(f32::from(delta.x), f32::from(delta.y))
        };
        self.set_viewport(viewport, cx);
    }

    fn on_pinch(&mut self, event: &PinchEvent, x: f32, cx: &mut Context<Self>) {
        let factor = f64::from(1.0 + event.delta);
        self.set_viewport(self.viewport.zoomed(factor, x.max(0.0)), cx);
    }
}

impl Focusable for Timeline {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Timeline {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh_order(cx);
        let timeline = cx.entity();
        let surface = canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |bounds, hitbox, window, cx| {
                let width = f32::from(bounds.size.width) - HEADER_WIDTH;
                let height = f32::from(bounds.size.height) - RULER_HEIGHT;
                let scene = Rc::new(timeline.read(cx).scene(width, height, cx));
                timeline.read(cx).painted.set(scene.viewport);
                timeline.read(cx).painted_size.set((width, height));
                paint_scene(&scene, bounds, window, cx);
                listen(timeline, scene, bounds, hitbox, window);
            },
        );
        div()
            .size_full()
            .track_focus(&self.focus_handle)
            .child(surface.size_full())
    }
}

/// Mouse listeners live for one frame and know what that frame painted.
fn listen(
    timeline: Entity<Timeline>,
    scene: Rc<Scene>,
    bounds: Bounds<Pixels>,
    hitbox: Hitbox,
    window: &mut Window,
) {
    window.on_mouse_event({
        let (timeline, hitbox) = (timeline.clone(), hitbox.clone());
        move |event: &MouseDownEvent, phase, window, cx| {
            let hit = phase == DispatchPhase::Bubble && hitbox.is_hovered(window);
            if hit && event.button == MouseButton::Left {
                let (x, y) = Timeline::timeline_position(bounds, event.position);
                timeline.update(cx, |timeline, cx| timeline.on_mouse_down(x, y, &scene, cx));
            }
        }
    });
    window.on_mouse_event({
        let (timeline, hitbox) = (timeline.clone(), hitbox.clone());
        move |event: &ScrollWheelEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                let (x, _) = Timeline::timeline_position(bounds, event.position);
                timeline.update(cx, |timeline, cx| timeline.on_scroll(event, x, cx));
            }
        }
    });
    window.on_mouse_event(move |event: &PinchEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
            let (x, _) = Timeline::timeline_position(bounds, event.position);
            timeline.update(cx, |timeline, cx| timeline.on_pinch(event, x, cx));
        }
    });
}

/// A rect of [`layout`] in window coordinates, on whole pixels so that edges stay sharp.
fn placed(rect: Rect, origin: Point<Pixels>) -> Bounds<Pixels> {
    let (left, top) = (rect.x.round(), rect.y.round());
    let right = (rect.x + rect.width).round().max(left + 1.0);
    let bottom = (rect.y + rect.height).round().max(top + 1.0);
    Bounds::new(
        origin + point(px(left), px(top)),
        size(px(right - left), px(bottom - top)),
    )
}

/// How a label that does not fit is handled.
enum Fit {
    /// Ends in an ellipsis at this width.
    Truncate(f32),
    /// Not painted when it would cross this x. Half a number reads as another number.
    SkipPast(Pixels),
}

fn paint_text(
    text: SharedString,
    origin: Point<Pixels>,
    font_size: f32,
    weight: FontWeight,
    color: Hsla,
    fit: Fit,
    window: &mut Window,
    cx: &mut App,
) {
    let mut font = typography::tabular();
    font.weight = weight;
    let run = TextRun {
        len: text.len(),
        font: font.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let (text, runs) = match fit {
        Fit::Truncate(width) => {
            let mut wrapper = window.text_system().line_wrapper(font, px(font_size));
            let runs = [run];
            let (text, runs) =
                wrapper.truncate_line(text, px(width), "…", &runs, TruncateFrom::End);
            (text, runs.into_owned())
        }
        Fit::SkipPast(_) => (text, vec![run]),
    };
    let line = window
        .text_system()
        .shape_line(text, px(font_size), &runs, None);
    if matches!(fit, Fit::SkipPast(right) if origin.x + line.width > right) {
        return;
    }
    let line_height = px((font_size * 1.4).round());
    // A glyph that cannot be painted leaves a gap in a label. Nothing else depends on it.
    if let Err(error) = line.paint(origin, line_height, TextAlign::Left, None, window, cx) {
        eprintln!("arrangement view: {error}");
    }
}

fn paint_scene(scene: &Scene, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let theme = cx.theme();
    let (text, muted, hairline, clip_fill, clip_border) = (
        theme.gray_900,
        theme.gray_700,
        theme.alpha_at(0.05),
        theme.alpha_at(0.05),
        theme.alpha_at(0.10),
    );
    let selection = theme.gray_950;
    let headers = Bounds::new(
        bounds.origin + point(px(0.), px(RULER_HEIGHT)),
        size(px(HEADER_WIDTH), bounds.size.height - px(RULER_HEIGHT)),
    );
    let ruler = Bounds::new(
        bounds.origin + point(px(HEADER_WIDTH), px(0.)),
        size(bounds.size.width - px(HEADER_WIDTH), px(RULER_HEIGHT)),
    );
    let timeline = Bounds::new(
        bounds.origin + point(px(HEADER_WIDTH), px(RULER_HEIGHT)),
        size(ruler.size.width, headers.size.height),
    );

    // The ruler: a short mark and a number per bar. No grid below it.
    window.with_content_mask(Some(ContentMask { bounds: ruler }), |window| {
        for (bar, x) in &scene.bars {
            let x = px(x.round());
            let mark = Bounds::new(
                ruler.origin + point(x, px(RULER_HEIGHT - 8.)),
                size(px(1.), px(8.)),
            );
            window.paint_quad(fill(mark, clip_border));
            let label = ruler.origin + point(x + px(8.), px(8.));
            let number = bar.to_string().into();
            let fit = Fit::SkipPast(ruler.right());
            paint_text(
                number,
                label,
                12.,
                FontWeight::NORMAL,
                muted,
                fit,
                window,
                cx,
            );
        }
    });

    window.with_content_mask(Some(ContentMask { bounds: headers }), |window| {
        for row in &scene.rows {
            let top = headers.origin + point(px(0.), px(row.y.round()));
            let dot = Bounds::new(
                top + point(px(24.), px(TRACK_HEIGHT / 2. - 4.)),
                size(px(8.), px(8.)),
            );
            window.paint_quad(quad(
                dot,
                px(4.),
                row.accent,
                px(0.),
                row.accent,
                BorderStyle::Solid,
            ));
            let name = top + point(px(44.), px(TRACK_HEIGHT / 2. - 10.));
            let fit = Fit::Truncate(HEADER_WIDTH - 44. - 16.);
            paint_text(
                row.name.clone(),
                name,
                14.,
                FontWeight::MEDIUM,
                text,
                fit,
                window,
                cx,
            );
        }
    });

    // One hairline under the ruler and one between the headers and the timeline.
    let under_ruler = Bounds::new(
        bounds.origin + point(px(0.), px(RULER_HEIGHT - 1.)),
        size(bounds.size.width, px(1.)),
    );
    let beside_headers = Bounds::new(
        bounds.origin + point(px(HEADER_WIDTH - 1.), px(0.)),
        size(px(1.), bounds.size.height),
    );
    window.paint_quad(fill(under_ruler, hairline));
    window.paint_quad(fill(beside_headers, hairline));

    window.with_content_mask(Some(ContentMask { bounds: timeline }), |window| {
        for shape in &scene.clips {
            let border = if shape.selected {
                selection
            } else {
                clip_border
            };
            let body = placed(shape.rect, timeline.origin);
            let radius = px(6.).min(body.size.width / 2.);
            window.paint_quad(quad(
                body,
                radius,
                clip_fill,
                px(1.),
                border,
                BorderStyle::Solid,
            ));
            for note in &shape.notes {
                window.paint_quad(fill(placed(*note, timeline.origin), shape.accent));
            }
        }
    });
}

/// The playhead: one line over the ruler and the tracks. It repaints on every playhead
/// change, so it reads nothing from the project.
struct PlayheadLine {
    playhead: Entity<Playhead>,
    painted: Rc<Cell<Viewport>>,
}

impl PlayheadLine {
    fn new(
        playhead: Entity<Playhead>,
        timeline: &Entity<Timeline>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&playhead, |_, _, cx| cx.notify()).detach();
        // A scroll or a zoom moves the line too.
        cx.observe(timeline, |_, _, cx| cx.notify()).detach();
        Self {
            playhead,
            painted: timeline.read(cx).painted.clone(),
        }
    }
}

impl Render for PlayheadLine {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tick = self.playhead.read(cx).tick;
        let painted = self.painted.clone();
        let color = cx.theme().gray_950;
        canvas(
            |_, _, _| {},
            move |bounds, (), window, _| {
                // The timeline painted before this line, so the viewport is this frame's.
                let x = painted.get().x_of(tick).round();
                if x < 0.0 || x >= f32::from(bounds.size.width) - HEADER_WIDTH {
                    return;
                }
                let top = bounds.origin + point(px(HEADER_WIDTH + x), px(RULER_HEIGHT / 2.));
                let line = Bounds::new(
                    top,
                    size(px(1.), bounds.size.height - px(RULER_HEIGHT / 2.)),
                );
                let head = Bounds::new(top - point(px(3.), px(3.)), size(px(7.), px(7.)));
                window.paint_quad(fill(line, color));
                window.paint_quad(quad(
                    head,
                    px(3.5),
                    color,
                    px(0.),
                    color,
                    BorderStyle::Solid,
                ));
            },
        )
        .absolute()
        .inset_0()
    }
}
