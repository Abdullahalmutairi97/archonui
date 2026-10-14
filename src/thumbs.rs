//! Small PNG thumbnails of attached images, cached on disk. GPUI fails to
//! draw very large images (a 3000×2000 one stays blank), and decoding them
//! on every render would be slow anyway.

use crate::store;
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
};

/// Longest side of a thumbnail, in pixels (large enough for 2× displays).
const MAX_SIDE: u32 = 480;

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
    let image = image::ImageReader::open(src)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
    let tmp = dest.with_extension("png.tmp");
    image
        .thumbnail(MAX_SIDE, MAX_SIDE)
        .save_with_format(&tmp, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
}
