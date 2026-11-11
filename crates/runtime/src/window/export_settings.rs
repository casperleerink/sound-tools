//! How a piece is exported, in a small window of its own that "Export audio…" and "Export
//! selection…" open first: the format, the sample rate, the bit depth of a WAV and the
//! loudness. Export… there goes on to the save panel. The last choice is kept for this machine.

use std::path::PathBuf;
use std::rc::Rc;

use gpui::{App, Context, FocusHandle, Global, KeyDownEvent, Window, div, prelude::*, px};
use sound_export::{CEILING_DBTP, Depth, Format, Loudness, SampleRate, Settings};
use sound_ui::components::button::{Button, ButtonVariant};
use sound_ui::components::segmented_control::SegmentedControl;
use sound_ui::{ActiveTheme, typography};

use super::new_project::{small_window, small_window_root};

const WIDTH: f32 = 440.;
const HEIGHT: f32 = 300.;

/// Where the settings are kept, in the support folder of the machine. Without it, as in a test
/// window, every export starts from the defaults.
pub struct RememberedIn(pub PathBuf);

impl Global for RememberedIn {}

/// What Export… does with the settings.
pub type OnExport = Rc<dyn Fn(Settings, &mut App)>;

/// Opens the screen with the last settings, or brings the open one forward with `on_export`.
pub fn choose(title: &'static str, on_export: OnExport, cx: &mut App) {
    let open = (cx.windows().into_iter()).find_map(|window| window.downcast::<ExportSettings>());
    if let Some(open) = open {
        let shown = open.update(cx, |screen, window, _| {
            screen.on_export = on_export.clone();
            window.set_window_title(title);
            window.activate_window();
        });
        if shown.is_ok() {
            return;
        }
    }
    let settings = remembered(cx);
    let opened = cx.open_window(small_window(title, WIDTH, HEIGHT, cx), |window, cx| {
        cx.new(|cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            ExportSettings {
                depth: match settings.format {
                    Format::Wav(depth) => depth,
                    Format::Mp3 => Depth::default(),
                },
                settings,
                on_export,
                focus,
                cancel: cx.focus_handle().tab_stop(true),
            }
        })
    });
    if let Err(error) = opened {
        eprintln!("error: the window did not open: {error:#}");
    }
}

/// The settings last chosen, or the defaults when there are none or they do not read.
fn remembered(cx: &App) -> Settings {
    let file = cx.try_global::<RememberedIn>().map(|file| file.0.clone());
    let bytes = file.and_then(|file| std::fs::read(file).ok());
    bytes
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Keeps `settings` for the next export. Not keeping them loses only that.
fn remember(settings: Settings, cx: &App) {
    let Some(RememberedIn(file)) = cx.try_global::<RememberedIn>() else {
        return;
    };
    let written = serde_json::to_vec_pretty(&settings)
        .map_err(anyhow::Error::from)
        .and_then(|bytes| {
            if let Some(folder) = file.parent() {
                std::fs::create_dir_all(folder)?;
            }
            Ok(std::fs::write(file, bytes)?)
        });
    if let Err(error) = written {
        eprintln!("error: the export settings were not kept: {error:#}");
    }
}

pub struct ExportSettings {
    settings: Settings,
    /// The depth of a WAV, kept while MP3 is chosen.
    depth: Depth,
    on_export: OnExport,
    focus: FocusHandle,
    /// Return there cancels.
    cancel: FocusHandle,
}

impl ExportSettings {
    fn export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (settings, on_export) = (self.settings, self.on_export.clone());
        remember(settings, cx);
        window.remove_window();
        on_export(settings, cx);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key != "enter" {
            return;
        }
        if self.cancel.is_focused(window) {
            window.remove_window();
        } else {
            self.export(window, cx);
        }
        cx.stop_propagation();
    }

    fn set_format(&mut self, format: Format) {
        self.settings.format = match format {
            Format::Wav(_) => Format::Wav(self.depth),
            Format::Mp3 => Format::Mp3,
        };
    }

    fn set_depth(&mut self, depth: Depth) {
        self.depth = depth;
        self.settings.format = Format::Wav(depth);
    }

    fn set_rate(&mut self, rate: SampleRate) {
        self.settings.rate = rate;
    }

    fn set_loudness(&mut self, loudness: Option<Loudness>) {
        self.settings.loudness = loudness;
    }
}

