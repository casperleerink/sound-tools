//! The spike's song page in GPUI, written as `crates/ui/README.md` says is fastest: clips and
//! notes on one `canvas`, only what shows drawn, what moves every frame (playhead, meters,
//! position) on a light layer over cached heavy views, and a 16 ms poll that notifies only what
//! changed.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use daw_model::{
    self as model, BAR, BEAT, HEADER, LEAD, MIXER, Moment, Options, PITCHES, ROW, RULER, STRIP,
    Scenario, Song, TRANSPORT, WINDOW,
};
use gpui::{
    App, Bounds, BorderStyle, Context, Entity, HeadlessAppContext, Hsla, Pixels, ScrollEvent,
    SharedString, StyleRefinement, TextAlign, TextRun, TitlebarOptions, Window, WindowBounds,
    WindowOptions, canvas, div, fill, font, point, prelude::*, px, quad, rgb, rgba, size,
};

/// Paints of the light layer, which paints in every frame that draws.
static FRAMES: AtomicU64 = AtomicU64::new(0);
/// Renders of the timeline, the heavy view.
static VIEW_RENDERS: AtomicU64 = AtomicU64::new(0);

const BACKGROUND: u32 = 0x1b1c20;
const PANEL: u32 = 0x232429;
const LINE: u32 = 0x34363d;
const TEXT: u32 = 0xe6e6ea;
const DIM: u32 = 0x8b8d96;
const ACCENT: u32 = 0x7cc7ff;

