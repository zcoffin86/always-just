# Always Just

Always Just is a Rust-based, real-time-oriented microtonal synthesizer. It adjusts active MIDI pitches dynamically toward adaptive Just Intonation, reducing beat frequencies in polyphonic music. The current implementation includes MIDI-file parsing and offline WAV rendering, plus live MIDI input and audio output, several oscillator waveforms, heuristic key detection, and key-based Just Intonation tuning.

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

Key-selection hysteresis can be scaled from `0` to `1` on live input, MIDI rendering, and the demo:

```sh
cargo run -- realtime "PSR-295/293" --key-hysteresis 0
cargo run -- render input.mid output.wav --key-hysteresis 0.5
cargo run -- demo demo.wav 2 --key-hysteresis 0
```

The default is `1`, which uses the current full key-switch thresholds (0.5 score points normally and 2.5 when the current tonic is the held bass). Intermediate values proportionally scale both thresholds. `0` removes the margin: the detector switches when a competing key scores strictly higher, but does not switch on a tie. The chosen scale is printed in the realtime and render diagnostics. Values outside `0..1` are rejected.

For realtime playback, the first positional argument is an optional substring of the MIDI input port name. The next optional arguments select the oscillator waveform and output buffer size, in that order:

```sh
cargo run -- realtime "PSR-295/293" square
cargo run -- realtime "PSR-295/293" saw
cargo run -- realtime "PSR-295/293" triangle
cargo run -- realtime "PSR-295/293" pwm
cargo run -- realtime "PSR-295/293" sine 128
```

Available waveforms are `sine`, `square`, `saw`, `triangle`, and `pwm` (a fixed 25% pulse-width waveform). The final optional argument sets the output buffer size in frames; it defaults to `256`. Smaller values can reduce latency but are more likely to underrun, while larger values improve stability at the cost of latency. Omit the port filter to use the first available MIDI input. The command prints the selected MIDI port and audio output device, responds to MIDI note-on and note-off messages, and waits for Enter before stopping.

The real-time path requests a 256-frame output buffer (about 5.8 ms at 44.1 kHz). MIDI callbacks enqueue events into a bounded queue, while the audio callback owns and renders the synthesizer without taking a mutex or allocating a sample buffer on every callback. A slightly larger buffer is intentional: very small ALSA buffers can reduce latency but are more likely to underrun on systems whose audio scheduler cannot reliably service them.

While playing, each MIDI note-on or note-off recalculates the key from the currently held notes and updates the tuning targets. A single held note does not select a key, so its target returns to 12-TET. Releasing a note removes it from key detection immediately, although its short release envelope may still be audible. Each event prints the current detected key and the tuning applied to every held note, including the exact target ratio:

```text
MIDI update: key=D major, voices=3
  ch  1 note  62:   293.66 Hz (+0.00 cents, ratio 1:1)
  ch  1 note  66:   366.96 Hz (-13.69 cents, ratio 5:4)
  ch  1 note  69:   440.00 Hz (+1.96 cents, ratio 3:2)
```

When no key is selected, diagnostics identify the target as `12-TET` rather than showing a Just Intonation ratio. Tuned targets are smoothed over approximately 15 ms by the synthesizer.

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

Key detection uses only the currently held notes, so released notes stop influencing the result immediately. A single active note does not select a key and remains at 12-TET; Just Intonation adjustments engage only when at least two notes sound together. Major/minor tonal profiles rank candidates using pitch classes plus octave-aware register information: the lowest sounding MIDI note and its intervals to the other active notes provide bass and voicing evidence. An existing key is retained unless a replacement has a positive score advantage that meets the configured hysteresis margin. When the existing tonic is still held as the lowest note, the full margin is 2.5 score points; otherwise it is 0.5. The detected key's tonic, rather than necessarily the lowest currently sounding note, is used as the Just Intonation tuning root. The `--key-hysteresis` option scales both margins from zero to one. Brief or ambiguous passages can still produce uncertain estimates, so the confidence gap remains visible in the diagnostics.

The tuning table maps pitch-class intervals from the detected tonic to exact ratios, with mode-specific entries. For example, a minor-key harmonic seventh uses `7:4` (about -31.17 cents from 12-TET), while the major seventh uses `15:8` (about -11.73 cents). Both the ratio and cents offset are shown per voice in realtime output.

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

## Current Rust Implementation

The local Rust implementation provides deterministic synthesis and offline rendering as well as optional live MIDI/audio I/O. It includes polyphonic note tracking, 12-TET MIDI frequency conversion, a Standard MIDI File parser (format 0/1, tempo changes, and note events), heuristic major/minor key detection with configurable hysteresis, mode-dependent Just Intonation targets, frequency smoothing, diagnostic output, and mono 16-bit PCM WAV output. The realtime path uses MIDI input and the default audio output device; the core tuning and rendering logic remains testable without audio hardware.

```sh
cargo check
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo test <test_name_substring>
cargo run -- demo demo.wav 2
cargo run -- render input.mid output.wav
cargo run -- realtime "MIDI device name" --key-hysteresis 0
```

The offline renderer and live audio path share the same synthesizer and key-detection behavior. The core synthesis and MIDI logic is kept testable without audio hardware.

### Current source layout

The implementation is organized around a small Cargo crate. Live MIDI and audio use the `midir` and `cpal` backends; pure tuning, synthesis, and offline-rendering tests do not require audio hardware:

- `src/midi.rs` parses supported MIDI events and converts ticks to seconds.
- `src/synth.rs` tracks voices, recalculates tuning on note-on and note-off, smooths target-frequency changes, and renders the selected waveform with short attack/release ramps.
- `src/diagnostics.rs` analyzes polyphony, estimates keys from currently held notes, and applies configurable key-switch hysteresis.
- `src/tuning.rs` maps the detected key and active notes to Just Intonation target frequencies and exposes each target ratio for diagnostics.
- `src/wav.rs` writes mono 16-bit PCM WAV files.
- `src/realtime.rs` connects live MIDI input to the synthesizer and the default audio output.
- `src/main.rs` provides the `demo`, `render`, and `realtime` commands.
