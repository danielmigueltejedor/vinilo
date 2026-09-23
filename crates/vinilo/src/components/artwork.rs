// SPDX-FileCopyrightText: 2026 Miguel Rincon
// SPDX-License-Identifier: GPL-3.0-or-later

//! Album art: fetch once, keep on disk.
//!
//! Two reasons this caches to a file rather than holding bytes in memory:
//!
//! 1. **MPRIS needs a path.** `mpris:artUrl` has to be a `file://` URL — the
//!    GNOME Shell applet will not reliably fetch an `https://` one. M3 depends
//!    on this, which is why it is built now rather than alongside MPRIS.
//! 2. Apple serves artwork as a *template* (`…/{w}x{h}bb.jpg`), so we request
//!    exactly the pixels the widget needs instead of scaling a 3000px JPEG.

use std::path::{Path, PathBuf};

use relm4::gtk::{gdk, gdk_pixbuf, glib};

use vinilo_core::music::types::Artwork;

// Fetching and the cache layout live in core: the daemon needs the same files
// in the same place, and none of it is a toolkit's business. What stays here is
// the half that turns a JPEG into pixels.
pub use vinilo_core::artwork::{ART_SIZE, fetch, write_atomically};
pub use vinilo_core::paths::artwork_dir as cache_dir;

/// A cover turned into pixels **off the GTK thread**.
///
/// The decode is the expensive half of showing a cover — measured at 2.5ms for
/// one 320px JPEG — and `gtk_image_set_from_file` does it synchronously on
/// whichever thread calls it. A grid fills 385 tiles in one go (#27), so doing
/// it inline froze the UI for half a second.
///
/// Raw pixels rather than a `gdk::Texture` because a texture is a GObject and
/// therefore not `Send`: it cannot be built on a worker and carried back. A
/// `Vec<u8>` can, and turning one into a `gdk::MemoryTexture` on the main
/// thread is a wrap, not a decode.
pub struct Decoded {
    pixels: Vec<u8>,
    width: i32,
    height: i32,
    stride: usize,
    has_alpha: bool,
}

impl std::fmt::Debug for Decoded {
    /// Without this the pixels would be printed. All of them.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Decoded")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl Decoded {
    /// Wrap the pixels as a texture. Main thread, and cheap — no decoding here.
    pub fn into_texture(self) -> gdk::MemoryTexture {
        let format = if self.has_alpha {
            gdk::MemoryFormat::R8g8b8a8
        } else {
            gdk::MemoryFormat::R8g8b8
        };
        gdk::MemoryTexture::new(
            self.width,
            self.height,
            format,
            &glib::Bytes::from_owned(self.pixels),
            self.stride,
        )
    }
}

/// Fetch a tile's cover and decode it, off the GTK thread, reporting whichever
/// half fails.
///
/// The two steps live together because their failures are the same kind of
/// thing — cosmetic, so no toast; a missing cover is not worth interrupting
/// anyone — and because they used to be written as `.ok()` and `and_then` at
/// the call site, which is **silent**. A tile stuck on its placeholder for ever
/// left no trace at all, so a 404 on one cover, a slow network and an
/// undecodable file were indistinguishable from each other and from "it is
/// still loading".
///
/// `key` is only for the log, so a warning here can be lined up with the
/// `tile art delivered` trace on the other side.
pub async fn load_tile(art: Artwork, size: u32, key: &str) -> (Option<PathBuf>, Option<Decoded>) {
    let path = match fetch(art, size).await {
        Ok(path) => Some(path),
        Err(err) => {
            // The error carries the URL it tried — see `fetch`.
            tracing::warn!(%key, ?err, "tile artwork not fetched");
            None
        }
    };
    let decoded = path.as_deref().and_then(|path| {
        let decoded = decode(path, size as i32);
        if decoded.is_none() {
            tracing::warn!(%key, file = %path.display(), "tile artwork on disk but undecodable");
        }
        decoded
    });
    (path, decoded)
}

/// Decode a cover that is already on disk. Call this off the GTK thread.
pub fn decode(path: &Path, size: i32) -> Option<Decoded> {
    let pixbuf = gdk_pixbuf::Pixbuf::from_file_at_scale(path, size, size, true).ok()?;
    Some(Decoded {
        width: pixbuf.width(),
        height: pixbuf.height(),
        stride: pixbuf.rowstride() as usize,
        has_alpha: pixbuf.has_alpha(),
        pixels: pixbuf.read_pixel_bytes().to_vec(),
    })
}

