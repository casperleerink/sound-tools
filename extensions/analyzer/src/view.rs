//! The card of the analyzer: the spectrum from 20 Hz to 20 kHz with the level meter at its
//! right, then the note and how far off it is, the loudness and the peak. It has nothing to set,
//! so it has no knob and nothing behind expand. The rack gives the view a [`CardFrame`]: the
//! picker of the slot as the title, and the power and close icons.
//!
//! The spectrum is a line over a fill coloured by level, as a meter is: green, then yellow
//! towards 0 dB and red at the top.
//!
//! Once per poll of the session the view reads the peaks of the analyzer and what came into its
//! scope since the last poll, works out what it shows with [`Analysis`], and draws again only
//! when that changed. A card at rest costs no frame.

use gpui::{
    Background, Bounds, ContentMask, Context, Div, Entity, FontWeight, Hsla, PathBuilder, Pixels,
    SharedString, Task, Window, canvas, div, fill, linear_color_stop, linear_gradient, point,
    prelude::*, px, size,
};
use sound_core::Instance;
use sound_ui::components::cell::Cell;
use sound_ui::components::curves::{RESPONSE_CAPTION, response_decades};
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::display::{Display, INSET_HEIGHT};
use sound_ui::components::knob::short;
use sound_ui::components::meter::Meter;
use sound_ui::{
    ActiveTheme, Devices, Metering, OfferGroup, POLL_INTERVAL, Session, Views, every_poll,
    typography, weak_action,
};

use crate::analysis::{Analysis, COLUMNS, FLOOR_DB, tuning};
use crate::{Analyzer, AnalyzerState};

/// The name the rack puts on the card of an analyzer.
pub const NAME: &str = "Analyzer";

/// The width of the display. With two columns of cells its card is 352 pt, as the limiter's.
const DISPLAY_WIDTH: f32 = 200.;
/// The meter at the right of the display, and the air around it.
const METER_INSET: f32 = 6.;
const METER_ROOM: f32 = 24.;
/// The part of the width the spectrum takes; the meter has the rest.
const SPECTRUM_WIDTH: f32 = (DISPLAY_WIDTH - METER_ROOM) / DISPLAY_WIDTH;
/// The line of the spectrum, as the curve of every display.
const LINE_WIDTH: f32 = 1.5;
/// The fill is quieter than a meter: it sits under the line.
const FILL_OPACITY: f32 = 0.45;
/// Where the fill turns from green to yellow, and from yellow to red, in dB.
const YELLOW_FROM_DB: f32 = -18.;
const RED_FROM_DB: f32 = -6.;

/// Registers the view of the `analyzer` tool, what a rack calls one and its offer in a picker.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(AnalyzerView::new);
    devices.built_in::<AnalyzerState>(
        NAME,
        OfferGroup::Mix,
        "device-analyzer",
        crate::EXTENSION,
        "This project does not load the analyzer.",
    );
}

pub struct AnalyzerView {
    session: Entity<Session>,
    analyzer: Instance<AnalyzerState>,
    frame: CardFrame,
    analysis: Analysis,
    /// What the scope gave so far, as the count to read from next, and the frames of one poll.
    read: u64,
    frames: Vec<[f32; 2]>,
    metering: Metering,
    _polling: Task<()>,
}

impl AnalyzerView {
    pub fn new(
        session: Entity<Session>,
        analyzer: Instance<AnalyzerState>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let project = session.read(cx).project();
        let sample_rate = project.sample_rate();
        // What the analyzer heard before this card was made, such as before its last silence,
        // is not what it hears now: read from the end.
        let read = project.scope(analyzer.id(), Analyzer::SCOPE);
        let read = read.map_or(0, |scope| scope.read(u64::MAX, &mut Vec::new()));
        if let Some(peaks) = project.peaks(analyzer.id(), Analyzer::PEAKS) {
            peaks.take();
        }
        Self {
            session,
            analyzer,
            frame,
            analysis: Analysis::new(sample_rate),
            read,
            frames: Vec::new(),
            metering: Metering::default(),
            _polling: every_poll(cx, Self::poll),
        }
    }

    /// Hears what came since the last poll, and draws again when that changes what the card
    /// shows.
    fn poll(&mut self, cx: &mut Context<Self>) {
        let (project, id) = (self.session.read(cx).project(), self.analyzer.id());
        let level_changed = self
            .metering
            .read(project.peaks(id, Analyzer::PEAKS).as_ref());
        if let Some(scope) = project.scope(id, Analyzer::SCOPE) {
            self.frames.clear();
            self.read = scope.read(self.read, &mut self.frames);
            self.analysis.hear(&self.frames);
        }
        let moved = self.analysis.look(POLL_INTERVAL.as_secs_f32());
        if level_changed || moved {
            cx.notify();
        }
    }

