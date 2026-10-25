//! What the composer attaches to a message: pictures pasted or dropped, and other dropped
//! files. Each shows above the text, with a button that takes it out, until it is sent.

use std::fs;
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
    /// What its chip and the sent message say: the file's name.
    pub(super) name: String,
    pub(super) attachment: Attachment,
    /// The picture, drawn small on its chip.
    preview: Option<Arc<gpui::Image>>,
    remove: FocusHandle,
}

/// An attachment as it is read, before it has its chip.
struct Read {
    name: String,
    attachment: Attachment,
    preview: Option<Arc<gpui::Image>>,
}

impl Read {
    /// `None` for a picture in a format the agent cannot see.
    fn image(name: String, image: gpui::Image) -> Option<Self> {
        let format = match image.format {
            gpui::ImageFormat::Png => ImageFormat::Png,
            gpui::ImageFormat::Jpeg => ImageFormat::Jpeg,
            gpui::ImageFormat::Gif => ImageFormat::Gif,
            gpui::ImageFormat::Webp => ImageFormat::Webp,
            _ => return None,
        };
        let bytes = image.bytes.clone();
        Some(Self {
            name,
            attachment: Attachment::Image(Image { format, bytes }),
            preview: Some(Arc::new(image)),
        })
    }

    /// A picture the agent can see is read whole; any other file goes as its path.
    fn file(path: PathBuf) -> Result<Self, String> {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        if let Some(format) = picture_format(&path) {
            let bytes =
                fs::read(&path).map_err(|error| format!("{name} could not be read: {error}"))?;
            if let Some(read) = Self::image(name.clone(), gpui::Image::from_bytes(format, bytes)) {
                return Ok(read);
            }
        }
        Ok(Self {
            name,
            attachment: Attachment::File(path),
            preview: None,
        })
    }
}

/// By the extension, as the Finder shows the file.
fn picture_format(path: &Path) -> Option<gpui::ImageFormat> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "png" => Some(gpui::ImageFormat::Png),
        "jpg" | "jpeg" => Some(gpui::ImageFormat::Jpeg),
        "gif" => Some(gpui::ImageFormat::Gif),
        "webp" => Some(gpui::ImageFormat::Webp),
        _ => None,
    }
}

impl Sidebar {
    /// Attaches files dropped or pasted, in their order once every picture is read.
    pub fn attach_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let reading =
            cx.background_spawn(async move { paths.into_iter().map(Read::file).collect() });
        cx.spawn(async move |sidebar, cx| {
            let read: Vec<Result<Read, String>> = reading.await;
            // A sidebar that went has nothing to attach to.
            sidebar
                .update(cx, |sidebar, cx| {
                    for read in read {
                        match read {
                            Ok(read) => sidebar.add(read, cx),
                            Err(error) => sidebar.conversation.notice(error),
                        }
                    }
                    sidebar.show(None, cx);
                })
                .ok();
        })
        .detach();
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
        match Read::image(PASTED_IMAGE.to_string(), image) {
            Some(read) => self.add(read, cx),
            None => {
                self.conversation
                    .notice("This kind of picture can't be sent.");
                self.show(None, cx);
            }
        }
        true
    }

    fn add(&mut self, read: Read, cx: &mut Context<Self>) {
        let Read {
            name,
            attachment,
            preview,
        } = read;
        self.attachments.push(Attached {
            name,
            attachment,
            preview,
            remove: cx.focus_handle().tab_stop(true),
        });
        cx.notify();
    }

    fn remove_attachment(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index < self.attachments.len() {
            self.attachments.remove(index);
        }
        // Its button had the focus, from the keys.
        window.focus(&self.input.focus_handle(cx), cx);
        cx.notify();
    }

    /// One chip per attachment: its picture or a clip, its name, and the button that takes it
    /// out.
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
                let picture = match &attached.preview {
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
                            .child(attached.name.clone()),
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