/// How many pixels we sample to find the sleeve's colour. A cover is already
/// a square; 48px is enough to see whether it is white, red, or mixed.
const SAMPLE_PX: i32 = 48;

/// Cached colour filename. A `.hex` file is the sleeve's colour as `RRGGBB`,
/// painted as CSS — not a stretched photograph and not a PNG under a veil.
const WASH_EXT: &str = "backdrop.hex";

/// Sample the sleeve and write its colour beside the cover.
///
/// Apple Music tints the player from the record: a white sleeve stays white,
/// a red one stays red. The cover is still the cover in the UI; this is only
/// the atmosphere behind it. Off the GTK thread (rule 8).
pub fn backdrop(path: &Path) -> Option<PathBuf> {
    let out = path.with_extension(WASH_EXT);
    if out.exists() {
        return Some(out);
    }
    let pixbuf = gdk_pixbuf::Pixbuf::from_file_at_scale(path, SAMPLE_PX, SAMPLE_PX, false).ok()?;
    let pixels = pixbuf.read_pixel_bytes().to_vec();
    let color = lift_chroma(dominant(
        &pixels,
        pixbuf.width() as usize,
        pixbuf.height() as usize,
        pixbuf.n_channels() as usize,
        pixbuf.rowstride() as usize,
    ));
    std::fs::write(
        &out,
        format!("{:02x}{:02x}{:02x}", color[0], color[1], color[2]),
    )
    .ok()?;
    Some(out)
}

/// CSS hex (`#rrggbb`) stored next to the cover, if we sampled one.
pub fn wash_hex(path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let hex = raw.trim();
    if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(format!("#{hex}"))
    } else {
        None
    }
}

/// The colour that should tint the player.
///
/// Average of the sleeve, unless a quarter or more of it is actually coloured
/// — then those pixels win, so a red record with a black frame stays red
/// instead of going maroon. A white sleeve with a small logo stays white:
/// the logo is not a quarter of the picture, and "if it is white, white" is
/// the whole point.
fn dominant(pixels: &[u8], w: usize, h: usize, channels: usize, stride: usize) -> [u8; 3] {
    if w == 0 || h == 0 || channels < 3 {
        return [128, 128, 128];
    }
    let mut all = [0u64; 3];
    let mut all_n = 0u64;
    let mut vivid = [0u64; 3];
    let mut vivid_n = 0u64;
    for y in 0..h {
        for x in 0..w {
            let i = y * stride + x * channels;
            if channels >= 4 && pixels.get(i + 3).copied().unwrap_or(255) < 16 {
                continue;
            }
            let r = pixels[i];
            let g = pixels[i + 1];
            let b = pixels[i + 2];
            all[0] += u64::from(r);
            all[1] += u64::from(g);
            all[2] += u64::from(b);
            all_n += 1;
            if chroma(r, g, b) >= 28 {
                vivid[0] += u64::from(r);
                vivid[1] += u64::from(g);
                vivid[2] += u64::from(b);
                vivid_n += 1;
            }
        }
    }
    if all_n == 0 {
        return [128, 128, 128];
    }
    if vivid_n * 4 >= all_n {
        average(vivid, vivid_n)
    } else {
        average(all, all_n)
    }
}

fn average(sum: [u64; 3], n: u64) -> [u8; 3] {
    [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8]
}

fn chroma(r: u8, g: u8, b: u8) -> u32 {
    let r = u32::from(r);
    let g = u32::from(g);
    let b = u32::from(b);
    r.max(g).max(b) - r.min(g).min(b)
}

