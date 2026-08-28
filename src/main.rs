use std::{env, error::Error, process};

use aways_just::{
    diagnostics, midi, realtime,
    synth::{Synthesizer, Waveform},
    wav, SAMPLE_RATE,
};

fn print_diagnostics(path: &str, events: &[midi::MidiEvent], duration: f32) {
    let analysis = diagnostics::analyze(events);
    println!("diagnostics:");
    println!("  input: {path}");
    println!("  duration: {duration:.2}s");
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
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("realtime") if (2..=4).contains(&args.len()) => {
            let waveform = args
                .get(3)
                .map(|value| Waveform::parse(value).ok_or_else(|| format!("unknown waveform: {value}")))
                .transpose()?
                .unwrap_or_default();
            realtime::run(args.get(2).map(String::as_str), waveform)
        }
        Some("render") if args.len() == 4 => {
            let events = midi::read_file(&args[2])?;
            let duration = events.iter().map(|event| event.time).fold(0.0_f32, f32::max) + 1.0;
            print_diagnostics(&args[2], &events, duration);
            let (samples, report) = Synthesizer::render_events_with_report(&events, duration);
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
            wav::write_mono_16(&args[2], &Synthesizer::render_events(&events, duration), SAMPLE_RATE)?;
            println!("rendered {:.2}s demo to {}", duration, args[2]);
            Ok(())
        }
        _ => Err("usage: aways-just demo <output.wav> [seconds]\n       aways-just render <input.mid> <output.wav>\n       aways-just realtime [midi-port-filter] [sine|square|saw|triangle|pwm]".into()),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        process::exit(1);
    }
}