/// Each choice of a row: a value for the tests to find it by, its label, and what it sets.
type Choices<T> = &'static [(&'static str, &'static str, T)];

/// A WAV of any depth is the one WAV choice of the format row.
const FORMATS: Choices<Format> = &[
    ("wav", "WAV", Format::Wav(Depth::Float)),
    ("mp3", "MP3", Format::Mp3),
];
const RATES: Choices<SampleRate> = &[
    ("44100", "44.1 kHz", SampleRate::Hz44100),
    ("48000", "48 kHz", SampleRate::Hz48000),
];
const DEPTHS: Choices<Depth> = &[
    ("16", "16-bit", Depth::Sixteen),
    ("24", "24-bit", Depth::TwentyFour),
    ("32", "32-bit float", Depth::Float),
];
const LOUDNESSES: Choices<Option<Loudness>> = &[
    ("as-it-plays", "As it plays", None),
    ("streaming", "Streaming", Some(Loudness::Streaming)),
    ("broadcast", "Broadcast", Some(Loudness::Broadcast)),
];

/// One row: what it sets, and its choices, with `chosen` selected.
fn row<T: Copy + PartialEq + 'static>(
    label: &'static str,
    chosen: T,
    choices: Choices<T>,
    set: fn(&mut ExportSettings, T),
    cx: &mut Context<ExportSettings>,
) -> impl IntoElement {
    let muted = cx.theme().gray_800;
    let value = choices.iter().find(|choice| choice.2 == chosen);
    let screen = cx.entity().downgrade();
    let control = SegmentedControl::new(label, value.map_or("", |choice| choice.0))
        .options(choices.iter().map(|(value, label, _)| (*value, *label)))
        .on_change(move |value, _, cx| {
            let Some(choice) = choices.iter().find(|choice| choice.0 == value.as_ref()) else {
                return;
            };
            // Fails only once the screen is closed.
            (screen.update(cx, |screen, cx| {
                set(screen, choice.2);
                cx.notify();
            }))
            .ok();
        });
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(div().text_color(muted).child(label))
        .child(control)
}

impl Render for ExportSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Settings {
            format,
            rate,
            loudness,
        } = self.settings;
        let format_choice = match format {
            Format::Wav(_) => Format::Wav(Depth::Float),
            Format::Mp3 => Format::Mp3,
        };
        let theme = cx.theme();
        let (background, text, muted) = (theme.gray_100, theme.gray_950, theme.gray_800);
        let mut rows = div().flex().flex_col().gap_4();
        rows = rows.child(row("Format", format_choice, FORMATS, Self::set_format, cx));
        rows = rows.child(row("Sample rate", rate, RATES, Self::set_rate, cx));
        if let Format::Wav(depth) = format {
            rows = rows.child(row("Bit depth", depth, DEPTHS, Self::set_depth, cx));
        }
        rows = rows.child(row(
            "Loudness",
            loudness,
            LOUDNESSES,
            Self::set_loudness,
            cx,
        ));
        let hint = loudness.map(|loudness| {
            let minus = |value: f64| format!("{value:.0}").replace('-', "−");
            let (lufs, peak) = (minus(loudness.lufs()), minus(CEILING_DBTP));
            format!("{lufs} LUFS, peaks under {peak} dB")
        });
        let buttons = div()
            .flex()
            .justify_end()
            .gap_2()
            .child(
                Button::new("export-cancel", "Cancel")
                    .variant(ButtonVariant::Ghost)
                    .focus_handle(&self.cancel)
                    .on_click(|_, window, _| window.remove_window()),
            )
            .child(
                Button::new("export-confirm", "Export…")
                    .variant(ButtonVariant::Primary)
                    .tab_stop(true)
                    .on_click(cx.listener(|screen, _, window, cx| screen.export(window, cx))),
            );
        small_window_root()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .flex()
            .flex_col()
            .justify_between()
            .p_6()
            .bg(background)
            .text_color(text)
            .font(typography::ui_font())
            .text_size(px(14.))
            .child(
                div().flex().flex_col().gap_2().child(rows).child(
                    div()
                        .flex()
                        .justify_end()
                        .text_size(px(12.))
                        .text_color(muted)
                        .children(hint),
                ),
            )
            .child(buttons)
    }
}
