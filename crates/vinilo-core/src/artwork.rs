// SPDX-FileCopyrightText: 2026 Miguel Rincon
// SPDX-License-Identifier: GPL-3.0-or-later

//! Album art: fetch once, keep on disk.
//!
//! Two reasons this caches to a file rather than holding bytes in memory:
//!
//! 1. **MPRIS needs a path.** `mpris:artUrl` has to be a `file://` URL — the
//!    GNOME Shell applet will not reliably fetch an `https://` one.
//! 2. Apple serves artwork as a *template* (`…/{w}x{h}bb.jpg`), so we request
//!    exactly the pixels the caller needs instead of scaling a 3000px JPEG.
//!
//! Decoding is not here. Turning a JPEG into pixels is a toolkit's job and a
//! terminal wants none of it — but every frontend, and the daemon, needs the
//! same file in the same place.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::music::types::Artwork;
use crate::paths::artwork_dir;

/// The size fetched for a now-playing bar and for MPRIS. One size keeps the
/// cache simple; a Shell scales it down and nobody notices.
pub const ART_SIZE: u32 = 512;

/// Where a cover of this size lands on disk.
pub fn cache_path(art: &Artwork, size: u32) -> Option<PathBuf> {
    Some(artwork_dir()?.join(format!("{}-{size}.jpg", art.cache_key())))
}

const FETCH_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";

/// surfacing an error, because a missing cover is not worth a toast.
pub async fn fetch(art: Artwork, size: u32) -> Result<PathBuf> {
    let path = cache_path(&art, size).context("no cache directory available")?;
    if path.is_file() {
        return Ok(path);
    }

    let url = art.url(size);
    let mut last_err = None;
    for candidate in artwork_candidates(&url) {
        match download_bytes(&candidate).await {
            Ok(bytes) if !bytes.is_empty() => {
                write_atomically(&path, &bytes)?;
                return Ok(path);
            }
            Ok(_) => last_err = Some(anyhow::anyhow!("empty artwork {candidate}")),
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("artwork request failed")))
}

fn artwork_candidates(url: &str) -> Vec<String> {
    let mut out = vec![url.to_owned()];
    if url.contains("i.ytimg.com/vi/") && url.contains("hqdefault") {
        out.push(url.replace("hqdefault.jpg", "mqdefault.jpg"));
        out.push(url.replace("hqdefault.jpg", "0.jpg"));
    }
    out
}

async fn download_bytes(url: &str) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .user_agent(FETCH_UA)
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let bytes = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("requesting artwork {url}"))?
        .error_for_status()
        .context("artwork request failed")?
        .bytes()
        .await
        .context("reading artwork body")?;
    Ok(bytes.to_vec())
}

/// `rename` within the same directory is atomic.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().context("artwork path has no parent")?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;

    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("renaming into {}", path.display()))?;
    Ok(())
}
