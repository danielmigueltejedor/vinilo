// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Paths a person asked Vinilo to play: files, or a folder of files.
//!
//! The desktop entry claims audio MIME types and `inode/directory`, so Files
//! (and `xdg-open`) can hand us either. Expanding a folder here, in the
//! engine, means every client — GNOME, the terminal, a second `vinilo`
//! invocation — sends the same request and gets the same queue.

use std::path::{Path, PathBuf};

/// How many files a dropped folder may contribute. A whole music library
/// opened by accident should not stall the daemon walking a hundred thousand
/// paths; two thousand is already more than a sitting.
const MAX_FILES: usize = 2_000;

/// Extensions we will put on a local queue.
///
/// The decoder may still refuse a file — that is a later, per-track failure.
/// This list is only "looks like audio", so a folder of photos and flacs
/// becomes a queue of flacs.
const AUDIO_EXT: &[&str] = &[
    "mp3", "flac", "ogg", "oga", "opus", "m4a", "aac", "wav", "wave", "aiff", "aif", "ape", "mpc",
    "mp2", "wma", "wv",
];

/// True when this path is a file whose extension we recognise.
pub fn is_audio_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    AUDIO_EXT.iter().any(|want| ext.eq_ignore_ascii_case(want))
}

/// Expand files and directories into a stable list of audio paths.
///
/// Directories are walked depth-first, hidden names skipped, the result sorted
/// so an album folder plays in track order rather than whatever readdir
/// returned. Duplicates are dropped. The original argument order is kept for
/// the top-level paths themselves: two albums dropped together play in the
/// order they were dropped, each album internally sorted.
pub fn expand_audio_paths<I>(paths: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = PathBuf>,
{
    let mut out = Vec::new();
    for path in paths {
        if out.len() >= MAX_FILES {
            break;
        }
        if path.is_dir() {
            let mut files = Vec::new();
            walk(&path, &mut files);
            files.sort();
            for file in files {
                if out.len() >= MAX_FILES {
                    break;
                }
                if !out.iter().any(|seen| seen == &file) {
                    out.push(file);
                }
            }
        } else if is_audio_file(&path) && !out.iter().any(|seen| seen == &path) {
            out.push(path);
        }
    }
    out
}

fn walk(dir: &Path, into: &mut Vec<PathBuf>) {
    if into.len() >= MAX_FILES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut children: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    children.sort_by_key(|e| e.file_name());
    for entry in children {
        if into.len() >= MAX_FILES {
            return;
        }
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            walk(&path, into);
        } else if is_audio_file(&path) {
            into.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vinilo-local-files-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        std::fs::write(path, []).unwrap();
    }

    #[test]
    fn a_folder_becomes_its_audio_files_in_name_order() {
        let dir = tmp();
        touch(&dir.join("b.mp3"));
        touch(&dir.join("a.flac"));
        touch(&dir.join("readme.txt"));
        touch(&dir.join(".hidden.mp3"));
        let files = expand_audio_paths([dir.clone()]);
        let names: Vec<_> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["a.flac", "b.mp3"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_plain_file_is_kept_and_a_photo_is_not() {
        let dir = tmp();
        let song = dir.join("track.ogg");
        let photo = dir.join("cover.jpg");
        touch(&song);
        touch(&photo);
        let files = expand_audio_paths([song.clone(), photo]);
        assert_eq!(files, vec![song]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn duplicates_are_dropped() {
        let dir = tmp();
        let song = dir.join("one.wav");
        touch(&song);
        let files = expand_audio_paths([song.clone(), song.clone()]);
        assert_eq!(files.len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