fn colour(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// `hex` (0xRRGGBB) at `alpha` (0..255).
fn faded(hex: u32, alpha: u32) -> Hsla {
    rgba(hex << 8 | alpha).into()
}

/// What every view reads: the song and where the scenario is.
struct Show {
    song: Rc<Song>,
    /// Fixed at the start, so a moved clip does not change where play wraps.
    end: u64,
    options: Options,
    moment: Moment,
    /// A level per track, then the master's.
    meters: Vec<f32>,
    edits: u64,
    /// Interface state the wheel moves; the scenarios leave it.
    scroll_y: f32,
}

impl Show {
    fn new(options: Options) -> Self {
        let song = model::song(options.tracks, options.clips);
        let end = song.end();
        Self {
            meters: vec![0.0; song.tracks.len() + 1],
            moment: model::moment(options.scenario, 0.0, end),
            song: Rc::new(song),
            end,
            options,
            edits: 0,
            scroll_y: 0.0,
        }
    }

    /// Moves the scenario to `seconds`. Says whether the timeline changed, and whether the
    /// light layer did.
    fn advance(&mut self, seconds: f64) -> (bool, bool) {
        let moment = model::moment(self.options.scenario, seconds, self.end);
        let mut timeline = moment.scroll_x != self.moment.scroll_x || moment.zoom != self.moment.zoom;
        if self.options.scenario == Scenario::Edit {
            let due = model::edits(seconds);
            while self.edits < due {
                model::apply_edit(Rc::make_mut(&mut self.song), self.edits);
                self.edits += 1;
                timeline = true;
            }
        }
        let over = timeline || moment != self.moment || moment.playing;
        if moment.playing {
            for (index, level) in self.meters.iter_mut().enumerate() {
                *level = model::meter(index, seconds);
            }
        }
        self.moment = moment;
        (timeline, over)
    }

    /// The x in the timeline of `tick`.
    fn x(&self, tick: u64) -> f32 {
        LEAD + tick as f32 * self.moment.zoom - self.moment.scroll_x
    }
}

struct Root {
    show: Entity<Show>,
    timeline: Entity<Timeline>,
    mixer: Entity<Mixer>,
    over: Entity<Over>,
}

impl Root {
    fn new(options: Options, live: bool, cx: &mut Context<Self>) -> Self {
        let show = cx.new(|_| Show::new(options));
        let timeline = cx.new(|_| Timeline { show: show.clone() });
        let mixer = cx.new(|_| Mixer { show: show.clone() });
        let over = cx.new(|_| Over { show: show.clone() });
        if live {
            let started = Instant::now();
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_millis(16)).await;
                    let seconds = started.elapsed().as_secs_f64();
                    match this.update(cx, |root, cx| root.poll(seconds, cx)) {
                        Ok(true) => {}
                        _ => break,
                    }
                }
            })
            .detach();
        }
        Self { show, timeline, mixer, over }
    }

    /// One poll, as `Session` does it: notifies only what changed. False once the run is over.
    fn poll(&mut self, seconds: f64, cx: &mut Context<Self>) -> bool {
        let options = self.show.read(cx).options;
        if options.seconds.is_some_and(|limit| seconds >= limit.as_secs_f64()) {
            println!(
                r#"{{"app":"gpui","scenario":"{}","tracks":{},"clips":{},"seconds":{:.3},"frames":{},"view_renders":{}}}"#,
                options.scenario.name(),
                options.tracks,
                options.clips,
                seconds,
                FRAMES.load(Ordering::Relaxed),
                VIEW_RENDERS.load(Ordering::Relaxed),
            );
            cx.quit();
            return false;
        }
        self.advance(seconds, cx);
        true
    }

    fn advance(&mut self, seconds: f64, cx: &mut Context<Self>) {
        let (timeline, over) = self.show.update(cx, |show, _| show.advance(seconds));
        if timeline {
            self.timeline.update(cx, |_, cx| cx.notify());
        }
        if over {
            self.over.update(cx, |_, cx| cx.notify());
        }
    }
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let transport = div()
            .h(px(TRANSPORT))
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .bg(colour(PANEL))
            .border_b_1()
            .border_color(colour(LINE))
            .child(
                div()
                    .size(px(28.))
                    .rounded(px(6.))
                    .bg(colour(ACCENT))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(colour(BACKGROUND))
                    .child("▶"),
            )
            // The position, which the light layer paints here.
            .child(div().w(px(96.)))
            .child(div().text_color(colour(DIM)).child("120 bpm"));
        let layer = StyleRefinement::default().absolute().top_0().left_0().size_full();
        div()
            .relative()
            .size_full()
            .bg(colour(BACKGROUND))
            .text_color(colour(TEXT))
            .text_size(px(12.))
            .child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(transport)
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .child(self.timeline.clone().cached(StyleRefinement::default().size_full())),
                    )
                    .child(
                        div()
                            .h(px(MIXER))
                            .child(self.mixer.clone().cached(StyleRefinement::default().size_full())),
                    ),
            )
            .child(self.over.clone().cached(layer))
    }
}

/// Ruler, track headers, bar lines, clips and notes. Renders again only when the song, the
/// scroll or the zoom changed.
struct Timeline {
    show: Entity<Show>,
}

