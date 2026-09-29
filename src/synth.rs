use std::collections::HashMap;

use crate::tuning::target_frequency;
use crate::{
    diagnostics::{HysteresisScale, KeyDetector},
    midi::{MidiEvent, MidiKind},
    midi_note_frequency, SAMPLE_RATE,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Waveform {
    #[default]
    Sine,
    Square,
    Saw,
    Triangle,
    Pwm,
}

impl Waveform {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "sine" => Some(Self::Sine),
            "square" | "pulse" => Some(Self::Square),
            "saw" | "sawtooth" => Some(Self::Saw),
            "triangle" | "tri" => Some(Self::Triangle),
            "pwm" => Some(Self::Pwm),
            _ => None,
        }
    }

    fn sample(self, phase: f32) -> f32 {
        match self {
            Self::Sine => (phase * std::f32::consts::TAU).sin(),
            Self::Square => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Self::Saw => 2.0 * phase - 1.0,
            Self::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
            Self::Pwm => {
                if phase < 0.25 {
                    1.0
                } else {
                    -1.0
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ActiveNote {
    pub channel: u8,
    pub note: u8,
    pub velocity: f32,
}

#[derive(Default)]
pub struct NoteTracker {
    notes: HashMap<(u8, u8), ActiveNote>,
}

impl NoteTracker {
    pub fn handle(&mut self, event: MidiEvent) {
        match event.kind {
            MidiKind::NoteOn { note, velocity } => {
                if velocity > 0 {
                    self.notes.insert(
                        (event.channel, note),
                        ActiveNote {
                            channel: event.channel,
                            note,
                            velocity: f32::from(velocity) / 127.0,
                        },
                    );
                } else {
                    self.notes.remove(&(event.channel, note));
                }
            }
            MidiKind::NoteOff { note } => {
                self.notes.remove(&(event.channel, note));
            }
        }
    }

    pub fn active(&self) -> impl Iterator<Item = ActiveNote> + '_ {
        self.notes.values().copied()
    }
}

#[derive(Clone, Copy, Debug)]
struct Voice {
    channel: u8,
    note: u8,
    phase: f32,
    frequency: f32,
    target_frequency: f32,
    amplitude: f32,
    level: f32,
    state: VoiceState,
    release_samples: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VoiceState {
    Attacking,
    Sustaining,
    Releasing,
}

pub struct Synthesizer {
    voices: Vec<Voice>,
    sample_rate: f32,
    waveform: Waveform,
    attack_samples: usize,
    release_samples: usize,
    smoothing_alpha: f32,
    detector: KeyDetector,
}

impl Synthesizer {
    pub fn new(sample_rate: u32) -> Self {
        Self::with_waveform(sample_rate, Waveform::Sine)
    }

    pub fn with_waveform(sample_rate: u32, waveform: Waveform) -> Self {
        Self::with_waveform_and_hysteresis(sample_rate, waveform, HysteresisScale::DEFAULT)
    }

    pub fn with_waveform_and_hysteresis(
        sample_rate: u32,
        waveform: Waveform,
        hysteresis_scale: HysteresisScale,
    ) -> Self {
        Self {
            voices: Vec::new(),
            sample_rate: sample_rate as f32,
            waveform,
            attack_samples: ((sample_rate as f32 * 0.005).round() as usize).max(1),
            release_samples: ((sample_rate as f32 * 0.01).round() as usize).max(1),
            smoothing_alpha: 1.0 - (-1.0 / (sample_rate as f32 * 0.015)).exp(),
            detector: KeyDetector::with_hysteresis_scale(hysteresis_scale),
        }
    }

    pub fn waveform(&self) -> Waveform {
        self.waveform
    }

    pub fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        if velocity == 0 {
            self.note_off(channel, note);
            return;
        }
        self.voices
            .retain(|voice| (voice.channel, voice.note) != (channel, note));
        self.voices.push(Voice {
            channel,
            note,
            phase: 0.0,
            frequency: midi_note_frequency(note),
            target_frequency: midi_note_frequency(note),
            amplitude: f32::from(velocity) / 127.0 * 0.2,
            level: 0.0,
            state: VoiceState::Attacking,
            release_samples: 0,
        });
    }

    pub fn note_off(&mut self, channel: u8, note: u8) {
        for voice in &mut self.voices {
            if (voice.channel, voice.note) == (channel, note)
                && voice.state != VoiceState::Releasing
            {
                voice.state = VoiceState::Releasing;
                voice.release_samples = self.release_samples;
            }
        }
    }

    pub fn handle_midi_event(&mut self, event: MidiEvent) -> TuningDiagnostic {
        match event.kind {
            MidiKind::NoteOn { note, velocity } => {
                self.note_on(event.channel, note, velocity);
            }
            MidiKind::NoteOff { note } => self.note_off(event.channel, note),
        }
        self.detector.update_active(
            event.time,
            [],
            self.voices
                .iter()
                .filter(|voice| voice.state != VoiceState::Releasing)
                .map(|voice| voice.note),
        );
        self.apply_key(self.detector.current());
        let key = self.detector.current();
        let notes = self
            .voices
            .iter()
            .filter(|voice| voice.state != VoiceState::Releasing)
            .map(|voice| {
                let tuned = key.map(|key| target_frequency(key, voice.note));
                TuningNote {
                    channel: voice.channel,
                    note: voice.note,
                    velocity: voice.amplitude / 0.2,
                    frequency: tuned
                        .map_or(midi_note_frequency(voice.note), |value| value.frequency),
                    cents_offset: tuned.map_or(0.0, |value| value.cents_offset),
                    ratio: tuned.map(|value| value.ratio),
                }
            })
            .collect();
        TuningDiagnostic { key, notes }
    }

    fn apply_key(&mut self, key: Option<crate::diagnostics::DetectedKey>) -> f32 {
        let Some(key) = key else {
            for voice in &mut self.voices {
                if voice.state != VoiceState::Releasing {
                    voice.target_frequency = midi_note_frequency(voice.note);
                }
            }
            return 0.0;
        };
        let mut maximum: f32 = 0.0;
        for voice in &mut self.voices {
            if voice.state == VoiceState::Releasing {
                continue;
            }
            let tuned = target_frequency(key, voice.note);
            voice.target_frequency = tuned.frequency;
            maximum = maximum.max(tuned.cents_offset.abs());
        }
        maximum
    }

    pub fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames];
        self.render_into(&mut output);
        output
    }

    pub fn render_into(&mut self, output: &mut [f32]) {
        output.fill(0.0);
        for sample in output.iter_mut() {
            *sample = self.render_sample();
        }
    }

    pub fn render_sample(&mut self) -> f32 {
        let mut sample = 0.0;
        for voice in &mut self.voices {
            voice.frequency += (voice.target_frequency - voice.frequency) * self.smoothing_alpha;
            match voice.state {
                VoiceState::Attacking => {
                    voice.level = (voice.level + 1.0 / self.attack_samples as f32).min(1.0);
                    if voice.level >= 1.0 {
                        voice.state = VoiceState::Sustaining;
                    }
                }
                VoiceState::Sustaining => {}
                VoiceState::Releasing => {
                    voice.level = (voice.level - 1.0 / self.release_samples as f32).max(0.0);
                    voice.release_samples = voice.release_samples.saturating_sub(1);
                }
            }
            sample += self.waveform.sample(voice.phase) * voice.amplitude * voice.level;
            voice.phase = (voice.phase + voice.frequency / self.sample_rate) % 1.0;
        }
        self.voices
            .retain(|voice| voice.state != VoiceState::Releasing || voice.level > 0.0);
        sample.clamp(-1.0, 1.0)
    }

    pub fn render_events(events: &[MidiEvent], duration: f32) -> Vec<f32> {
        Self::render_events_with_report(events, duration, Waveform::Sine).0
    }

    pub fn render_events_with_report(
        events: &[MidiEvent],
        duration: f32,
        waveform: Waveform,
    ) -> (Vec<f32>, RenderReport) {
        Self::render_events_with_report_and_hysteresis(
            events,
            duration,
            waveform,
            HysteresisScale::DEFAULT,
        )
    }

    pub fn render_events_with_report_and_hysteresis(
        events: &[MidiEvent],
        duration: f32,
        waveform: Waveform,
        hysteresis_scale: HysteresisScale,
    ) -> (Vec<f32>, RenderReport) {
        let mut synth = Self::with_waveform_and_hysteresis(SAMPLE_RATE, waveform, hysteresis_scale);
        let mut detector = KeyDetector::with_hysteresis_scale(hysteresis_scale);
        let mut report = RenderReport::default();
        let total_frames = (duration.max(0.0) * SAMPLE_RATE as f32).ceil() as usize;
        let mut output = Vec::with_capacity(total_frames);
        let mut event_index = 0;
        for frame in 0..total_frames {
            let time = frame as f32 / SAMPLE_RATE as f32;
            while let Some(event) = events.get(event_index).filter(|event| event.time <= time) {
                match event.kind {
                    MidiKind::NoteOn { note, velocity } => {
                        synth.note_on(event.channel, note, velocity);
                    }
                    MidiKind::NoteOff { note } => synth.note_off(event.channel, note),
                }
                detector.update_active(
                    event.time,
                    [],
                    synth
                        .voices
                        .iter()
                        .filter(|voice| voice.state != VoiceState::Releasing)
                        .map(|voice| voice.note),
                );
                report.max_tuning_offset_cents = report
                    .max_tuning_offset_cents
                    .max(synth.apply_key(detector.current()));
                event_index += 1;
            }
            output.extend(synth.render(1));
        }
        (output, report)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderReport {
    pub max_tuning_offset_cents: f32,
}

#[derive(Clone, Debug)]
pub struct TuningDiagnostic {
    pub key: Option<crate::diagnostics::DetectedKey>,
    pub notes: Vec<TuningNote>,
}

#[derive(Clone, Copy, Debug)]
pub struct TuningNote {
    pub channel: u8,
    pub note: u8,
    pub velocity: f32,
    pub frequency: f32,
    pub cents_offset: f32,
    pub ratio: Option<crate::tuning::Ratio>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_note_on_and_off() {
        let mut tracker = NoteTracker::default();
        tracker.handle(MidiEvent {
            time: 0.0,
            channel: 2,
            kind: MidiKind::NoteOn {
                note: 60,
                velocity: 127,
            },
        });
        assert_eq!(tracker.active().count(), 1);
        tracker.handle(MidiEvent {
            time: 1.0,
            channel: 2,
            kind: MidiKind::NoteOff { note: 60 },
        });
        assert_eq!(tracker.active().count(), 0);
    }

    #[test]
    fn renders_a_non_silent_sine_wave() {
        let events = [MidiEvent {
            time: 0.0,
            channel: 0,
            kind: MidiKind::NoteOn {
                note: 69,
                velocity: 127,
            },
        }];
        let samples = Synthesizer::render_events(&events, 0.01);
        assert_eq!(samples.len(), 441);
        assert!(samples.iter().any(|sample| sample.abs() > 0.01));
    }

    #[test]
    fn preserves_midi_velocity_in_voice_diagnostics() {
        let mut synth = Synthesizer::new(SAMPLE_RATE);
        let diagnostic = synth.handle_midi_event(MidiEvent {
            time: 0.0,
            channel: 0,
            kind: MidiKind::NoteOn {
                note: 60,
                velocity: 64,
            },
        });
        assert_eq!(diagnostic.notes.len(), 1);
        assert!((diagnostic.notes[0].velocity - 64.0 / 127.0).abs() < 0.001);
    }

    #[test]
    fn recalculates_tuning_when_a_note_is_removed() {
        let mut synth = Synthesizer::new(SAMPLE_RATE);
        synth.handle_midi_event(MidiEvent {
            time: 0.0,
            channel: 0,
            kind: MidiKind::NoteOn {
                note: 60,
                velocity: 100,
            },
        });
        let chord = synth.handle_midi_event(MidiEvent {
            time: 0.1,
            channel: 0,
            kind: MidiKind::NoteOn {
                note: 64,
                velocity: 100,
            },
        });
        assert_eq!(
            chord.key,
            Some(crate::diagnostics::DetectedKey {
                root: 0,
                mode: crate::diagnostics::KeyMode::Major,
            })
        );
        assert_eq!(chord.notes[1].ratio, Some(crate::tuning::Ratio::new(5, 4)));

        let remaining_note = synth.handle_midi_event(MidiEvent {
            time: 0.2,
            channel: 0,
            kind: MidiKind::NoteOff { note: 60 },
        });
        assert_eq!(remaining_note.key, None);
        assert_eq!(remaining_note.notes.len(), 1);
        assert_eq!(remaining_note.notes[0].note, 64);
        assert_eq!(remaining_note.notes[0].ratio, None);
        assert_eq!(remaining_note.notes[0].frequency, midi_note_frequency(64));
        assert_eq!(remaining_note.notes[0].cents_offset, 0.0);
    }

    #[test]
    fn supports_non_sine_waveforms() {
        let mut synth = Synthesizer::with_waveform(SAMPLE_RATE, Waveform::Square);
        synth.note_on(0, 69, 127);
        let samples = synth.render(500);
        assert!(samples.iter().any(|sample| *sample > 0.1));
        assert!(samples.iter().any(|sample| *sample < -0.1));
    }

    #[test]
    fn ramps_voice_changes_instead_of_jumping() {
        let events = [
            MidiEvent {
                time: 0.0,
                channel: 0,
                kind: MidiKind::NoteOn {
                    note: 60,
                    velocity: 127,
                },
            },
            MidiEvent {
                time: 0.01,
                channel: 0,
                kind: MidiKind::NoteOff { note: 60 },
            },
        ];
        let samples = Synthesizer::render_events(&events, 0.02);
        let largest_step = samples
            .windows(2)
            .map(|window| (window[1] - window[0]).abs())
            .fold(0.0, f32::max);
        assert!(largest_step < 0.02);
    }
}
