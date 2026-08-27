Here is a comprehensive Software Design Description (SDD) for **Always Just**. It focuses on dynamic microtonal pitch adjustment, real-time polyphonic MIDI processing, and modular audio synthesis.

---

# Software Design Description: Always Just

## 1. System Overview

**Always Just** is a real-time dynamic microtonal audio synthesizer designed to adjust MIDI pitches dynamically on-the-fly, reducing acoustic beat frequencies by enforcing adaptive **Just Intonation (JI)**. Unlike static custom tunings, the system dynamically analyzes active pitch combinations and applies continuous microtonal offsets to achieve pure harmonic intervals for played chords.

```
+--------------------+      +-----------------------+      +---------------------+
| MIDI Input Source  | ---> | Dynamic Pitch Engine  | ---> | Synthesis Engine    |
| (Live / MIDI File) |      | (Root & Interval Calc)|      | (Custom Chiptune)   |
+--------------------+      +-----------------------+      +---------------------+
                                                                      |
                                                                      v
                                                           +---------------------+
                                                           | Real-Time Audio Out |
                                                           | (Stereo Buffer)     |
                                                           +---------------------+

```

---

## 2. Core Architectural Components

### A. MIDI Processing & Note State Tracking

Maintains an internal table of all active sounding notes to evaluate polyphonic structures in real time.

* **Note Pool:** Tracks note-on/note-off events, note age (velocity-weighted for root detection), and channel assignments.
* **Pitch-to-Frequency Conversion:** Translates 12-Tone Equal Temperament (12-TET) MIDI note numbers into base frequencies ($f_0 = 440 \times 2^{(n-69)/12}$).

### B. Dynamic Tuning Engine (DTE)

Calculates real-time microtonal pitch adjustments ($\Delta f$) based on active harmonic ratios.

* **Root Note Identification:** Determines the nominal root of the active polyphonic chord.
* *Strategy:* Uses lowest active pitch or pitch-class histogram matching (triads, 7th chords).


* **Just Ratio Mapping:** Maps active intervals relative to the root using exact whole-number ratios (e.g., $3:2$ for perfect fifths, $5:4$ for major thirds).
* **Frequency Smoothing:** Applies exponential smoothing to frequency target updates to avoid audible pitch clicks:

$$f_{\text{target}}(t) = f_{\text{current}} + (1 - e^{-t / \tau}) \cdot (f_{\text{just}} - f_{\text{current}})$$



### C. Audio & Synthesis Engine

A modular voice allocator driving configurable waveform generators.

* **Voice Allocator:** Manages active voice instances, routing target dynamic frequencies and envelope states.
* **Waveform Oscillators:**
* *Sine Wave:* Pure reference oscillator.
* *NES/SNES Engine (Chiptune):* Pulse waves (12.5%, 25%, 50% duty cycles), triangle wave (bass), pseudo-random noise generator, and 4-bit sample table options.


* **ADSR Envelope Generator:** Applies amplitude envelopes to eliminate clicks on note state changes.

---

## 3. Just Intonation Mapping Table

| Interval | 12-TET Ratio | Just Ratio | Offset (Cents) |
| --- | --- | --- | --- |
| Unison / Octave | 1.0000 | 1:1 / 2:1 | 0.00 |
| Minor Third | 1.1892 | 6:5 | +15.64 |
| Major Third | 1.2599 | 5:4 | -13.69 |
| Perfect Fourth | 1.3348 | 4:3 | -1.96 |
| Perfect Fifth | 1.4983 | 3:2 | +1.96 |
| Minor Seventh | 1.7818 | 7:4 (Harmonic) | -31.17 |
| Major Seventh | 1.8877 | 15:8 | -11.73 |

---

## 4. Class Design & Data Structures

```
                      +-------------------+
                      |   AudioEngine     |
                      +-------------------+
                                |
                   +------------+------------+
                   |                         |
        +--------------------+    +--------------------+
        | DynamicTuningEngine|    |   VoiceAllocator   |
        +--------------------+    +--------------------+
                   |                         |
                   v                         v
        +--------------------+    +--------------------+
        |   HarmonicSolver   |    |    SynthVoice      |
        +--------------------+    +--------------------+
                                             |
                                  +----------+----------+
                                  |                     |
                       +--------------------+ +--------------------+
                       |  PulseOscillator   | |   ADSR Envelope    |
                       +--------------------+ +--------------------+

```

### Core Interface Definitions

```cpp
struct NoteEvent {
    uint8_t midiNote;
    uint8_t velocity;
    double timestamp;
};

class DynamicTuningEngine {
public:
    // Takes currently active MIDI notes and returns exact frequencies in Hz
    std::unordered_map<uint8_t, double> calculateTunedFrequencies(
        const std::vector<NoteEvent>& activeNotes
    );
private:
    uint8_t detectRoot(const std::vector<NoteEvent>& notes);
    double getJustRatio(uint8_t root, uint8_t targetNote);
};

class SynthVoice {
public:
    void trigger(double frequency, float velocity);
    void release();
    void setFrequency(double targetFrequency); // Smooth interpolation
    float renderNextSample();
private:
    double currentPhase;
    double currentFrequency;
    double targetFrequency;
    WaveformType waveform; // Sine, Pulse25, Pulse50, NES_Triangle
    EnvelopeState adsr;
};

```

---

## 5. Execution Sequence for Real-Time Playback

```
[MIDI Input Event] -> [Note Tracker Updates Active Pool]
                                  |
                                  v
                    [DynamicTuningEngine Computes]
                    1. Identify Root Note
                    2. Interval Distance = (Note - Root) % 12
                    3. Target Hz = Root_Hz * JustRatio[Interval]
                                  |
                                  v
                    [Voice Allocator]
                    1. Assigns/Updates Active SynthVoices
                    2. Interpolates target frequencies over 15-20ms
                                  |
                                  v
                    [Audio Thread (Callback)]
                    1. Render active voices to buffer
                    2. Clip/Soft-limit mix output
                    3. Send buffer to Audio Hardware

```

---

## 6. Development Phasing

* **Phase 1 (Proof of Concept):** Pure sine wave synthesis, basic monophonic/polyphonic 12-TET fallback, MIDI file parsing via standard offline audio export.
* **Phase 2 (Dynamic Engine):** Implementation of the `HarmonicSolver` root-detection algorithm and real-time frequency smoothing to eliminate pitch clicks.
* **Phase 3 (Chiptune Synthesis):** NES pulse/triangle wave generation with simple ADSR, bandlimited synthesis (BLEP) to eliminate aliasing.
* **Phase 4 (UI & Host Integration):** Waveform visualization, tuning offset display (cents shift indicator), and optional plugin wrappers (VST3/CLAP).