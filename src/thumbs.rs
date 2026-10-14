//! Small PNG thumbnails of attached images, cached on disk. GPUI fails to
//! draw very large images (a 3000×2000 one stays blank), and decoding them
//! on every render would be slow anyway.

use crate::store;
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
};

/// Longest side of a thumbnail, in pixels (large enough for 2× displays).
const MAX_SIDE: u32 = 480;

/// Most memory a decode may allocate.
const MAX_DECODE_BYTES: u64 = 256 * 1024 * 1024;

static TMP_SEQ: AtomicU32 = AtomicU32::new(0);

#[derive(Clone)]
pub enum Thumb {
    Loading,
    Ready(PathBuf),
    Failed,
}

/// Blocking: the cached thumbnail for `src`, creating it if needed.
pub fn thumbnail(src: &Path) -> Result<PathBuf, String> {
    let meta = src.metadata().map_err(|e| e.to_string())?;
    let mut h = DefaultHasher::new();
    (src, meta.len(), meta.modified().ok()).hash(&mut h);
    let dest = store::dir().with_file_name("thumbs").join(format!("{:016x}.png", h.finish()));
    if dest.exists() {
        return Ok(dest);
    }
    let mut reader = image::ImageReader::open(src).and_then(|r| r.with_guessed_format()).map_err(|e| e.to_string())?;
    // A tiny file can declare a gigantic canvas; refuse to allocate for it.
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let image = reader.decode().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
    // Unique per call, so two threads making the same thumbnail don't write one file.
    let tmp = dest.with_extension(format!("png.{}.{}.tmp", std::process::id(), TMP_SEQ.fetch_add(1, Ordering::Relaxed)));
    // `thumbnail` also scales up, which would blur a small icon; only shrink.
    let image = if image.width() > MAX_SIDE || image.height() > MAX_SIDE { image.thumbnail(MAX_SIDE, MAX_SIDE) } else { image };
    let saved = image.save_with_format(&tmp, image::ImageFormat::Png).map_err(|e| e.to_string());
    let result = saved.and_then(|()| std::fs::rename(&tmp, &dest).map_err(|e| e.to_string()));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map(|()| dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("archonui-thumbs-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn small_images_keep_their_size_and_large_ones_shrink() {
        let dir = temp("sizes");
        let small = dir.join("small.png");
        image::RgbaImage::new(16, 8).save(&small).unwrap();
        let big = dir.join("big.png");
        image::RgbaImage::new(1200, 600).save(&big).unwrap();
        let t = image::open(thumbnail(&small).unwrap()).unwrap();
        assert_eq!((t.width(), t.height()), (16, 8));
        let t = image::open(thumbnail(&big).unwrap()).unwrap();
        assert_eq!((t.width(), t.height()), (480, 240));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_and_missing_images_are_errors_not_panics() {
        let dir = temp("bad");
        let junk = dir.join("junk.png");
        std::fs::write(&junk, b"not an image").unwrap();
        assert!(thumbnail(&junk).is_err());
        assert!(thumbnail(&dir.join("missing.png")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_requests_for_one_image_all_succeed() {
        let dir = temp("race");
        let src = dir.join("a.png");
        image::RgbaImage::new(900, 900).save(&src).unwrap();
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let src = src.clone();
                std::thread::spawn(move || thumbnail(&src))
            })
            .collect();
        for t in threads {
            assert!(t.join().unwrap().is_ok());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
