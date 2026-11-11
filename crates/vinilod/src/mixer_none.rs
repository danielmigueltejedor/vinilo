// SPDX-FileCopyrightText: 2026 Miguel Rincon
// SPDX-License-Identifier: GPL-3.0-or-later

//! The mixer where there is no PulseAudio to ask (macOS).
//!
//! `start` answers `None`, which the daemon already treats as "no audio
//! server": it keeps its own record of the volume and nothing else changes.

pub struct Mixer;

impl Mixer {
    pub fn start() -> Option<Self> {
        None
    }

    pub fn set(&self, _volume: f64) {}

    pub fn current(&self) -> Option<f64> {
        None
    }
}
