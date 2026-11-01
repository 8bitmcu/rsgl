use image::{imageops, ImageFormat};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// Longest side of the cached image. It gets stretched to the window when
/// drawn, which the blur hides, and keeps blurring and decoding cheap.
const MAX_SIDE: u32 = 640;

/// Empties `dir` if its images were blurred with a different sigma, since
/// `blurred()` only compares mtimes and would keep the old blur around.
pub fn reset_if_changed(dir: &Path, sigma: f32) {
    let marker = dir.join("sigma");
    let want = sigma.to_string();
    if fs::read_to_string(&marker).is_ok_and(|s| s == want) {
        return;
    }
    let _ = fs::remove_dir_all(dir);
    let _ = fs::create_dir_all(dir).and_then(|_| fs::write(&marker, want));
}

/// Writes a blurred copy of `src` to `dest` and returns `dest`. Skips the work
/// if `dest` is already newer than `src`. A `sigma` of 0 skips the blur.
pub fn blurred(src: &Path, dest: &Path, sigma: f32) -> Result<PathBuf, Box<dyn Error>> {
    let mtime = |p: &Path| fs::metadata(p).and_then(|m| m.modified()).ok();
    if mtime(dest).unwrap_or(SystemTime::UNIX_EPOCH) >= mtime(src).ok_or("source missing")? {
        return Ok(dest.to_path_buf());
    }

    let small = image::open(src)?
        .resize(MAX_SIDE, MAX_SIDE, imageops::FilterType::Triangle)
        .to_rgb8();
    let out = if sigma > 0.0 { imageops::blur(&small, sigma) } else { small };

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_extension("part");
    out.save_with_format(&tmp, ImageFormat::Jpeg)?;
    fs::rename(&tmp, dest)?; // same trick as downloads: never show a half-written file
    Ok(dest.to_path_buf())
}