impl Render for Timeline {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        VIEW_RENDERS.fetch_add(1, Ordering::Relaxed);
        let show = self.show.read(cx);
        let (song, moment, scroll_y, end) = (show.song.clone(), show.moment, show.scroll_y, show.end);
        let headers = song.tracks.iter().enumerate().map(|(index, track)| {
            let toggle = |label: &'static str, on: bool, lit: u32| {
                div()
                    .size(px(20.))
                    .rounded(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(10.))
                    .bg(if on { colour(lit) } else { colour(LINE) })
                    .text_color(if on { colour(BACKGROUND) } else { colour(DIM) })
                    .child(label)
            };
            div()
                .absolute()
                .left_0()
                .top(px(index as f32 * ROW - scroll_y))
                .w(px(HEADER))
                .h(px(ROW))
                .flex()
                .border_b_1()
                .border_color(colour(LINE))
                .child(div().w(px(4.)).h_full().bg(colour(track.colour)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .p_2()
                        .child(SharedString::from(track.name.clone()))
                        .child(
                            div()
                                .flex()
                                .gap_1()
                                .child(toggle("M", track.mute, 0xf0b44c))
                                .child(toggle("S", track.solo, 0x7ee0a0))
                                .child(toggle("R", track.armed, 0xff6b6b)),
                        ),
                )
        });
        let headers: Vec<_> = headers.collect();
        let ruler = canvas(
            |_, _, _| {},
            move |bounds, (), window, cx| paint_ruler(bounds, moment, end, window, cx),
        )
        .size_full();
        let clips = canvas(
            |_, _, _| {},
            move |bounds, (), window, cx| paint_clips(bounds, &song, moment, end, scroll_y, window, cx),
        )
        .size_full();
        let show = self.show.clone();
        div()
            .id("timeline")
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(RULER))
                    .flex()
                    .border_b_1()
                    .border_color(colour(LINE))
                    .child(div().w(px(HEADER)).h_full().bg(colour(PANEL)))
                    .child(div().flex_1().h_full().child(ruler)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .overflow_hidden()
                    .on_scroll(move |event: &ScrollEvent, window, cx| {
                        let delta = event.delta.pixel_delta(px(ROW)).y;
                        show.update(cx, |show, cx| {
                            let tracks = show.song.tracks.len() as f32 * ROW;
                            show.scroll_y = (show.scroll_y - f32::from(delta)).clamp(0.0, tracks.max(0.0));
                            cx.notify();
                        });
                        window.refresh();
                    })
                    .child(
                        div()
                            .relative()
                            .w(px(HEADER))
                            .h_full()
                            .bg(colour(PANEL))
                            .overflow_hidden()
                            .children(headers),
                    )
                    .child(div().flex_1().h_full().child(clips)),
            )
    }
}

fn paint_ruler(bounds: Bounds<Pixels>, moment: Moment, end: u64, window: &mut Window, cx: &mut App) {
    let at = |tick: u64| LEAD + tick as f32 * moment.zoom - moment.scroll_x;
    let width = f32::from(bounds.size.width);
    let (left, top, height) = (bounds.origin.x, bounds.origin.y, f32::from(bounds.size.height));
    window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
        let mut beat = 0;
        while beat <= end {
            let x = at(beat);
            if x > width {
                break;
            }
            if x >= -40.0 {
                let bar = beat % BAR == 0;
                let tall = if bar { height } else { 6.0 };
                window.paint_quad(fill(
                    Bounds::new(point(left + px(x), top + px(height - tall)), size(px(1.), px(tall))),
                    colour(if bar { DIM } else { LINE }),
                ));
                if bar {
                    let label: SharedString = format!("{}", beat / BAR + 1).into();
                    paint_text(label, point(left + px(x + 4.), top + px(6.)), 11., colour(DIM), window, cx);
                }
            }
            beat += BEAT;
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn paint_clips(
    bounds: Bounds<Pixels>,
    song: &Song,
    moment: Moment,
    end: u64,
    scroll_y: f32,
    window: &mut Window,
    cx: &mut App,
) {
    let at = |tick: u64| LEAD + tick as f32 * moment.zoom - moment.scroll_x;
    let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let origin = bounds.origin;
    let (low, high) = (f32::from(PITCHES.0), f32::from(PITCHES.1));
    window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
        // Bar lines.
        let mut bar = 0;
        while bar <= end {
            let x = at(bar);
            if x > width {
                break;
            }
            if x >= 0.0 {
                window.paint_quad(fill(
                    Bounds::new(origin + point(px(x), px(0.)), size(px(1.), px(height))),
                    colour(LINE),
                ));
            }
            bar += BAR;
        }
        for (row, track) in song.tracks.iter().enumerate() {
            let top = row as f32 * ROW - scroll_y;
            if top + ROW < 0.0 || top > height {
                continue;
            }
            window.paint_quad(fill(
                Bounds::new(origin + point(px(0.), px(top + ROW - 1.)), size(px(width), px(1.))),
                colour(LINE),
            ));
            for clip in &track.clips {
                let (left, right) = (at(clip.start), at(clip.start + clip.length));
                if right < 0.0 || left > width {
                    continue;
                }
                let area = Bounds::new(
                    origin + point(px(left), px(top + 2.)),
                    size(px(right - left - 1.), px(ROW - 5.)),
                );
                window.paint_quad(quad(
                    area,
                    px(4.),
                    faded(track.colour, 0x40),
                    px(1.),
                    colour(track.colour),
                    BorderStyle::Solid,
                ));
                if right - left > 36.0 {
                    let name: SharedString = clip.name.clone().into();
                    paint_text(name, origin + point(px(left + 6.), px(top + 4.)), 10., colour(TEXT), window, cx);
                }
                let span = ROW - 26.0;
                for note in &clip.notes {
                    let x = at(clip.start + note.start);
                    let w = (note.length as f32 * moment.zoom).max(1.0).min(right - x - 1.0);
                    if x + w < 0.0 || x > width || w <= 0.0 {
                        continue;
                    }
                    let y = top + 18.0 + (1.0 - (f32::from(note.pitch) - low) / (high - low)) * span;
                    window.paint_quad(fill(
                        Bounds::new(origin + point(px(x), px(y)), size(px(w), px(2.))),
                        colour(track.colour),
                    ));
                }
            }
        }
    });
}

