// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Play a googlevideo (or similar) URL through ffmpeg → PCM → rodio.
//!
//! The catalogue used to download the whole file before the first sample.
//! AuraStream streams the URL; we do the same idea without LibVLC: ffmpeg
//! decodes over the network and we feed s16le into a rodio sink.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use rodio::Source;

const SAMPLE_RATE: u32 = 44_100;
const CHANNELS: u16 = 2;

/// Spawn ffmpeg reading `url` and return a rodio Source of PCM.
pub fn open_url_source(url: &str, user_agent: &str) -> Result<(UrlPcm, Child), String> {
    let headers = format!(
        "User-Agent: {user_agent}\r\nOrigin: https://www.youtube.com\r\nReferer: https://www.youtube.com/\r\n"
    );
    let mut child = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-headers",
            &headers,
            "-i",
            url,
            "-f",
            "s16le",
            "-acodec",
            "pcm_s16le",
            "-ac",
            "2",
            "-ar",
            "44100",
            "-vn",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("ffmpeg: {err}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "ffmpeg produced no stdout".to_string())?;
    let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(8);
    thread::Builder::new()
        .name("yt-pcm".into())
        .spawn(move || {
            let mut reader = stdout;
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let samples = i16_le_to_f32(&buf[..n]);
                        if tx.send(samples).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        })
        .map_err(|err| format!("pcm thread: {err}"))?;
    // Wait briefly for the first chunk so a dead URL fails before we claim play.
    // Eight seconds is enough for a healthy googlevideo start; twelve was
    // padding that made a dead resolve feel like a hang.
    let first = rx
        .recv_timeout(Duration::from_secs(8))
        .map_err(|_| "ffmpeg produced no audio".to_string())?;
    Ok((
        UrlPcm {
            rx,
            pending: first.into_iter(),
            current: Vec::new().into_iter(),
        },
        child,
    ))
}

fn i16_le_to_f32(bytes: &[u8]) -> Vec<f32> {
    let n = bytes.len() / 2;
    let mut out = Vec::with_capacity(n);
    for chunk in bytes.as_chunks::<2>().0 {
        let sample = i16::from_le_bytes(*chunk);
        out.push(f32::from(sample) / f32::from(i16::MAX));
    }
    out
}

pub struct UrlPcm {
    rx: mpsc::Receiver<Vec<f32>>,
    pending: std::vec::IntoIter<f32>,
    current: std::vec::IntoIter<f32>,
}

impl Iterator for UrlPcm {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(s) = self.current.next() {
            return Some(s);
        }
        if let Some(s) = self.pending.next() {
            return Some(s);
        }
        match self.rx.recv_timeout(Duration::from_secs(30)) {
            Ok(chunk) => {
                self.current = chunk.into_iter();
                self.current.next()
            }
            Err(_) => None,
        }
    }
}

impl Source for UrlPcm {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        CHANNELS
    }

    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// Best-effort: pull the URL to disk in the background for later cache hits.
pub fn cache_url_in_background(url: String, user_agent: String, path: std::path::PathBuf) {
    let _ = thread::Builder::new().name("yt-cache".into()).spawn(move || {
        let Ok(mut child) = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-headers",
                &format!(
                    "User-Agent: {user_agent}\r\nOrigin: https://www.youtube.com\r\nReferer: https://www.youtube.com/\r\n"
                ),
                "-i",
                &url,
                "-vn",
                "-c",
                "copy",
                "-y",
                &path.to_string_lossy(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return;
        };
        let _ = child.wait();
        if let Ok(meta) = std::fs::metadata(&path)
            && meta.len() < 1024
        {
            let _ = std::fs::remove_file(&path);
        }
    });
}

/// Keep the child alive for the Source's lifetime; drop kills ffmpeg.
pub struct FfmpegGuard(Child);

impl Drop for FfmpegGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl From<Child> for FfmpegGuard {
    fn from(child: Child) -> Self {
        Self(child)
    }
}
