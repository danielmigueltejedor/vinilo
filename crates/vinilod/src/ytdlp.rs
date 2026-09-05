// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! `yt-dlp` as the fetch half of Spotify / YouTube Music / Tidal.
//!
//! Search for YouTube Music is also `yt-dlp` (`ytsearch`). The binary is
//! expected on PATH — Arch: `pacman -S yt-dlp`. Playback prefers the native
//! audio YouTube already has (m4a/mp3); ffmpeg transcode is only the fallback
//! for webm, because that is what made skipping tracks feel like a download.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use vinilo_core::provider::Provider;
use vinilo_core::setup;
use vinilo_core::streams::{self, StreamHit, youtube_hit_from_json};

pub fn missing_hint() -> String {
    "yt-dlp is not installed. On Arch: sudo pacman -S yt-dlp ffmpeg".into()
}

fn cookie_args() -> Vec<String> {
    let provider = vinilo_core::provider::load().unwrap_or(Provider::YoutubeMusic);
    setup::ytdlp_cookie_args(provider)
}

fn apply_cookies(cmd: &mut Command) {
    for arg in cookie_args() {
        cmd.arg(arg);
    }
}

pub fn available() -> bool {
    Command::new("yt-dlp")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn search(query: &str) -> Result<Vec<StreamHit>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    if !available() {
        return Err(missing_hint());
    }
    let spec = format!("ytsearch20:{query}");
    let mut cmd = Command::new("yt-dlp");
    cmd.args([
        "-j",
        "--flat-playlist",
        "--no-warnings",
        "--no-playlist",
        &spec,
    ]);
    apply_cookies(&mut cmd);
    let output = cmd.output().map_err(|err| format!("yt-dlp: {err}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("yt-dlp search failed: {}", err.trim()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut hits = Vec::new();
    for line in stdout.lines() {
        let Ok(value) = serde_json::from_str(line) else {
            continue;
        };
        if let Some(hit) = youtube_hit_from_json(&value) {
            hits.push(hit);
        }
    }
    Ok(hits)
}

/// Formats rodio / lofty can open without a transcode.
const NATIVE: &[&str] = &["mp3", "m4a", "aac", "ogg", "flac", "wav"];

/// Download audio for a hit into `dir`. Reuses a file already on disk.
pub fn download(hit: &StreamHit, dir: &Path) -> Result<PathBuf, String> {
    if !available() {
        return Err(missing_hint());
    }
    let _ = std::fs::create_dir_all(dir);
    let stem = hit
        .id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>();
    if let Some(existing) = existing_file(dir, &stem) {
        streams::write_sidecar(&existing, hit);
        return Ok(existing);
    }
    let template = dir.join(format!("{stem}.%(ext)s"));
    try_download(&hit.play_query, &template, false);
    if existing_file(dir, &stem).is_none() && !hit.id.starts_with("yt:") {
        try_download(&hit.youtube_search_spec(), &template, false);
    }
    if let Some(path) = existing_file(dir, &stem) {
        let path = ensure_native(path);
        streams::write_sidecar(&path, hit);
        return Ok(path);
    }
    // Last resort: ffmpeg mp3. Quality 5 is plenty for skipping tracks;
    // quality 0 re-encoded every song and made Next wait on the transcode.
    try_download(&hit.play_query, &template, true);
    let path = existing_file(dir, &stem).ok_or_else(|| "yt-dlp wrote no file".to_string())?;
    streams::write_sidecar(&path, hit);
    Ok(path)
}

fn try_download(target: &str, template: &Path, extract_mp3: bool) {
    let _ = run_download(target, template, extract_mp3);
}

fn run_download(target: &str, template: &Path, extract_mp3: bool) -> Result<(), String> {
    let mut cmd = Command::new("yt-dlp");
    cmd.args([
        "-f",
        "bestaudio[ext=m4a]/bestaudio[ext=mp3]/bestaudio/best",
        "-o",
        &template.to_string_lossy(),
        "--no-playlist",
        "--no-warnings",
        "--newline",
        "-N",
        "4",
        "--no-mtime",
    ]);
    if extract_mp3 {
        cmd.args(["-x", "--audio-format", "mp3", "--audio-quality", "5"]);
    }
    apply_cookies(&mut cmd);
    cmd.arg(target);
    let output = cmd.output().map_err(|err| format!("yt-dlp: {err}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("could not fetch audio: {}", err.trim()));
    }
    Ok(())
}

fn ensure_native(path: PathBuf) -> PathBuf {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if NATIVE.iter().any(|want| *want == ext) {
        return path;
    }
    let mp3 = path.with_extension("mp3");
    if mp3.is_file() && mp3.metadata().map(|m| m.len() > 0).unwrap_or(false) {
        return mp3;
    }
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-i",
            &path.to_string_lossy(),
            "-q:a",
            "4",
            &mp3.to_string_lossy(),
        ])
        .status();
    if status.map(|s| s.success()).unwrap_or(false) && mp3.is_file() {
        let _ = std::fs::remove_file(&path);
        mp3
    } else {
        path
    }
}

fn existing_file(dir: &Path, stem: &str) -> Option<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return None;
    };
    let mut native = None;
    let mut other = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name()?.to_string_lossy();
        if name.ends_with(".json") || name.ends_with(".part") || name.ends_with(".ytdl") {
            continue;
        }
        let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
        const AUDIO: &[&str] = &["mp3", "m4a", "opus", "ogg", "webm", "flac", "wav", "aac"];
        if !AUDIO.iter().any(|want| *want == ext) {
            continue;
        }
        if name.starts_with(stem)
            && path.is_file()
            && entry.metadata().map(|m| m.len() > 0).unwrap_or(false)
        {
            let age = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.elapsed().ok())
                .unwrap_or(Duration::from_secs(0));
            // A half-written download from a killed process is useless.
            if age > Duration::from_millis(50) {
                if NATIVE.iter().any(|want| *want == ext) {
                    native = Some(path);
                } else {
                    other = Some(path);
                }
            }
        }
    }
    native.or(other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_audio_is_not_transcoded() {
        for ext in ["m4a", "mp3", "ogg", "flac"] {
            assert!(NATIVE.contains(&ext), "{ext} should play without ffmpeg");
        }
        assert!(!NATIVE.contains(&"webm"));
    }
}