fn paint_text(text: SharedString, at: gpui::Point<Pixels>, size: f32, color: Hsla, window: &mut Window, cx: &mut App) {
    let run = TextRun {
        len: text.len(),
        font: font(".SystemUIFont"),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window.text_system().shape_line(text, px(size), &[run], None);
    if let Err(error) = line.paint(at, px(size * 1.3), TextAlign::Left, None, window, cx) {
        eprintln!("could not paint text: {error}");
    }
}

/// A strip per track and one for the master: name and fader. The meters are on the light layer.
struct Mixer {
    show: Entity<Show>,
}

impl Render for Mixer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let song = self.show.read(cx).song.clone();
        let strips = (song.tracks.iter().map(|t| (t.name.clone(), t.gain_db, t.colour)))
            .chain([("Master".to_string(), 0.0, TEXT)])
            .map(|(name, gain_db, tint)| {
                let travel = MIXER - 80.0;
                let thumb = (1.0 - ((gain_db + 60.0) / 66.0).clamp(0.0, 1.0)) * travel;
                div()
                    .w(px(STRIP))
                    .h_full()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_2()
                    .py_2()
                    .border_r_1()
                    .border_color(colour(LINE))
                    .child(div().w(px(STRIP - 12.)).h(px(3.)).rounded(px(2.)).bg(colour(tint)))
                    .child(div().text_size(px(11.)).child(SharedString::from(name)))
                    .child(
                        div()
                            .relative()
                            .w(px(24.))
                            .h(px(travel))
                            .child(div().absolute().left(px(10.)).top_0().w(px(4.)).h_full().rounded(px(2.)).bg(colour(LINE)))
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top(px(thumb))
                                    .w(px(24.))
                                    .h(px(10.))
                                    .rounded(px(3.))
                                    .bg(colour(TEXT)),
                            ),
                    )
            });
        div()
            .size_full()
            .flex()
            .bg(colour(PANEL))
            .border_t_1()
            .border_color(colour(LINE))
            .overflow_hidden()
            .children(strips)
    }
}

/// What moves every frame, over the whole window: the playhead, the position and every meter.
struct Over {
    show: Entity<Show>,
}

