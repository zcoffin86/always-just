use crate::diagnostics::{DetectedKey, KeyMode};
use crate::midi_note_frequency;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ratio {
    pub numerator: u32,
    pub denominator: u32,
}

impl Ratio {
    pub const fn new(numerator: u32, denominator: u32) -> Self {
        Self {
            numerator,
            denominator,
        }
    }

    fn value(self) -> f32 {
        self.numerator as f32 / self.denominator as f32
    }
}

const MAJOR_RATIOS: [Ratio; 12] = [
    Ratio::new(1, 1),
    Ratio::new(16, 15),
    Ratio::new(9, 8),
    Ratio::new(6, 5),
    Ratio::new(5, 4),
    Ratio::new(4, 3),
    Ratio::new(45, 32),
    Ratio::new(3, 2),
    Ratio::new(8, 5),
    Ratio::new(5, 3),
    Ratio::new(9, 5),
    Ratio::new(15, 8),
];

const MINOR_RATIOS: [Ratio; 12] = [
    Ratio::new(1, 1),
    Ratio::new(16, 15),
    Ratio::new(9, 8),
    Ratio::new(6, 5),
    Ratio::new(5, 4),
    Ratio::new(4, 3),
    Ratio::new(45, 32),
    Ratio::new(3, 2),
    Ratio::new(8, 5),
    Ratio::new(5, 3),
    Ratio::new(7, 4),
    Ratio::new(15, 8),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TunedFrequency {
    pub frequency: f32,
    pub cents_offset: f32,
    pub ratio: Ratio,
}

pub fn target_frequency(key: DetectedKey, note: u8) -> TunedFrequency {
    let interval = (i16::from(note % 12) - i16::from(key.root) + 12) % 12;
    let root_note = note - interval as u8;
    let ratios = match key.mode {
        KeyMode::Major => MAJOR_RATIOS,
        KeyMode::Minor => MINOR_RATIOS,
    };
    let ratio = ratios[usize::from(interval as u8)].value();
    let twelve_tet = 2.0_f32.powf(interval as f32 / 12.0);
    TunedFrequency {
        frequency: midi_note_frequency(root_note) * ratio,
        cents_offset: 1200.0 * (ratio / twelve_tet).log2(),
        ratio: ratios[usize::from(interval as u8)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_key_tonic_instead_of_lowest_note() {
        let key = DetectedKey {
            root: 0,
            mode: KeyMode::Major,
        };
        let tuned = target_frequency(key, 67);
        assert!((tuned.frequency / midi_note_frequency(60) - 1.5).abs() < 0.001);
    }

    #[test]
    fn keeps_minor_seventh_and_major_seventh_ratios_on_their_intervals() {
        let key = DetectedKey {
            root: 0,
            mode: KeyMode::Minor,
        };
        let minor_seventh = target_frequency(key, 70);
        let major_seventh = target_frequency(key, 71);

        assert!((minor_seventh.frequency / midi_note_frequency(60) - 7.0 / 4.0).abs() < 0.001);
        assert!((minor_seventh.cents_offset + 31.17).abs() < 0.02);
        assert!((major_seventh.frequency / midi_note_frequency(60) - 15.0 / 8.0).abs() < 0.001);
        assert!((major_seventh.cents_offset + 11.73).abs() < 0.02);
    }
}
