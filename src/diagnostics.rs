use std::collections::HashMap;

use crate::midi::{MidiEvent, MidiKind};

const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

const MAJOR_PROFILE: [f32; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];

const MINOR_PROFILE: [f32; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];
const KEY_SWITCH_MARGIN: f32 = 0.5;
const ANCHORED_KEY_SWITCH_MARGIN: f32 = 2.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HysteresisScale(f32);

impl HysteresisScale {
    pub const DEFAULT: Self = Self(1.0);

    pub fn new(scale: f32) -> Result<Self, &'static str> {
        if scale.is_finite() && (0.0..=1.0).contains(&scale) {
            Ok(Self(scale))
        } else {
            Err("key hysteresis must be a number from 0 to 1")
        }
    }

    pub fn value(self) -> f32 {
        self.0
    }
}

impl Default for HysteresisScale {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyMode {
    Major,
    Minor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DetectedKey {
    pub root: u8,
    pub mode: KeyMode,
}

pub struct KeyDetector {
    current: Option<DetectedKey>,
    confidence: f32,
    hysteresis_scale: HysteresisScale,
}

impl KeyDetector {
    pub fn with_hysteresis_scale(hysteresis_scale: HysteresisScale) -> Self {
        Self {
            current: None,
            confidence: 0.0,
            hysteresis_scale,
        }
    }

    pub fn update_active(
        &mut self,
        _time: f32,
        _events: impl IntoIterator<Item = (u8, u8)>,
        active_notes: impl IntoIterator<Item = u8>,
    ) {
        let active_notes: Vec<u8> = active_notes.into_iter().collect();
        if active_notes.len() < 2 {
            self.current = None;
            self.confidence = 0.0;
            return;
        }
        let (candidate, confidence, candidate_score) = detect_key_from_notes(&active_notes);
        let should_switch = match (self.current, candidate) {
            (Some(current), Some(candidate)) if current != candidate => {
                let margin = if active_notes
                    .iter()
                    .min()
                    .is_some_and(|note| note % 12 == current.root)
                {
                    ANCHORED_KEY_SWITCH_MARGIN
                } else {
                    KEY_SWITCH_MARGIN
                };
                let advantage = candidate_score - key_score_from_notes(&active_notes, current);
                advantage > 0.0 && advantage >= margin * self.hysteresis_scale.value()
            }
            _ => true,
        };
        if should_switch {
            self.current = candidate;
            self.confidence = confidence;
        }
    }

    pub fn current(&self) -> Option<DetectedKey> {
        self.current
    }

    pub fn confidence(&self) -> f32 {
        self.confidence
    }
}

impl Default for KeyDetector {
    fn default() -> Self {
        Self::with_hysteresis_scale(HysteresisScale::DEFAULT)
    }
}

impl std::fmt::Display for DetectedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {}",
            NOTE_NAMES[usize::from(self.root)],
            match self.mode {
                KeyMode::Major => "major",
                KeyMode::Minor => "minor",
            }
        )
    }
}

#[derive(Debug)]
pub struct Analysis {
    pub note_on_count: usize,
    pub note_off_count: usize,
    pub max_polyphony: usize,
    pub key_changes: Vec<(f32, DetectedKey, usize, f32)>,
}

pub fn analyze(events: &[MidiEvent]) -> Analysis {
    analyze_with_hysteresis_scale(events, HysteresisScale::DEFAULT)
}