impl Render for Over {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let show = self.show.read(cx);
        let playhead = show.x(show.moment.playhead);
        let position: SharedString = model::position(show.moment.playhead).into();
        let meters = show.meters.clone();
        let (tracks, scroll_y) = (show.song.tracks.len(), show.scroll_y);
        canvas(
            |_, _, _| {},
            move |bounds, (), window, cx| {
                FRAMES.fetch_add(1, Ordering::Relaxed);
                let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                let mixer_top = height - MIXER;
                let rows_top = TRANSPORT + RULER;
                paint_text(position, point(px(52.), px(14.)), 13., colour(TEXT), window, cx);
                let meter = |window: &mut Window, x: f32, top: f32, tall: f32, wide: f32, level: f32| {
                    window.paint_quad(fill(Bounds::new(point(px(x), px(top)), size(px(wide), px(tall))), colour(LINE)));
                    let lit = tall * level.clamp(0.0, 1.0);
                    window.paint_quad(fill(
                        Bounds::new(point(px(x), px(top + tall - lit)), size(px(wide), px(lit))),
                        colour(if level > 0.9 { 0xff6b6b } else { 0x7ee0a0 }),
                    ));
                };
                let rows = Bounds::new(point(px(0.), px(rows_top)), size(px(width), px(mixer_top - rows_top)));
                window.with_content_mask(Some(gpui::ContentMask { bounds: rows }), |window| {
                    for (row, level) in meters.iter().take(tracks).enumerate() {
                        let top = rows_top + row as f32 * ROW - scroll_y + 8.0;
                        meter(window, HEADER - 8.0, top, ROW - 16.0, 3.0, *level);
                    }
                });
                for (index, level) in meters.iter().enumerate() {
                    let x = index as f32 * STRIP + STRIP - 18.0;
                    if x > width {
                        break;
                    }
                    let (top, tall) = (mixer_top + 40.0, MIXER - 80.0);
                    meter(window, x, top, tall, 4.0, *level);
                    meter(window, x + 6.0, top, tall, 4.0, *level * 0.92);
                }
                let x = HEADER + playhead;
                if x >= HEADER && x <= width {
                    window.paint_quad(fill(
                        Bounds::new(point(px(x), px(TRANSPORT)), size(px(1.), px(mixer_top - TRANSPORT))),
                        colour(ACCENT),
                    ));
                }
            },
        )
        .size_full()
    }
}

fn value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let index = args.iter().position(|arg| arg == name)?;
    args.get(index + 1).map(String::as_str)
}

/// `--screenshot out.png [--at seconds]`: the window at `seconds` into the scenario, drawn
/// offscreen, as `crates/runtime/src/screenshot.rs` does.
fn screenshot(options: Options, png: &str, at: f64) -> anyhow::Result<()> {
    let text_system = gpui_platform::current_platform(true).text_system();
    let mut cx = HeadlessAppContext::with_platform(text_system, Arc::new(()), gpui_platform::current_headless_renderer);
    let window = cx.open_window(size(px(WINDOW.0), px(WINDOW.1)), |_, cx| {
        cx.new(|cx| Root::new(options, false, cx))
    })?;
    cx.run_until_parked();
    cx.update_window(window.into(), |root, window, cx| {
        if let Ok(root) = root.downcast::<Root>() {
            root.update(cx, |root, cx| root.advance(at, cx));
        }
        window.refresh();
    })?;
    cx.run_until_parked();
    cx.capture_screenshot(window.into())?.save(png)?;
    println!("wrote {png}");
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let options = Options::parse(args.iter().cloned());
    if let Some(png) = value(&args, "--screenshot") {
        let at = value(&args, "--at").and_then(|at| at.parse().ok()).unwrap_or(1.0);
        return screenshot(options, png, at);
    }
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(WINDOW.0), px(WINDOW.1)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions { title: Some("GPUI DAW spike".into()), ..Default::default() }),
                ..Default::default()
            },
            |_, cx| cx.new(|cx| Root::new(options, true, cx)),
        );
        if let Err(error) = opened {
            eprintln!("could not open the window: {error}");
            cx.quit();
        }
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
    Ok(())
}
