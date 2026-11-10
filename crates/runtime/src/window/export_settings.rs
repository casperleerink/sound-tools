//! How a piece is exported, in a small window of its own that "Export audio…" and "Export
//! selection…" open first: the format, the sample rate, the bit depth of a WAV and the
//! loudness. Export… there goes on to the save panel. The last choice is kept for this machine.

use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    App, Context, FocusHandle, Global, KeyDownEvent, SharedString, TitlebarOptions, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use sound_export::{Depth, Format, Loudness, SampleRate, Settings};
use sound_ui::components::button::{Button, ButtonVariant};
use sound_ui::components::segmented_control::SegmentedControl;
use sound_ui::{ActiveTheme, typography};

const WIDTH: f32 = 440.;
const HEIGHT: f32 = 300.;

/// Where the settings are kept, in the support folder of the machine. Without it, as in a test
/// window, every export starts from the defaults.
pub struct RememberedIn(pub PathBuf);

impl Global for RememberedIn {}

/// What Export… does with the settings.
pub type OnExport = Rc<dyn Fn(Settings, &mut App)>;

/// Opens the screen with the last settings, or brings the open one forward with `on_export`.
pub fn choose(title: &str, on_export: OnExport, cx: &mut App) {
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
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(gpui::Bounds::centered(
            None,
            size(px(WIDTH), px(HEIGHT)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(SharedString::from(title.to_string())),
            ..Default::default()
        }),
        app_id: Some(crate::app::TOOL_NAME.into()),
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
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

/// Keeps `settings` for the next export, off the UI thread. Not keeping them loses only that.
fn remember(settings: Settings, cx: &mut App) {
    let Some(file) = cx.try_global::<RememberedIn>().map(|file| file.0.clone()) else {
        return;
    };
    cx.background_spawn(async move {
        let written = serde_json::to_vec_pretty(&settings)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| {
                if let Some(folder) = file.parent() {
                    std::fs::create_dir_all(folder)?;
                }
                Ok(std::fs::write(&file, bytes)?)
            });
        if let Err(error) = written {
            eprintln!("error: the export settings were not kept: {error:#}");
        }
    })
    .detach();
}

pub struct ExportSettings {
    settings: Settings,
    /// The depth of a WAV, kept while MP3 is chosen.
    depth: Depth,
    on_export: OnExport,
    focus: FocusHandle,
}

impl ExportSettings {
    fn export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (settings, on_export) = (self.settings, self.on_export.clone());
        remember(settings, cx);
        window.remove_window();
        on_export(settings, cx);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => window.remove_window(),
            "enter" => self.export(window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn set(&mut self, value: &str, cx: &mut Context<Self>) {
        let settings = &mut self.settings;
        match value {
            "wav" => settings.format = Format::Wav(self.depth),
            "mp3" => settings.format = Format::Mp3,
            "44100" => settings.rate = SampleRate::Hz44100,
            "48000" => settings.rate = SampleRate::Hz48000,
            "as-it-plays" => settings.loudness = None,
            "streaming" => settings.loudness = Some(Loudness::Streaming),
            "broadcast" => settings.loudness = Some(Loudness::Broadcast),
            depth => {
                self.depth = match depth {
                    "16" => Depth::Sixteen,
                    "24" => Depth::TwentyFour,
                    _ => Depth::Float,
                };
                settings.format = Format::Wav(self.depth);
            }
        }
        cx.notify();
    }
}

/// One row: what it sets, and the choices.
fn row(
    label: &'static str,
    id: &'static str,
    value: &'static str,
    options: &[(&'static str, &'static str)],
    cx: &mut Context<ExportSettings>,
) -> impl IntoElement {
    let muted = cx.theme().gray_800;
    let screen = cx.entity().downgrade();
    let control = SegmentedControl::new(id, value)
        .options(options.iter().copied())
        .on_change(move |value, _, cx| {
            // Fails only once the screen is closed.
            screen.update(cx, |screen, cx| screen.set(&value, cx)).ok();
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
        let settings = self.settings;
        let format = match settings.format {
            Format::Wav(_) => "wav",
            Format::Mp3 => "mp3",
        };
        let rate = match settings.rate {
            SampleRate::Hz44100 => "44100",
            SampleRate::Hz48000 => "48000",
        };
        let depth = match self.depth {
            Depth::Sixteen => "16",
            Depth::TwentyFour => "24",
            Depth::Float => "32",
        };
        let loudness = match settings.loudness {
            None => "as-it-plays",
            Some(Loudness::Streaming) => "streaming",
            Some(Loudness::Broadcast) => "broadcast",
        };
        let theme = cx.theme();
        let (background, text, muted) = (theme.gray_100, theme.gray_950, theme.gray_800);
        let mut rows = div().flex().flex_col().gap_4().child(row(
            "Format",
            "export-format",
            format,
            &[("wav", "WAV"), ("mp3", "MP3")],
            cx,
        ));
        rows = rows.child(row(
            "Sample rate",
            "export-rate",
            rate,
            &[("44100", "44.1 kHz"), ("48000", "48 kHz")],
            cx,
        ));
        // An MP3 has no bit depth to choose.
        if format == "wav" {
            rows = rows.child(row(
                "Bit depth",
                "export-depth",
                depth,
                &[("16", "16-bit"), ("24", "24-bit"), ("32", "32-bit float")],
                cx,
            ));
        }
        rows = rows.child(row(
            "Loudness",
            "export-loudness",
            loudness,
            &[
                ("as-it-plays", "As it plays"),
                ("streaming", "Streaming"),
                ("broadcast", "Broadcast"),
            ],
            cx,
        ));
        let hint = match settings.loudness {
            None => "",
            Some(Loudness::Streaming) => "−14 LUFS, peaks under −1 dB",
            Some(Loudness::Broadcast) => "−23 LUFS, peaks under −1 dB",
        };
        let buttons = div()
            .flex()
            .justify_end()
            .gap_2()
            .child(
                Button::new("export-cancel", "Cancel")
                    .variant(ButtonVariant::Ghost)
                    .on_click(|_, window, _| window.remove_window()),
            )
            .child(
                Button::new("export-confirm", "Export…")
                    .variant(ButtonVariant::Primary)
                    .on_click(cx.listener(|screen, _, window, cx| screen.export(window, cx))),
            );
        div()
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
                        .child(hint),
                ),
            )
            .child(buttons)
    }
}