pub fn analyze_with_hysteresis_scale(
    events: &[MidiEvent],
    hysteresis_scale: HysteresisScale,
) -> Analysis {
    let mut active: HashMap<(u8, u8), u8> = HashMap::new();
    let mut detector = KeyDetector::with_hysteresis_scale(hysteresis_scale);
    let mut note_on_count = 0;
    let mut note_off_count = 0;
    let mut max_polyphony = 0;
    let mut current_key = None;
    let mut key_changes = Vec::new();

    let mut index = 0;
    while let Some(first) = events.get(index) {
        let time = first.time;
        let mut new_notes = Vec::new();
        while let Some(event) = events.get(index).filter(|event| event.time == time) {
            match event.kind {
                MidiKind::NoteOn { note, velocity } if velocity > 0 => {
                    note_on_count += 1;
                    active.insert((event.channel, note), velocity);
                    new_notes.push((note, velocity));
                }
                MidiKind::NoteOn { note, .. } | MidiKind::NoteOff { note } => {
                    note_off_count += 1;
                    active.remove(&(event.channel, note));
                }
            }
            index += 1;
        }
        max_polyphony = max_polyphony.max(active.len());

        let previous_key = detector.current();
        detector.update_active(time, new_notes, active.keys().map(|(_, note)| *note));
        let key = detector.current();
        if key != current_key && key != previous_key {
            if let Some(key) = key {
                key_changes.push((time, key, active.len(), detector.confidence()));
            }
            current_key = key;
        }
    }

    Analysis {
        note_on_count,
        note_off_count,
        max_polyphony,
        key_changes,
    }
}

fn detect_key_from_notes(notes: &[u8]) -> (Option<DetectedKey>, f32, f32) {
    let pitch_classes = pitch_classes_from_notes(notes);
    if pitch_classes.iter().all(|weight| *weight == 0.0) {
        return (None, 0.0, 0.0);
    }

    let mut best = None;
    let mut second = f32::NEG_INFINITY;
    for root in 0..12 {
        for mode in [KeyMode::Major, KeyMode::Minor] {
            let key = DetectedKey {
                root: root as u8,
                mode,
            };
            let score = key_score_from_notes(notes, key);
            if best.is_none_or(|(_, best_score)| score > best_score) {
                if let Some((_, best_score)) = best {
                    second = second.max(best_score);
                }
                best = Some((key, score));
            } else {
                second = second.max(score);
            }
        }
    }
    let (key, score) = best.expect("non-empty note set produces a key candidate");
    (Some(key), score - second, score)
}

fn pitch_classes_from_notes(notes: &[u8]) -> [f32; 12] {
    let mut pitch_classes = [0.0_f32; 12];
    for &note in notes {
        pitch_classes[usize::from(note % 12)] += 1.0;
    }
    pitch_classes
}

fn key_score_from_notes(notes: &[u8], key: DetectedKey) -> f32 {
    let pitch_classes = pitch_classes_from_notes(notes);
    let mut score = key_score(pitch_classes, key);
    let bass = *notes.iter().min().expect("key scoring requires notes");
    if bass % 12 == key.root {
        score += 2.0;
    }
    for &note in notes {
        let interval = (i16::from(note) - i16::from(bass)).rem_euclid(12);
        let profile = match key.mode {
            KeyMode::Major => MAJOR_PROFILE,
            KeyMode::Minor => MINOR_PROFILE,
        };
        score += profile[interval as usize] * 0.05;
    }
    score
}

