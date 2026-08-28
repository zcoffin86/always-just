# Always Just

Always Just is a Rust-based, real-time-oriented microtonal synthesizer. It adjusts active MIDI pitches dynamically toward adaptive Just Intonation, reducing beat frequencies in polyphonic music. The current implementation is an offline renderer with the Phase 1 sine-synthesis foundation and a Phase 2 key-based tuning engine.

The project is intentionally local for now. MIDI and WAV files can live in a local music directory or any other local directory; no GitHub repository or online service is required to build or run it.

## Quick Start

Install Rust and Cargo, plus the native build tools required by Rust's target platform. These tools are not Rust crate dependencies, but they are typical for Rust applications that produce native executables: Cargo invokes the platform linker during `cargo build`, `cargo run`, and test execution. On Linux this usually means a C compiler driver such as `cc` or `gcc`, `pkg-config`, and ALSA development headers for real-time MIDI/audio support; macOS requires Apple's Xcode Command Line Tools; Windows requires the MSVC or GNU build tools appropriate to the selected Rust toolchain.

For Debian or Ubuntu:

```sh
sudo apt install build-essential pkg-config libasound2-dev
```

For Fedora:

```sh
sudo dnf groupinstall "Development Tools"
```

For macOS:

```sh
xcode-select --install
```

Then install Rust and Cargo with [rustup](https://rustup.rs/) and run:

```sh
cargo check
cargo test
cargo run -- demo demo.wav 2
```

On Linux and macOS, Cargo uses the platform's native linker. Install the standard C build tools for your operating system if Rust reports that `cc` or another system linker is missing; the project deliberately does not pin a machine-specific linker path.

The demo writes a two-second C-major chord to `demo.wav`. To render a Standard MIDI File:

```sh
cargo run -- render \
  "/path/to/Music/pachelbel_canon_and_gigue_(c)icking-archive.mid" \
  "/path/to/Music/pachelbel_canon_and_gigue_phase1.wav"
```

Add an optional waveform (`sine`, `square`, `saw`, `triangle`, or `pwm`) as the final argument:

```sh
cargo run -- render \
  "/path/to/Music/pachelbel_canon_and_gigue_(c)icking-archive.mid" \
  "/path/to/Music/pachelbel_canon_and_gigue_square.wav" \
  square
```

The `render` command prints diagnostics to the terminal before writing the WAV. The output file is mono, 16-bit PCM at 44.1 kHz. If omitted, the waveform defaults to sine.

To use a connected MIDI keyboard with the default audio output:

```sh
cargo run -- realtime "PSR-295/293"
```

The first optional argument is a substring of the MIDI input port name. The second optional argument selects the oscillator waveform:

```sh
cargo run -- realtime "PSR-295/293" square
cargo run -- realtime "PSR-295/293" saw
cargo run -- realtime "PSR-295/293" triangle
cargo run -- realtime "PSR-295/293" pwm
```

Available waveforms are `sine`, `square`, `saw`, `triangle`, and `pwm` (a fixed 25% pulse-width waveform). Omit the port filter to use the first available MIDI input. The command prints the selected MIDI port and audio output device, responds to MIDI note-on and note-off messages, and waits for Enter before stopping.

The real-time path requests a 256-frame output buffer (about 5.8 ms at 44.1 kHz). MIDI callbacks enqueue events into a bounded queue, while the audio callback owns and renders the synthesizer without taking a mutex or allocating a sample buffer on every callback. A slightly larger buffer is intentional: very small ALSA buffers can reduce latency but are more likely to underrun on systems whose audio scheduler cannot reliably service them.

While playing, each MIDI note change also prints the current detected key and the tuning applied to every active note:

```text
MIDI update: key=D major, voices=3
  ch  1 note  62:   293.66 Hz (+0.00 cents)
  ch  1 note  66:   366.96 Hz (-13.69 cents)
  ch  1 note  69:   440.00 Hz (+1.96 cents)
```

For visual verification, use the offline renderer to create a WAV, then open it in an audio editor such as Audacity. The waveform view can reveal clicks or discontinuities; a spectrogram or spectrum view can show the sine partials and frequency movement. Real-time terminal diagnostics confirm the MIDI events and tuning decisions but cannot display the audio waveform itself.

### Finding MIDI test files

The Pachelbel Canon MIDI used during Phase 1 testing, along with other useful classical MIDI files, can be downloaded from [Kunst der Fuge's Pachelbel page](https://www.kunstderfuge.com/pachelbel.htm). You can use any compatible MIDI files; place them in a local directory such as `~/Music` or pass an explicit path to `cargo run -- render`.

## Diagnostics

For each MIDI render, the application reports:

- Input path and estimated render duration
- Number of note-on and note-off events
- Maximum simultaneous voices observed
- Heuristic key changes with timestamps
- Active voice count and a confidence gap for each key estimate

Key detection uses only the currently held pitch classes, so released notes stop influencing the result immediately. A single active note does not select a key and remains at 12-TET; Just Intonation adjustments engage only when at least two notes sound together. Major/minor tonal profiles are used to rank candidates. The detected key's tonic, rather than the lowest currently sounding note, is used as the Just Intonation tuning root. Brief or ambiguous passages can still produce uncertain estimates, so the confidence gap remains visible in the diagnostics.

Example output:

```text
diagnostics:
  input: /path/to/Music/pachelbel_canon_and_gigue_(c)icking-archive.mid
  duration: 433.00s
  note-ons: 3032
  note-offs: 3036
  maximum simultaneous voices: 4
  key changes:
        1.50s  D major   active voices:  1  confidence gap: 0.46
       18.00s  B minor   active voices:  2  confidence gap: 1.49
       24.00s  D major   active voices:  3  confidence gap: 1.65
```

Diagnostics from the Pachelbel test render are kept in `diagnostics/pachelbel-diagnostics.log`. This is an example artifact rather than required application input.

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

## Phase 1 Rust Proof of Concept

The first local implementation is a dependency-free Rust command-line renderer. It provides deterministic sine-wave synthesis with short attack/release ramps, polyphonic note tracking, 12-TET MIDI frequency conversion, a minimal Standard MIDI File parser (format 0/1, tempo changes, and note events), diagnostic output with active-voice statistics and heuristic key-change detection, and mono 16-bit PCM WAV output.

```sh
cargo check
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo test <test_name_substring>
cargo run -- demo demo.wav 2
cargo run -- render input.mid output.wav
```

The renderer is intentionally offline; live MIDI devices and real-time audio backends are planned for later phases. The core synthesis and MIDI modules are kept independent of audio hardware so they can be tested deterministically.

### Current source layout

The implementation is organized around a small dependency-free Cargo crate:

- `src/midi.rs` parses supported MIDI events and converts ticks to seconds.
- `src/synth.rs` tracks voices and renders sine samples with short attack/release ramps.
- `src/diagnostics.rs` analyzes polyphony and estimates key movement for terminal output.
- `src/tuning.rs` maps the detected key and active notes to Just Intonation target frequencies.
- `src/wav.rs` writes mono 16-bit PCM WAV files.
- `src/main.rs` provides the `demo` and `render` commands.
