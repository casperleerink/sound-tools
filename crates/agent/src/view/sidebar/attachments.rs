//! What the composer attaches to a message: pictures pasted or dropped, and other dropped
//! files. Each shows above the text, with a button that takes it out, until it is sent.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{
    AnyElement, ClipboardEntry, Context, FocusHandle, Focusable, ObjectFit, Window, div, img,
    prelude::*, px,
};
use sound_ui::ActiveTheme;
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::icon::Icon;

use super::Sidebar;
use crate::{Attachment, Image, ImageFormat};

/// What a pasted picture is called, as it has no file.
const PASTED_IMAGE: &str = "Pasted image";

pub(super) struct Attached {
    pub(super) read: Read,
    remove: FocusHandle,
}

/// An attachment as it is read, before it has its chip.
pub(super) struct Read {
    /// What its chip and the thread show.
    pub(super) name: String,
    pub(super) attachment: Attachment,
    /// The picture, drawn small on its chip.
    preview: Option<Arc<gpui::Image>>,
}

impl Read {
    fn image(name: String, format: ImageFormat, bytes: Vec<u8>) -> Self {
        let preview = gpui::Image::from_bytes(gpui_format(format), bytes.clone());
        Self {
            name,
            attachment: Attachment::Image(Image { format, bytes }),
            preview: Some(Arc::new(preview)),
        }
    }

    /// A picture the agent can see is read whole; any other file goes as its path.
    fn file(path: PathBuf) -> Result<Self, String> {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let Some(format) = picture_format(&path) else {
            return Ok(Self {
                name,
                attachment: Attachment::File(path),
                preview: None,
            });
        };
        let bytes =
            fs::read(&path).map_err(|error| format!("{name} could not be read: {error}"))?;
        Ok(Self::image(name, format, bytes))
    }

    /// A picture in another format, such as a screenshot on the clipboard of macOS or
    /// Windows, goes as PNG.
    fn pasted(image: gpui::Image) -> Result<Self, String> {
        let name = PASTED_IMAGE.to_string();
        if let Some(format) = sendable(image.format) {
            return Ok(Self::image(name, format, image.bytes));
        }
        let mut png = Vec::new();
        image::load_from_memory(&image.bytes)
            .and_then(|decoded| {
                decoded.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            })
            .map_err(|_| "This kind of picture can't be sent.".to_string())?;
        Ok(Self::image(name, ImageFormat::Png, png))
    }
}

/// By the extension, as the Finder shows the file.
fn picture_format(path: &Path) -> Option<ImageFormat> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "png" => Some(ImageFormat::Png),
        "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
        "gif" => Some(ImageFormat::Gif),
        "webp" => Some(ImageFormat::Webp),
        _ => None,
    }
}

fn sendable(format: gpui::ImageFormat) -> Option<ImageFormat> {
    match format {
        gpui::ImageFormat::Png => Some(ImageFormat::Png),
        gpui::ImageFormat::Jpeg => Some(ImageFormat::Jpeg),
        gpui::ImageFormat::Gif => Some(ImageFormat::Gif),
        gpui::ImageFormat::Webp => Some(ImageFormat::Webp),
        _ => None,
    }
}

fn gpui_format(format: ImageFormat) -> gpui::ImageFormat {
    match format {
        ImageFormat::Png => gpui::ImageFormat::Png,
        ImageFormat::Jpeg => gpui::ImageFormat::Jpeg,
        ImageFormat::Gif => gpui::ImageFormat::Gif,
        ImageFormat::Webp => gpui::ImageFormat::Webp,
    }
}

impl Sidebar {
    /// Attaches files dropped or pasted, in their order once every picture is read.
    pub fn attach_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.read_attachments(move || paths.into_iter().map(Read::file).collect(), cx);
    }

    /// Reads in the background. No message goes until the read is in, so nothing slips into
    /// the message after.
    fn read_attachments(
        &mut self,
        read: impl FnOnce() -> Vec<Result<Read, String>> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.attachments_reading += 1;
        let reading = cx.background_spawn(async move { read() });
        cx.spawn(async move |sidebar, cx| {
            let read = reading.await;
            // A sidebar that went has nothing to attach to.
            sidebar
                .update(cx, |sidebar, cx| {
                    sidebar.attachments_reading -= 1;
                    for read in read {
                        match read {
                            Ok(read) => sidebar.attachments.push(Attached {
                                read,
                                remove: cx.focus_handle().tab_stop(true),
                            }),
                            Err(error) => sidebar.conversation.notice(error),
                        }
                    }
                    sidebar.show(None, cx);
                })
                .ok();
        })
        .detach();
        cx.notify();
    }

    /// A paste of copied files or of a picture attaches them, and gives `true`. Text is the
    /// field's to paste.
    pub(super) fn paste_attachments(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(item) = cx.read_from_clipboard() else {
            return false;
        };
        let (mut paths, mut image, mut text) = (Vec::new(), None, false);
        for entry in item.into_entries() {
            match entry {
                ClipboardEntry::ExternalPaths(external) => {
                    paths.extend_from_slice(external.paths())
                }
                ClipboardEntry::Image(picture) => image = Some(picture),
                ClipboardEntry::String(_) => text = true,
            }
        }
        if !paths.is_empty() {
            self.attach_files(paths, cx);
            return true;
        }
        let Some(image) = image.filter(|_| !text) else {
            return false;
        };
        self.read_attachments(move || vec![Read::pasted(image)], cx);
        true
    }

    fn remove_attachment(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index < self.attachments.len() {
            self.attachments.remove(index);
        }
        // Its button had the focus, from the keys.
        window.focus(&self.input.focus_handle(cx), cx);
        cx.notify();
    }

    pub(super) fn attachment_chips(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.attachments.is_empty() {
            return None;
        }
        let theme = cx.theme();
        let (fill, muted) = (theme.alpha_at(0.05), theme.gray_700);
        let chips = self
            .attachments
            .iter()
            .enumerate()
            .map(|(index, attached)| {
                let picture = match &attached.read.preview {
                    Some(preview) => img(preview.clone())
                        .size(px(20.))
                        .rounded(px(4.))
                        .object_fit(ObjectFit::Cover)
                        .into_any_element(),
                    None => Icon::new("paperclip")
                        .size(14.)
                        .color(muted)
                        .into_any_element(),
                };
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .min_w_0()
                    .h(px(28.))
                    .pl(px(6.))
                    .pr(px(2.))
                    .rounded(px(8.))
                    .bg(fill)
                    .child(picture)
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .child(attached.read.name.clone()),
                    )
                    .child(
                        Button::icon_only(("remove-attachment", index), "x")
                            .debug_selector(move || format!("agent-remove-attachment-{index}"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Xs)
                            .focus_handle(&attached.remove)
                            .on_click(cx.listener(move |sidebar, _, window, cx| {
                                sidebar.remove_attachment(index, window, cx);
                            })),
                    )
            });
        Some(
            div()
                .flex()
                .flex_wrap()
                .gap(px(6.))
                .children(chips)
                .into_any_element(),
        )
    }
}
