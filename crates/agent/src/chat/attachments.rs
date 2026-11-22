//! What the composer attaches to a message: pictures pasted or dropped, and other dropped
//! files. Each shows above the text, with a button that takes it out, until it is sent.

use std::fs;
use std::path::{Path, PathBuf};

use super::{Chat, Done};
use crate::jobs;
use crate::{Attachment, Image, ImageFormat};

/// What a pasted picture is called, as it has no file.
pub const PASTED_IMAGE: &str = "Pasted image";

/// One attachment of the next message, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attached {
    /// What its chip and the thread show.
    pub name: String,
    pub attachment: Attachment,
}

impl Attached {
    /// The picture its chip shows small, or none for a file that goes as its path.
    pub fn picture(&self) -> Option<&Image> {
        match &self.attachment {
            Attachment::Image(image) => Some(image),
            Attachment::File(_) => None,
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
            });
        };
        let bytes =
            fs::read(&path).map_err(|error| format!("{name} could not be read: {error}"))?;
        Ok(Self {
            name,
            attachment: Attachment::Image(Image { format, bytes }),
        })
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

impl Chat {
    /// What goes with the next message, in the order it came.
    pub fn attachments(&self) -> &[Attached] {
        &self.attachments
    }

    /// Whether a file dropped or pasted is still read: no message goes until it is in, so
    /// nothing slips into the message after.
    pub fn is_reading_attachments(&self) -> bool {
        self.attachments_reading > 0
    }

    /// Attaches files dropped on the composer or pasted, in their order, once every picture
    /// among them is read.
    pub fn attach_files(&mut self, paths: Vec<PathBuf>) {
        self.attachments_reading += 1;
        let outbox = self.inbox.outbox();
        // Detached: what it reads after the chat went goes nowhere.
        jobs::spawn(async move {
            let read = smol::unblock(move || paths.into_iter().map(Attached::file).collect());
            outbox.send(Done::Attached(read.await));
        })
        .detach();
    }

    /// Attaches a picture pasted as PNG. A paste of text is the composer's own.
    pub fn attach_picture(&mut self, png: Vec<u8>) {
        self.attachments.push(Attached {
            name: PASTED_IMAGE.to_string(),
            attachment: Attachment::Image(Image {
                format: ImageFormat::Png,
                bytes: png,
            }),
        });
    }

    pub(super) fn attached(&mut self, read: Vec<Result<Attached, String>>) {
        self.attachments_reading = self.attachments_reading.saturating_sub(1);
        for read in read {
            match read {
                Ok(attached) => self.attachments.push(attached),
                Err(error) => self.conversation.notice(error),
            }
        }
    }

    /// The × of an attachment's chip. The composer has the keys after.
    pub fn remove_attachment(&mut self, index: usize) {
        if index < self.attachments.len() {
            self.attachments.remove(index);
        }
    }
}
