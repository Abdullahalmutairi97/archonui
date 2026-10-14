//! Files attached to a message, and how each agent receives them.
//!
//! Agents take some files natively (images, text, PDFs for Claude). Anything
//! else is listed in the prompt by path for the agent to open with its own
//! tools; `Delivery::by_path` says which.

use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
};

/// Largest file ArchonUI accepts. Every agent read a 65 MiB image in testing
/// (they downscale images themselves), so this is the tested ceiling.
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Most files per message.
pub const MAX_FILES: usize = 10;
/// Claude Code silently drops PDFs above roughly 17.6 MB (tested); larger
/// ones are sent by path instead. Applies to the total of a message's PDFs.
pub const CLAUDE_PDF_BYTES: u64 = 16 * 1024 * 1024;
/// Text files up to this size are inlined; larger ones are sent by path.
pub const TEXT_INLINE_BYTES: u64 = 512 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Image,
    Pdf,
    Text,
    Other,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attachment {
    pub path: PathBuf,
    pub name: String,
    pub kind: Kind,
    pub size: u64,
}

impl Attachment {
    /// Inspect a file. Errors are user-facing.
    pub fn inspect(path: &Path) -> Result<Self, String> {
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into());
        let meta = path.metadata().map_err(|e| format!("{name}: {e}"))?;
        if !meta.is_file() {
            return Err(format!("{name} is not a file."));
        }
        let size = meta.len();
        if size > MAX_FILE_BYTES {
            return Err(format!(
                "{name} is {}; the limit is {} per file.",
                human_size(size),
                human_size(MAX_FILE_BYTES)
            ));
        }
        let kind = sniff(path).map_err(|e| format!("{name}: {e}"))?;
        Ok(Self { path: path.to_path_buf(), name, kind, size })
    }

    pub fn media_type(&self) -> &'static str {
        match self.kind {
            Kind::Pdf => "application/pdf",
            Kind::Text => "text/plain",
            Kind::Other => "application/octet-stream",
            // The kind was sniffed from the content, so the type is too: a JPEG saved as `.png` (or with no
            // extension) must not be labelled PNG, which Claude rejects. The extension is the fallback.
            Kind::Image => image_type(&head(&self.path).unwrap_or_default()).unwrap_or_else(|| {
                match self.path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref() {
                    Some("jpg" | "jpeg") => "image/jpeg",
                    Some("gif") => "image/gif",
                    Some("webp") => "image/webp",
                    _ => "image/png",
                }
            }),
        }
    }
}

/// The first bytes of `path`, enough to sniff its type.
fn head(path: &Path) -> io::Result<Vec<u8>> {
    let mut head = Vec::with_capacity(8192);
    File::open(path)?.take(8192).read_to_end(&mut head)?;
    Ok(head)
}

/// The image type `head` starts with, if it is one ArchonUI sends natively.
fn image_type(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if head.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Classify by content, not extension.
fn sniff(path: &Path) -> io::Result<Kind> {
    let head = head(path)?;
    Ok(if image_type(&head).is_some() {
        Kind::Image
    } else if head.starts_with(b"%PDF") {
        Kind::Pdf
    } else if !head.contains(&0) && utf8_prefix(&head) {
        Kind::Text
    } else {
        Kind::Other
    })
}

/// Valid UTF-8, allowing a character cut off at the end of the sample.
fn utf8_prefix(bytes: &[u8]) -> bool {
    match std::str::from_utf8(bytes) {
        Ok(_) => true,
        Err(e) => e.error_len().is_none() && bytes.len() - e.valid_up_to() < 4,
    }
}

pub fn human_size(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.0} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

/// Split attachments into those an agent takes natively and those it gets
/// by path.
pub struct Delivery<'a> {
    pub native: Vec<&'a Attachment>,
    pub by_path: Vec<&'a Attachment>,
}

impl<'a> Delivery<'a> {
    pub fn split(files: &'a [Attachment], mut native: impl FnMut(&Attachment) -> bool) -> Self {
        let (native, by_path) = files.iter().partition(|a| native(a));
        Self { native, by_path }
    }

    /// The prompt plus a list of the files the agent must open itself.
    pub fn prompt(&self, prompt: &str) -> String {
        let mut p = if prompt.trim().is_empty() {
            "Please look at the attached file(s).".to_string()
        } else {
            prompt.to_string()
        };
        if !self.by_path.is_empty() {
            p.push_str("\n\nAttached files (open them with your tools):");
            for a in &self.by_path {
                p.push_str(&format!("\n- {} ({})", a.path.display(), human_size(a.size)));
            }
        }
        p
    }

    /// Distinct parent directories of the by-path files.
    pub fn dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> =
            self.by_path.iter().filter_map(|a| a.path.parent().map(Path::to_path_buf)).collect();
        dirs.sort();
        dirs.dedup();
        dirs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("archonui-attach-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn sniffs_by_content() {
        assert_eq!(Attachment::inspect(&write("a.bin", b"\x89PNG\r\n\x1a\nrest")).unwrap().kind, Kind::Image);
        assert_eq!(Attachment::inspect(&write("b.txt", b"%PDF-1.4")).unwrap().kind, Kind::Pdf);
        assert_eq!(Attachment::inspect(&write("c", "héllo".as_bytes())).unwrap().kind, Kind::Text);
        assert_eq!(Attachment::inspect(&write("d", b"\x00\x01\x02")).unwrap().kind, Kind::Other);
    }

    #[test]
    fn image_media_type_follows_the_content_not_the_extension() {
        let jpeg_as_png = Attachment::inspect(&write("photo.png", b"\xff\xd8\xff\xe0rest")).unwrap();
        assert_eq!(jpeg_as_png.media_type(), "image/jpeg");
        let webp_no_ext = Attachment::inspect(&write("shot", b"RIFF\x00\x00\x00\x00WEBPVP8 ")).unwrap();
        assert_eq!(webp_no_ext.media_type(), "image/webp");
        let gif_as_jpg = Attachment::inspect(&write("anim.jpg", b"GIF89a....")).unwrap();
        assert_eq!(gif_as_jpg.media_type(), "image/gif");
    }

    #[test]
    fn delivery_lists_by_path_files() {
        let files = [Attachment::inspect(&write("e.zip", b"PK\x03\x04\x00")).unwrap()];
        let d = Delivery::split(&files, |a| a.kind == Kind::Image);
        assert!(d.native.is_empty());
        let p = d.prompt("hi");
        assert!(p.starts_with("hi\n\nAttached files") && p.contains("e.zip"));
        assert_eq!(Delivery::split(&[], |_| true).prompt(" "), "Please look at the attached file(s).");
    }
}
