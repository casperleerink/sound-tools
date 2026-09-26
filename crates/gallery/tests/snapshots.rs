//! Renders every gallery section to a PNG with no visible window.
//! `cargo test -p gallery --test snapshots` writes to `$GALLERY_SNAPSHOT_DIR`, default
//! `<target>/gallery-snapshots`. `GALLERY_SECTION=rack` renders one section only.
//!
//! The focus section is rendered once per tab, since one window has one focus: `focus.png`
//! stacks the frames, each with the focus from the keyboard on the next control.

use std::{path::PathBuf, sync::Arc};

use gallery::{Gallery, SECTIONS};
use gpui::{AppContext, HeadlessAppContext, KeyDownEvent, Keystroke, PlatformInput, px, size};
use sound_ui::Assets;

fn main() -> anyhow::Result<()> {
    let out_dir = std::env::var("GALLERY_SNAPSHOT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("../gallery-snapshots")
        });
    std::fs::create_dir_all(&out_dir)?;
    let only = std::env::var("GALLERY_SECTION").ok();

    let text_system = gpui_platform::current_platform(true).text_system();
    let mut cx = HeadlessAppContext::with_platform(
        text_system,
        Arc::new(Assets),
        gpui_platform::current_headless_renderer,
    );
    cx.update(|cx| {
        sound_ui::init(cx);
        gallery::init(cx);
    });

    for section in SECTIONS
        .iter()
        .filter(|s| only.as_deref().is_none_or(|o| o == **s))
    {
        // Tall enough for the longest section; empty rows are trimmed below.
        let window = cx.open_window(size(px(1440.), px(4000.)), |window, cx| {
            cx.new(|cx| Gallery::new(Some(section.to_string()), window, cx))
        })?;
        cx.run_until_parked();
        let image = match *section {
            "focus" => {
                let mut frames = Vec::new();
                for _ in 0..FOCUS_STOPS {
                    let tab = PlatformInput::KeyDown(KeyDownEvent {
                        keystroke: Keystroke::parse("tab")?,
                        is_held: false,
                        prefer_character_input: false,
                    });
                    cx.update_window(window.into(), |_, window, cx| {
                        window.dispatch_event(tab, cx);
                    })?;
                    cx.run_until_parked();
                    frames.push(trim_bottom(cx.capture_screenshot(window.into())?));
                }
                stack(frames)
            }
            _ => trim_bottom(cx.capture_screenshot(window.into())?),
        };
        let path = out_dir.join(format!("{section}.png"));
        image.save(&path)?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

/// The controls of the focus section that tab reaches, one frame each.
const FOCUS_STOPS: usize = 6;

/// The frames one under the other.
fn stack(frames: Vec<image::RgbaImage>) -> image::RgbaImage {
    let width = frames.iter().map(|frame| frame.width()).max().unwrap_or(0);
    let height = frames.iter().map(|frame| frame.height()).sum();
    let mut out = image::RgbaImage::new(width, height);
    let mut top = 0;
    for frame in frames {
        image::imageops::replace(&mut out, &frame, 0, i64::from(top));
        top += frame.height();
    }
    out
}

/// Crops rows at the bottom that match the background colour.
fn trim_bottom(image: image::RgbaImage) -> image::RgbaImage {
    let (width, height) = image.dimensions();
    let background = *image.get_pixel(width - 1, height - 1);
    let last_row = (0..height)
        .rev()
        .find(|&y| (0..width).any(|x| *image.get_pixel(x, y) != background))
        .unwrap_or(0);
    let keep = (last_row + 80).min(height);
    image::imageops::crop_imm(&image, 0, 0, width, keep).to_image()
}