    fn display(&self, cx: &mut Context<Self>) -> Display {
        let theme = cx.theme();
        let colors = Ink {
            line: theme.gray_950,
            green: theme.green.opacity(FILL_OPACITY),
            yellow: theme.yellow.opacity(FILL_OPACITY),
            red: theme.red.opacity(FILL_OPACITY),
        };
        let levels = self.analysis.reading().spectrum;
        let spectrum = canvas(
            |_, _, _| {},
            move |bounds, (), window, _| paint_spectrum(bounds, &levels, colors, window),
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let decades = response_decades().into_iter();
        let across = decades.map(|place| place * SPECTRUM_WIDTH).collect();
        let meter = Meter::new("level", self.metering.level())
            .length(INSET_HEIGHT - 2. * METER_INSET)
            .on_clear_clip(weak_action(cx, |view: &mut Self, cx| {
                view.metering.clear_clip();
                cx.notify();
            }));
        Display::new("spectrum", DISPLAY_WIDTH)
            .grid(across, vec![height(-30.), height(-60.)])
            .caption(RESPONSE_CAPTION)
            .child(spectrum)
            .child(
                div()
                    .absolute()
                    .top(px(METER_INSET))
                    .right(px(METER_INSET))
                    .child(meter),
            )
    }
}

/// Where a level of the spectrum is up the display.
fn height(db: f32) -> f32 {
    ((db - FLOOR_DB) / -FLOOR_DB).clamp(0., 1.)
}

/// A level as every readout shows a number, or a dash for silence.
fn level(value: Option<f32>) -> String {
    match value.filter(|value| value.is_finite() && *value > FLOOR_DB) {
        Some(value) => short(value),
        None => "–".into(),
    }
}

/// The colours of the spectrum.
#[derive(Clone, Copy)]
struct Ink {
    line: Hsla,
    green: Hsla,
    yellow: Hsla,
    red: Hsla,
}

/// The spectrum on the display: a column of fill under each level and the line through them.
/// The fill of a column is cut out of bands that span the height of the display, so a colour is
/// always at the same level, as on a meter.
fn paint_spectrum(bounds: Bounds<Pixels>, levels: &[f32], ink: Ink, window: &mut Window) {
    let (left, bottom) = (bounds.origin.x, bounds.bottom());
    let full = f32::from(bounds.size.height);
    let y = |up: f32| bottom - px(up * full);
    let column = f32::from(bounds.size.width) * SPECTRUM_WIDTH / COLUMNS as f32;
    let bands: [(f32, f32, Background); 3] = [
        (0., height(YELLOW_FROM_DB), ink.green.into()),
        (
            height(YELLOW_FROM_DB),
            height(RED_FROM_DB),
            linear_gradient(
                0.,
                linear_color_stop(ink.green, 0.),
                linear_color_stop(ink.yellow, 1.),
            ),
        ),
        (
            height(RED_FROM_DB),
            1.,
            linear_gradient(
                0.,
                linear_color_stop(ink.yellow, 0.),
                linear_color_stop(ink.red, 1.),
            ),
        ),
    ];
    for (index, db) in levels.iter().enumerate() {
        let up = height(*db);
        if up <= 0. {
            continue;
        }
        let x = left + px(index as f32 * column);
        // Half a point wider, so neighbours meet with no seam.
        let width = px(column + 0.5);
        let bar = Bounds::new(point(x, y(up)), size(width, y(0.) - y(up)));
        window.with_content_mask(Some(ContentMask { bounds: bar }), |window| {
            for (from, to, background) in bands.iter().filter(|(from, ..)| *from < up) {
                let band = Bounds::new(point(x, y(*to)), size(width, y(*from) - y(*to)));
                window.paint_quad(fill(band, *background));
            }
        });
    }
    let mut line = PathBuilder::stroke(px(LINE_WIDTH));
    for (index, db) in levels.iter().enumerate() {
        let place = point(left + px((index as f32 + 0.5) * column), y(height(*db)));
        match index {
            0 => line.move_to(place),
            _ => line.line_to(place),
        }
    }
    // A path that does not tessellate paints nothing, which is all there is to do about it.
    if let Ok(line) = line.build() {
        window.paint_path(line, ink.line);
    }
}

/// A readout in the place of a control: 14 pt medium, as names are.
fn readout(text: impl Into<SharedString>, cx: &Context<AnalyzerView>) -> Div {
    div()
        .text_size(px(14.))
        .font(typography::tabular())
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().gray_950)
        .child(text.into())
}

impl Render for AnalyzerView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Deleted: whatever hosts the view takes it away then.
        if self
            .session
            .read(cx)
            .project()
            .state(&self.analyzer)
            .is_none()
        {
            return div().into_any_element();
        }
        let reading = self.analysis.reading();
        let (name, cents) = match reading.note.map(tuning) {
            Some((name, cents)) => (name, format!("{cents:+} ct").replace('-', "\u{2212}")),
            None => ("–".into(), String::new()),
        };
        let note = Cell::new(readout(name, cx)).label("Note").value(cents);
        let loudness = Cell::new(readout(level(reading.loudness), cx))
            .label("Loudness")
            .value("LUFS");
        let [left, right] = self.metering.level().peak;
        let peak = Cell::new(readout(level(Some(left.max(right))), cx))
            .label("Peak")
            .value("dB");
        self.frame
            .card()
            .display(self.display(cx))
            .column(Column::new().top(note).bottom(loudness))
            .column(Column::new().top(peak))
            .into_any_element()
    }
}
