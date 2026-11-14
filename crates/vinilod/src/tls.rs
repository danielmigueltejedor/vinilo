// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! The TLS crypto provider, chosen once for the whole process.
//!
//! The dependency tree enables both of rustls's providers (`ring` through reqwest,
//! `aws-lc-rs` through tokio-rustls's defaults), and with two enabled rustls refuses to
//! guess: `ClientConfig::builder()` panics. librespot's HTTP client calls exactly that the
//! first time a Spotify session connects, so without this every Premium sign-in died after
//! the browser had already said it worked, and the next track asked to sign in again.

/// Install `ring` as the process default. Harmless when one is already installed.
pub fn install_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_default_client_config_builds_once_the_provider_is_installed() {
        super::install_provider();
        assert!(rustls::crypto::CryptoProvider::get_default().is_some());
        // The call librespot's HTTP client makes through hyper-rustls.
        let _ = rustls::ClientConfig::builder();
    }
}
