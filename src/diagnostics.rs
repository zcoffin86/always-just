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

#[derive(Default)]
pub struct KeyDetector {
    history: [f32; 12],
    last_time: f32,
    current: Option<DetectedKey>,
    confidence: f32,
}

impl KeyDetector {
    pub fn update(&mut self, time: f32, events: impl IntoIterator<Item = (u8, u8)>) {
        let elapsed = (time - self.last_time).max(0.0);
        let decay = 0.5_f32.powf(elapsed / 12.0);
        self.history.iter_mut().for_each(|weight| *weight *= decay);
        for (note, velocity) in events {
            self.history[usize::from(note % 12)] += f32::from(velocity) / 127.0;
        }
        let (key, confidence) = detect_key(self.history);
        self.confidence = confidence;
        if confidence >= 0.25 {
            self.current = key;
        }
        self.last_time = time;
    }

    pub fn current(&self) -> Option<DetectedKey> {
        self.current
    }

    pub fn confidence(&self) -> f32 {
        self.confidence
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
    let mut active: HashMap<(u8, u8), u8> = HashMap::new();
    let mut detector = KeyDetector::default();
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
        detector.update(time, new_notes);
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

fn detect_key(pitch_classes: [f32; 12]) -> (Option<DetectedKey>, f32) {
    if pitch_classes.iter().all(|weight| *weight == 0.0) {
        return (None, 0.0);
    }

    let mut best = (
        DetectedKey {
            root: 0,
            mode: KeyMode::Major,
        },
        f32::NEG_INFINITY,
    );
    let mut second = f32::NEG_INFINITY;
    for root in 0..12 {
        for mode in [KeyMode::Major, KeyMode::Minor] {
            let profile = match mode {
                KeyMode::Major => MAJOR_PROFILE,
                KeyMode::Minor => MINOR_PROFILE,
            };
            let score = (0..12)
                .map(|interval| pitch_classes[(root + interval) % 12] * profile[interval])
                .sum();
            if score > best.1 {
                second = best.1;
                best = (
                    DetectedKey {
                        root: root as u8,
                        mode,
                    },
                    score,
                );
            } else if score > second {
                second = score;
            }
        }
    }
    (Some(best.0), best.1 - second)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_a_major_triad() {
        let mut pitch_classes = [0.0; 12];
        pitch_classes[0] = 1.0;
        pitch_classes[4] = 1.0;
        pitch_classes[7] = 1.0;
        let (key, _) = detect_key(pitch_classes);
        assert_eq!(
            key,
            Some(DetectedKey {
                root: 0,
                mode: KeyMode::Major
            })
        );
    }
}
