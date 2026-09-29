use std::{env, error::Error, process};

use aways_just::{
    diagnostics::{self, HysteresisScale},
    midi, realtime,
    synth::{Synthesizer, Waveform},
    wav, SAMPLE_RATE,
};

fn print_diagnostics(
    path: &str,
    events: &[midi::MidiEvent],
    duration: f32,
    hysteresis_scale: HysteresisScale,
) {
    let analysis = diagnostics::analyze_with_hysteresis_scale(events, hysteresis_scale);
    println!("diagnostics:");
    println!("  input: {path}");
    println!("  duration: {duration:.2}s");
    println!("  key hysteresis: {:.2}", hysteresis_scale.value());
    println!("  note-ons: {}", analysis.note_on_count);
    println!("  note-offs: {}", analysis.note_off_count);
    println!("  maximum simultaneous voices: {}", analysis.max_polyphony);
    println!("  key changes:");
    if analysis.key_changes.is_empty() {
        println!("    (no active notes to analyze)");
    } else {
        for (time, key, voices, confidence) in analysis.key_changes {
            println!(
                "    {time:>8.2}s  {key:<8}  active voices: {voices:>2}  confidence gap: {confidence:.2}"
            );
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let (args, hysteresis_scale) = parse_hysteresis_argument(env::args().collect())?;
    match args.get(1).map(String::as_str) {
        Some("realtime") if (2..=5).contains(&args.len()) => {
            let waveform = args
                .get(3)
                .map(|value| Waveform::parse(value).ok_or_else(|| format!("unknown waveform: {value}")))
                .transpose()?
                .unwrap_or_default();
            let buffer_frames = args
                .get(4)
                .map(|value| {
                    let frames = value
                        .parse::<u32>()
                        .map_err(|_| format!("invalid audio buffer size: {value}"))?;
                    (frames > 0)
                        .then_some(frames)
                        .ok_or_else(|| "audio buffer size must be greater than zero".to_owned())
                })
                .transpose()?
                .unwrap_or(realtime::DEFAULT_BUFFER_FRAMES);
            realtime::run_with_hysteresis_scale(
                args.get(2).map(String::as_str),
                waveform,
                buffer_frames,
                hysteresis_scale,
            )
        }
        Some("render") if (4..=5).contains(&args.len()) => {
            let events = midi::read_file(&args[2])?;
            let duration = events.iter().map(|event| event.time).fold(0.0_f32, f32::max) + 1.0;
            print_diagnostics(&args[2], &events, duration, hysteresis_scale);
            let waveform = args
                .get(4)
                .map(|value| {
                    Waveform::parse(value).ok_or_else(|| format!("unknown waveform: {value}"))
                })
                .transpose()?
                .unwrap_or_default();
            let (samples, report) = Synthesizer::render_events_with_report_and_hysteresis(
                &events,
                duration,
                waveform,
                hysteresis_scale,
            );
            println!(
                "  maximum Just Intonation offset: {:.2} cents",
                report.max_tuning_offset_cents
            );
            wav::write_mono_16(&args[3], &samples, SAMPLE_RATE)?;
            println!("rendered {:.2}s to {}", duration, args[3]);
            Ok(())
        }
        Some("demo") if (3..=4).contains(&args.len()) => {
            let duration = args.get(3).map(|value| value.parse()).transpose()?.unwrap_or(2.0);
            let events = [
                midi::MidiEvent { time: 0.0, channel: 0, kind: midi::MidiKind::NoteOn { note: 60, velocity: 100 } },
                midi::MidiEvent { time: 0.0, channel: 0, kind: midi::MidiKind::NoteOn { note: 64, velocity: 80 } },
                midi::MidiEvent { time: 0.0, channel: 0, kind: midi::MidiKind::NoteOn { note: 67, velocity: 80 } },
                midi::MidiEvent { time: duration, channel: 0, kind: midi::MidiKind::NoteOff { note: 60 } },
                midi::MidiEvent { time: duration, channel: 0, kind: midi::MidiKind::NoteOff { note: 64 } },
                midi::MidiEvent { time: duration, channel: 0, kind: midi::MidiKind::NoteOff { note: 67 } },
            ];
            let (samples, _) = Synthesizer::render_events_with_report_and_hysteresis(
                &events,
                duration,
                Waveform::Sine,
                hysteresis_scale,
            );
            wav::write_mono_16(&args[2], &samples, SAMPLE_RATE)?;
            println!("rendered {:.2}s demo to {}", duration, args[2]);
            Ok(())
        }
        _ => Err("usage: aways-just demo <output.wav> [seconds]\n       aways-just render <input.mid> <output.wav> [sine|square|saw|triangle|pwm] [--key-hysteresis 0..1]\n       aways-just realtime [midi-port-filter] [sine|square|saw|triangle|pwm] [buffer-frames] [--key-hysteresis 0..1]".into()),
    }
}

fn parse_hysteresis_argument(
    args: Vec<String>,
) -> Result<(Vec<String>, HysteresisScale), Box<dyn Error>> {
    let mut filtered = Vec::with_capacity(args.len());
    let mut scale = None;
    let mut arguments = args.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == "--key-hysteresis" {
            if scale.is_some() {
                return Err("--key-hysteresis may only be specified once".into());
            }
            let value = arguments
                .next()
                .ok_or("--key-hysteresis requires a value from 0 to 1")?;
            let value = value.parse::<f32>().map_err(|_| {
                format!("invalid key hysteresis value: {value}; expected a number from 0 to 1")
            })?;
            scale = Some(HysteresisScale::new(value)?);
        } else {
            filtered.push(argument);
        }
    }
    Ok((filtered, scale.unwrap_or_default()))
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hysteresis_scale_without_changing_positional_arguments() {
        let (args, scale) = parse_hysteresis_argument(vec![
            "aways-just".into(),
            "realtime".into(),
            "MIDI keyboard".into(),
            "--key-hysteresis".into(),
            "0.25".into(),
            "sine".into(),
        ])
        .unwrap();

        assert_eq!(args, ["aways-just", "realtime", "MIDI keyboard", "sine"]);
        assert_eq!(scale.value(), 0.25);
    }

    #[test]
    fn rejects_invalid_or_repeated_hysteresis_options() {
        assert!(parse_hysteresis_argument(vec![
            "aways-just".into(),
            "realtime".into(),
            "--key-hysteresis".into(),
            "2".into(),
        ])
        .is_err());
        assert!(parse_hysteresis_argument(vec![
            "aways-just".into(),
            "realtime".into(),
            "--key-hysteresis".into(),
            "0.5".into(),
            "--key-hysteresis".into(),
            "0.2".into(),
        ])
        .is_err());
    }
}
