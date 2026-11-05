// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Wire digests required by foreign music APIs.
//!
//! Last.fm's `api_sig` is MD5 and YouTube's `SAPISIDHASH` is SHA-1. Those are
//! remote contracts, not choices we get to strengthen — a SHA-256 signature is
//! simply refused. This module exists so SonarCloud Autoscan can exclude the
//! mandated algorithms without silencing the rest of the crate (see
//! `.sonarcloud.properties`).

use md5::{Digest, Md5};
use sha1::{Digest as Sha1Digest, Sha1};

/// Last.fm `api_sig`: MD5 hex of the sorted `namevalue` blob + secret.
pub fn lastfm_api_sig_md5(input: &str) -> String {
    let dig = <Md5 as Digest>::digest(input.as_bytes());
    dig.iter().map(|b| format!("{b:02x}")).collect()
}

/// YouTube `SAPISIDHASH` body: SHA-1 hex of `{ts} {sapisid} {origin}`.
pub fn youtube_sapisid_sha1(input: &str) -> String {
    let mut hasher = Sha1::new();
    Sha1Digest::update(&mut hasher, input.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lastfm_empty_is_stable() {
        assert_eq!(lastfm_api_sig_md5(""), "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn youtube_empty_is_stable() {
        assert_eq!(
            youtube_sapisid_sha1(""),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
    }
}
