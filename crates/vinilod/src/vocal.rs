// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mid/side vocal attenuation for karaoke on catalogue / local PCM.
//!
//! Vocals usually sit in the stereo centre. Attenuating the mid channel
//! leaves the sides (instrumental-ish). Not perfect — and useless for
//! MusicKit, which never hands us samples — but enough to sing over.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use rodio::Source;

/// Shared vocal gain in milli-units: 0 = instrumental, 1000 = full mix.
#[derive(Clone)]
pub struct VocalGain(Arc<AtomicU32>);

impl VocalGain {
    pub fn new(level: f32) -> Self {
        let g = Self(Arc::new(AtomicU32::new(0)));
        g.set(level);
        g
    }

    pub fn set(&self, level: f32) {
        let ms = (level.clamp(0.0, 1.0) * 1000.0).round() as u32;
        self.0.store(ms, Ordering::Relaxed);
    }

    pub fn get(&self) -> f32 {
        self.0.load(Ordering::Relaxed) as f32 / 1000.0
    }
}

/// Wrap a stereo source and attenuate the mid (L+R) by [`VocalGain`].
pub struct MidSide<S> {
    inner: S,
    gain: VocalGain,
    pending_right: Option<f32>,
}

impl<S> MidSide<S> {
    pub fn new(inner: S, gain: VocalGain) -> Self {
        Self {
            inner,
            gain,
            pending_right: None,
        }
    }
}

impl<S> Iterator for MidSide<S>
where
    S: Iterator<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(r) = self.pending_right.take() {
            return Some(r);
        }
        let left = self.inner.next()?;
        let right = self.inner.next().unwrap_or(left);
        let g = self.gain.get();
        if (g - 1.0).abs() < 0.001 {
            self.pending_right = Some(right);
            return Some(left);
        }
        // mid = centre (vocals), side = difference (stereo instruments)
        let mid = (left + right) * 0.5;
        let side = (left - right) * 0.5;
        let out_l = side + mid * g;
        let out_r = -side + mid * g;
        self.pending_right = Some(out_r);
        Some(out_l)
    }
}

impl<S> Source for MidSide<S>
where
    S: Source<Item = f32>,
{
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }

    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

/// Apply mid/side only when the source is stereo; mono passes through.
pub fn maybe_vocal<S>(source: S, gain: VocalGain) -> Box<dyn Source<Item = f32> + Send>
where
    S: Source<Item = f32> + Send + 'static,
{
    if source.channels() == 2 {
        Box::new(MidSide::new(source, gain))
    } else {
        Box::new(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_gain_cancels_identical_centre_vocals() {
        // L=R=1 is pure mid; with g=0 both outs should be ~0.
        let samples = vec![1.0f32, 1.0, 0.5, 0.5];
        let gain = VocalGain::new(0.0);
        let mut filtered = MidSide {
            inner: samples.into_iter(),
            gain,
            pending_right: None,
        };
        let l0 = filtered.next().unwrap();
        let r0 = filtered.next().unwrap();
        assert!(l0.abs() < 1e-5 && r0.abs() < 1e-5);
        let l1 = filtered.next().unwrap();
        let r1 = filtered.next().unwrap();
        assert!(l1.abs() < 1e-5 && r1.abs() < 1e-5);
    }

    #[test]
    fn full_gain_preserves_the_mix() {
        let samples = vec![0.8f32, -0.2];
        let gain = VocalGain::new(1.0);
        let mut filtered = MidSide {
            inner: samples.into_iter(),
            gain,
            pending_right: None,
        };
        assert!((filtered.next().unwrap() - 0.8).abs() < 1e-5);
        assert!((filtered.next().unwrap() - (-0.2)).abs() < 1e-5);
    }
}
