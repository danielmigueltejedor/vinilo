// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Vocal attenuation for karaoke on catalogue / local PCM.
//!
//! Centre-panned vocals cancel when left and right are subtracted. The
//! slider crossfades between that instrumental and the original mix. Not
//! stem separation — and useless for MusicKit, which never hands us
//! samples — but loud enough to sing over on stereo catalogue audio.

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

/// Wrap a stereo source and blend the original with an L−R instrumental.
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
        let (out_l, out_r) = blend(left, right, self.gain.get());
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

/// Blend one stereo pair. `voice` is 0 (instrumental) … 1 (full mix).
pub fn blend(left: f32, right: f32, voice: f32) -> (f32, f32) {
    if voice >= 0.999 {
        return (left, right);
    }
    // Power curve: the bottom half of the slider is strongly instrumental.
    let voice = voice.powf(2.2);
    let instrumental = 1.0 - voice;
    let diff = left - right;
    let out_l = (left * voice + diff * instrumental * 0.85).clamp(-1.0, 1.0);
    let out_r = (right * voice + (-diff) * instrumental * 0.85).clamp(-1.0, 1.0);
    (out_l, out_r)
}

/// In-place L−R blend for an interleaved stereo buffer (Spotify / librespot).
pub fn apply_interleaved(samples: &mut [f32], voice: f32) {
    if voice >= 0.999 {
        return;
    }
    let mut i = 0;
    while i + 1 < samples.len() {
        let (l, r) = blend(samples[i], samples[i + 1], voice);
        samples[i] = l;
        samples[i + 1] = r;
        i += 2;
    }
}

/// Apply vocal blend only when the source is stereo; mono passes through.
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
    fn zero_gain_keeps_side_content() {
        // Pure side: L=1, R=-1 → L−R = 2.
        let samples = vec![1.0f32, -1.0];
        let gain = VocalGain::new(0.0);
        let mut filtered = MidSide {
            inner: samples.into_iter(),
            gain,
            pending_right: None,
        };
        let l = filtered.next().unwrap();
        let r = filtered.next().unwrap();
        assert!(l > 0.5, "instrumental left should stay audible, got {l}");
        assert!(r < -0.5, "instrumental right should stay audible, got {r}");
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

    #[test]
    fn mid_slider_reduces_centre_more_than_linear() {
        let samples = vec![0.9f32, 0.9];
        let gain = VocalGain::new(0.5);
        let mut filtered = MidSide {
            inner: samples.into_iter(),
            gain,
            pending_right: None,
        };
        let l = filtered.next().unwrap();
        // At 0.5 linear mid would keep ~0.45; power 2.2 keeps ~0.9 * 0.5^2.2 ≈ 0.2.
        assert!(
            l < 0.35,
            "halfway slider should heavily cut centre, got {l}"
        );
    }
}
