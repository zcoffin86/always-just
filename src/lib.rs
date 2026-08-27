pub mod diagnostics;
pub mod midi;
pub mod synth;
pub mod tuning;
pub mod wav;

pub const SAMPLE_RATE: u32 = 44_100;

pub fn midi_note_frequency(note: u8) -> f32 {
    440.0 * 2.0_f32.powf((f32::from(note) - 69.0) / 12.0)
}

#[cfg(test)]
mod tests {
    use super::midi_note_frequency;

    #[test]
    fn uses_a440_reference() {
        assert!((midi_note_frequency(69) - 440.0).abs() < 0.001);
    }
}
