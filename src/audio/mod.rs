//! Audio helpers. Actual audio graph construction lives in `crate::media`.

use std::collections::HashMap;

/// Smoothed audio level for one source, in the 0.0 .. 1.0 range.
#[derive(Clone, Copy, Debug, Default)]
pub struct Level {
    pub peak: f32,
    pub rms: f32,
}

impl Level {
    pub fn update(&mut self, rms: f32, peak: f32) {
        self.rms = self.rms * 0.65 + rms.clamp(0.0, 1.0) * 0.35;
        self.peak = self.peak.max(peak.clamp(0.0, 1.0)) * 0.92;
    }
}

#[derive(Clone, Debug, Default)]
pub struct LevelStore {
    levels: HashMap<String, Level>,
}

impl LevelStore {
    pub fn update(&mut self, name: &str, rms: f32, peak: f32) {
        self.levels.entry(name.to_string()).or_default().update(rms, peak);
    }

    pub fn get(&self, name: &str) -> Level {
        self.levels.get(name).copied().unwrap_or_default()
    }

    pub fn decay(&mut self) {
        for level in self.levels.values_mut() {
            level.peak *= 0.94;
            level.rms *= 0.94;
        }
    }
}

/// Converts a linear volume (0..1) to a readable dB string.
pub fn volume_to_db(volume: f32) -> String {
    if volume <= 0.001 {
        "-inf dB".to_string()
    } else {
        format!("{:.1} dB", 20.0 * volume.log10())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_smoothing_stays_in_range() {
        let mut level = Level::default();
        for _ in 0..100 {
            level.update(0.5, 0.9);
        }
        assert!((level.rms - 0.5).abs() < 0.01);
        assert!(level.peak <= 1.0);
        assert_eq!(volume_to_db(0.0), "-inf dB");
    }
}
