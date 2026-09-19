//! Renders every gallery section to a PNG with no visible window.
//! `cargo test -p gallery --test snapshots` writes to `$GALLERY_SNAPSHOT_DIR`, default
//! `<target>/gallery-snapshots`. `GALLERY_SECTION=inputs` renders one section only.

use std::{path::PathBuf, sync::Arc};

use gallery::{Gallery, SECTIONS};
use gpui::{AppContext, HeadlessAppContext, px, size};
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
    cx.update(sound_ui::init);

    for section in SECTIONS
        .iter()
        .filter(|s| only.as_deref().is_none_or(|o| o == **s))
    {
        // Tall enough for the longest section; empty rows are trimmed below.
        let window = cx.open_window(size(px(1440.), px(4000.)), |_, cx| {
            cx.new(|_| Gallery::new(Some(section.to_string())))
        })?;
        cx.run_until_parked();
        let image = cx.capture_screenshot(window.into())?;
        let path = out_dir.join(format!("{section}.png"));
        trim_bottom(image).save(&path)?;
        println!("wrote {}", path.display());
    }
    Ok(())
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