fn key_score(pitch_classes: [f32; 12], key: DetectedKey) -> f32 {
    let profile = match key.mode {
        KeyMode::Major => MAJOR_PROFILE,
        KeyMode::Minor => MINOR_PROFILE,
    };
    (0..12)
        .map(|interval| pitch_classes[(usize::from(key.root) + interval) % 12] * profile[interval])
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_a_major_triad() {
        let (key, _, _) = detect_key_from_notes(&[60, 64, 67]);
        assert_eq!(
            key,
            Some(DetectedKey {
                root: 0,
                mode: KeyMode::Major
            })
        );
    }

    #[test]
    fn follows_successive_chord_roots_without_long_history_lag() {
        let mut detector = KeyDetector::default();
        detector.update_active(0.0, [(55, 100), (59, 100), (62, 100)], [55, 59, 62]);
        assert_eq!(
            detector.current(),
            Some(DetectedKey {
                root: 7,
                mode: KeyMode::Major
            })
        );
        detector.update_active(0.5, [(50, 100), (54, 100), (57, 100)], [50, 54, 57]);
        assert_eq!(
            detector.current(),
            Some(DetectedKey {
                root: 2,
                mode: KeyMode::Major
            })
        );
    }

    #[test]
    fn active_notes_replace_previous_notes_for_two_note_voicings() {
        let mut detector = KeyDetector::default();
        detector.update_active(0.0, [(60, 100), (64, 100)], [60, 64]);
        assert_eq!(
            detector.current(),
            Some(DetectedKey {
                root: 0,
                mode: KeyMode::Major
            })
        );

        detector.update_active(0.1, [], [62, 65]);
        assert_eq!(
            detector.current(),
            Some(DetectedKey {
                root: 2,
                mode: KeyMode::Minor
            })
        );
    }

    #[test]
    fn empty_active_notes_clear_the_detected_key() {
        let mut detector = KeyDetector::default();
        detector.update_active(0.0, [], [60, 64]);
        detector.update_active(0.1, [], []);
        assert_eq!(detector.current(), None);
    }

    #[test]
    fn monophonic_notes_do_not_select_a_tuning_key() {
        let mut detector = KeyDetector::default();
        detector.update_active(0.0, [], [60]);
        assert_eq!(detector.current(), None);
        detector.update_active(0.1, [], [64]);
        assert_eq!(detector.current(), None);
    }

    #[test]
    fn retains_existing_key_without_a_meaningful_score_advantage() {
        let mut detector = KeyDetector::default();
        detector.update_active(0.0, [], [60, 64]);
        detector.update_active(0.1, [], [60, 64, 68]);
        assert_eq!(
            detector.current(),
            Some(DetectedKey {
                root: 0,
                mode: KeyMode::Major
            })
        );
    }

    #[test]
    fn hysteresis_scale_controls_how_quickly_key_selection_changes() {
        let mut stable = KeyDetector::default();
        stable.update_active(0.0, [], [60, 64, 67]);
        stable.update_active(0.1, [], [60, 67, 70]);
        assert_eq!(
            stable.current(),
            Some(DetectedKey {
                root: 0,
                mode: KeyMode::Major,
            })
        );

        let mut responsive = KeyDetector::with_hysteresis_scale(
            HysteresisScale::new(0.0).expect("zero is a valid hysteresis scale"),
        );
        responsive.update_active(0.0, [], [60, 64, 67]);
        responsive.update_active(0.1, [], [60, 67, 70]);
        assert_eq!(
            responsive.current(),
            Some(DetectedKey {
                root: 0,
                mode: KeyMode::Minor,
            })
        );
    }

    #[test]
    fn rejects_hysteresis_scales_outside_zero_to_one() {
        assert!(HysteresisScale::new(-0.1).is_err());
        assert!(HysteresisScale::new(1.1).is_err());
        assert!(HysteresisScale::new(f32::NAN).is_err());
        assert!(HysteresisScale::new(0.5).is_ok());
    }

    #[test]
    fn sustained_current_tonic_anchors_key_selection() {
        let mut detector = KeyDetector::default();
        detector.update_active(0.0, [], [55, 59, 62]);
        assert_eq!(
            detector.current(),
            Some(DetectedKey {
                root: 7,
                mode: KeyMode::Major
            })
        );

        detector.update_active(0.1, [], [55, 69, 77]);
        assert_eq!(
            detector.current(),
            Some(DetectedKey {
                root: 7,
                mode: KeyMode::Major
            })
        );
    }

    #[test]
    fn octave_register_and_lowest_note_influence_active_key_selection() {
        let mut detector = KeyDetector::default();
        detector.update_active(0.0, [], [36, 43]);
        assert_eq!(
            detector.current(),
            Some(DetectedKey {
                root: 0,
                mode: KeyMode::Major
            })
        );

        detector.update_active(0.1, [], [43, 48]);
        assert_eq!(
            detector.current(),
            Some(DetectedKey {
                root: 7,
                mode: KeyMode::Major
            })
        );
    }
}
