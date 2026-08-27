# Copilot Instructions for Always Just

## Repository Status

Always Just is currently a local design-stage project. `README.md` is the Software Design Description (SDD); there is not yet a Rust workspace, Cargo manifest, source tree, build system, test suite, or CI configuration. Do not assume that the C++ snippets in the SDD are implemented code: they are language-agnostic design sketches that should be translated into idiomatic Rust as implementation begins.

Work locally by default. Do not add GitHub Actions, remote integrations, or repository-hosting assumptions unless explicitly requested.

## Product and Architecture

Always Just is intended to be a real-time, polyphonic microtonal synthesizer. It receives live MIDI events or MIDI-file input, tracks active notes, determines a harmonic root, computes Just Intonation targets, smooths pitch changes, renders voices, and sends mixed audio to the output device.

The main runtime flow is:

```text
MIDI input
  -> active-note state
  -> dynamic tuning / root detection
  -> voice allocation and target-frequency smoothing
  -> oscillator and ADSR rendering
  -> mix, limiting, and audio output
```

Keep the boundaries between these stages explicit. The tuning engine should calculate musical targets independently of audio-device concerns, and voice rendering should be usable in deterministic offline tests.

### Core responsibilities

- **Note tracking:** Maintain active notes, velocity, timestamps/age, and MIDI channel information; handle note-on, note-off, and repeated notes predictably.
- **Dynamic tuning:** Convert MIDI notes to 12-TET reference frequencies, identify a root, map pitch-class intervals to Just Intonation ratios, and return target frequencies.
- **Voice allocation:** Create, update, release, and reclaim polyphonic voices without putting allocation or locking in the real-time audio callback.
- **Synthesis:** Render sine first, then the planned NES-style pulse, triangle, noise, and 4-bit sample options. Apply ADSR envelopes to prevent clicks.
- **Output:** Mix active voices, apply the planned clipping/soft-limiting stage, and hand buffers to the selected audio backend.

The SDD describes lowest-note root detection as the simple initial strategy and pitch-class histogram matching as a more robust option for inversions. Preserve the strategy as a replaceable policy rather than coupling it to MIDI or oscillator code.

### Musical calculations

The 12-TET reference frequency is:

```text
f0 = 440 * 2^((midi_note - 69) / 12)
```

Just ratios are relative to the detected root. The README specifies examples including 6:5 (minor third), 5:4 (major third), 4:3 (perfect fourth), 3:2 (perfect fifth), 7:4 (harmonic minor seventh), and 15:8 (major seventh). Keep ratio data in typed tables or structures rather than scattering magic numbers through the DSP code.

Frequency changes should be smoothed with the SDD's exponential interpolation model over roughly 15–20 ms. Make sample rate and smoothing duration explicit parameters so behavior is deterministic and testable.

## Rust Implementation Conventions

- Use a Cargo workspace only when multiple independently buildable crates are justified; start with a single library or binary crate rather than inventing a large hierarchy.
- Prefer small, typed domain models such as `NoteEvent`, `ActiveNote`, `IntervalRatio`, `TuningResult`, `SynthVoice`, and `Waveform`.
- Use Rust naming conventions: `UpperCamelCase` for types and traits, `snake_case` for functions and fields, and `SCREAMING_SNAKE_CASE` only for true constants.
- Keep pure music theory and DSP calculations in modules that do not require an audio device. This allows unit tests to run without hardware.
- Represent invalid MIDI values, unsupported intervals, and configuration errors with explicit `Result`/`Option` types or domain errors. Do not silently fall back when the caller needs to know that tuning data is unavailable.
- Prefer fixed-capacity or preallocated buffers in the audio path. Avoid heap allocation, blocking locks, filesystem access, logging, and unbounded work inside the audio callback.
- Communicate control changes to the audio thread through lock-free or otherwise real-time-safe mechanisms selected for the eventual audio backend. Do not hold a mutex across rendering.
- Keep phase, frequency, envelope, and oscillator state owned by each voice. A voice should expose clear control operations and deterministic sample/block rendering.
- Use `f32` for audio sample buffers where the chosen backend expects it; use `f64` for calculations only where the added precision is meaningful and does not complicate the real-time path.
- Document any backend-specific assumptions (sample rate, channel layout, buffer size, MIDI timestamp units) at the integration boundary instead of hiding them in core DSP code.

Suggested module boundaries, once code exists, are `midi`, `tuning`, `synthesis`, `voice`, and `audio`, but follow the actual dependency graph and avoid creating empty modules prematurely.

## Development Phases

Implement in the order described by the SDD:

1. Pure sine synthesis with deterministic rendering and basic monophonic/polyphonic note tracking.
2. Dynamic root detection, Just Intonation mapping, and smoothed target-frequency updates.
3. NES-style oscillators, ADSR, aliasing control such as BLEP where needed, and offline rendering coverage.
4. Visualization and optional host/plugin integration such as VST3 or CLAP.

Do not introduce plugin or UI dependencies into the core tuning/DSP layer. Keep hardware and host integration at the edges.

## Build, Test, and Lint

There are currently no repository-defined build, test, or lint commands because the Rust project has not been initialized.

After adding Cargo metadata, use the commands defined by that project:

- Build/check: `cargo check` (or `cargo build` when a compiled artifact is needed)
- Format: `cargo fmt --all -- --check`; apply formatting with `cargo fmt --all`
- Lint: `cargo clippy --all-targets --all-features -- -D warnings`
- Full tests: `cargo test`
- One test: `cargo test <test_name_substring>`; for an exact integration-test target, use `cargo test --test <target> <test_name_substring>`

Add repository-specific flags or package selectors here only after they exist in `Cargo.toml` or project documentation. Tests for tuning math, frequency conversion, smoothing, envelopes, and offline voice rendering should remain runnable without MIDI or audio hardware.

## Reference

Use `README.md` as the source of truth for the current product intent, data-flow diagrams, Just Intonation examples, frequency formulas, and phased scope. Update this file when the implementation establishes concrete commands, module boundaries, backend choices, or conventions that future sessions must follow.