/// Push a coloured sleeve further from grey so a red record stays red in CSS.
/// Greys and whites stay put — inventing a tint there would lie about the record.
fn lift_chroma(color: [u8; 3]) -> [u8; 3] {
    let [r, g, b] = color;
    if chroma(r, g, b) < 28 {
        return color;
    }
    let luma = (2126 * u32::from(r) + 7152 * u32::from(g) + 722 * u32::from(b)) / 10000;
    let luma = luma as i32;
    // 2.1× from grey. Anything milder still reads as a stained window once
    // the veil sits on top.
    let lift = |v: u8| {
        let lifted = luma + (i32::from(v) - luma) * 210 / 100;
        lifted.clamp(0, 255) as u8
    };
    let mut out = [lift(r), lift(g), lift(b)];
    // Dark sleeves vanish under the scrim; bring them up toward a mid glow.
    let out_luma =
        (2126 * u32::from(out[0]) + 7152 * u32::from(out[1]) + 722 * u32::from(out[2])) / 10000;
    if out_luma < 110 {
        let gain = 110.0 / out_luma.max(1) as f32;
        for c in &mut out {
            *c = (f32::from(*c) * gain).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use vinilo_core::artwork::cache_path;

    fn rgb(pixels: &[[u8; 3]]) -> Vec<u8> {
        pixels.iter().flat_map(|p| *p).collect()
    }

    #[test]
    fn cache_paths_are_stable_and_size_specific() {
        let art = Artwork::new("https://is1.mzstatic.com/image/thumb/x/{w}x{h}bb.jpg");
        let a = cache_path(&art, 512).unwrap();
        let b = cache_path(&art, 512).unwrap();
        let c = cache_path(&art, 64).unwrap();

        assert_eq!(a, b, "same art and size must reuse one file");
        assert_ne!(a, c, "different sizes must not collide");
        assert!(a.to_string_lossy().ends_with("-512.jpg"));
    }

    #[test]
    fn different_art_does_not_share_a_file() {
        let a = Artwork::new("https://is1.mzstatic.com/a/{w}x{h}bb.jpg");
        let b = Artwork::new("https://is1.mzstatic.com/b/{w}x{h}bb.jpg");
        assert_ne!(cache_path(&a, 512), cache_path(&b, 512));
    }

    #[test]
    fn a_white_sleeve_stays_white() {
        let px = rgb(&[[250, 250, 250]; 16]);
        assert_eq!(dominant(&px, 4, 4, 3, 12), [250, 250, 250]);
    }

    #[test]
    fn a_red_sleeve_stays_red() {
        let px = rgb(&[[200, 24, 24]; 16]);
        let [r, g, b] = dominant(&px, 4, 4, 3, 12);
        assert!(r > 180 && g < 40 && b < 40, "got [{r}, {g}, {b}]");
    }

    #[test]
    fn a_red_record_with_a_black_frame_stays_red() {
        // Twelve black pixels and four red ones: a quarter is coloured, so
        // the frame must not pull the wash to maroon.
        let mut tiles = vec![[0, 0, 0]; 12];
        tiles.extend([[210, 30, 30]; 4]);
        let [r, g, b] = dominant(&rgb(&tiles), 4, 4, 3, 12);
        assert!(r > 150 && r > g && r > b, "got [{r}, {g}, {b}]");
    }

    #[test]
    fn a_white_sleeve_with_a_small_logo_stays_white() {
        // One red pixel in sixteen is a logo, not the record.
        let mut tiles = vec![[245, 245, 245]; 15];
        tiles.push([220, 20, 20]);
        let [r, g, b] = dominant(&rgb(&tiles), 4, 4, 3, 12);
        assert!(r > 220 && g > 220 && b > 220, "got [{r}, {g}, {b}]");
    }

    #[test]
    fn lifting_chroma_leaves_white_alone() {
        assert_eq!(lift_chroma([250, 250, 250]), [250, 250, 250]);
        assert_eq!(lift_chroma([128, 128, 128]), [128, 128, 128]);
        let [r, g, b] = lift_chroma([160, 80, 80]);
        assert!(r > 200 && g <= 70 && b <= 70, "got [{r}, {g}, {b}]");
    }

    #[test]
    fn the_wash_filename_is_a_hex_not_a_photo() {
        let name = std::path::Path::new("/tmp/abc-512.jpg").with_extension(WASH_EXT);
        let s = name.to_string_lossy();
        assert!(s.ends_with("backdrop.hex"), "{s}");
        for old in [
            "backdrop256",
            "backdrop-tone.png",
            "backdrop-glow.png",
            "backdrop-vivid.png",
        ] {
            assert!(!s.contains(old), "{s} still looks like {old}");
        }
    }

    #[test]
    fn write_atomically_creates_the_directory_and_leaves_no_temp() {
        let dir = std::env::temp_dir().join(format!("vinilo-art-test-{}", std::process::id()));
        let target = dir.join("nested/cover.jpg");
        write_atomically(&target, b"not really a jpeg").unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"not really a jpeg");
        let strays: Vec<_> = std::fs::read_dir(target.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("tmp"))
            .collect();
        assert!(strays.is_empty(), "temp file left behind");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
