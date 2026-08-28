use std::{
    error::Error,
    io,
    sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
    thread,
    time::Duration,
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    FromSample, Sample, SampleFormat, SizedSample, StreamConfig,
};
use midir::{Ignore, MidiInput};

use crate::{
    midi::{MidiEvent, MidiKind},
    synth::{Synthesizer, Waveform},
};

const LOW_LATENCY_BUFFER_FRAMES: u32 = 256;

pub fn run(port_filter: Option<&str>, waveform: Waveform) -> Result<(), Box<dyn Error>> {
    let mut midi = MidiInput::new("aways-just")?;
    midi.ignore(Ignore::None);
    let ports = midi.ports();
    let port = ports
        .iter()
        .find(|port| {
            let Ok(name) = midi.port_name(port) else {
                return false;
            };
            port_filter.is_none_or(|filter| name.contains(filter))
        })
        .cloned()
        .ok_or_else(|| {
            let available = ports
                .iter()
                .filter_map(|port| midi.port_name(port).ok())
                .collect::<Vec<_>>()
                .join(", ");
            format!("no MIDI input matched; available ports: {available}")
        })?;
    let port_name = midi.port_name(&port)?;

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("no default audio output device found")?;
    let supported = device.default_output_config()?;
    let sample_rate = supported.sample_rate().0;
    let (midi_sender, midi_receiver) = sync_channel(256);
    let (diagnostic_sender, diagnostic_receiver) = sync_channel(256);
    let _midi_connection = midi.connect(
        &port,
        "aways-just-midi",
        move |timestamp, bytes, _| {
            if let Some(event) = parse_message(timestamp, bytes) {
                if let Err(error) = midi_sender.try_send(event) {
                    if !matches!(error, TrySendError::Disconnected(_)) {
                        eprintln!("MIDI event queue full; dropping input event");
                    }
                }
            }
        },
        (),
    )?;

    let config = StreamConfig {
        channels: supported.channels(),
        sample_rate: cpal::SampleRate(sample_rate),
        buffer_size: cpal::BufferSize::Fixed(LOW_LATENCY_BUFFER_FRAMES),
    };
    let channels = usize::from(config.channels);
    let stream = build_stream_for_format(
        &device,
        &config,
        channels,
        midi_receiver,
        diagnostic_sender,
        waveform,
        supported.sample_format(),
    )?;
    stream.play()?;

    println!("MIDI input: {port_name}");
    println!("waveform: {}", waveform_name(waveform));
    println!(
        "audio output: {} ({} channels at {} Hz)",
        device.name().unwrap_or_else(|_| "unknown".into()),
        config.channels,
        config.sample_rate.0
    );
    if config.buffer_size == cpal::BufferSize::Fixed(LOW_LATENCY_BUFFER_FRAMES) {
        println!(
            "audio buffer: {LOW_LATENCY_BUFFER_FRAMES} frames ({:.1} ms)",
            1000.0 * f64::from(LOW_LATENCY_BUFFER_FRAMES) / f64::from(sample_rate)
        );
    } else {
        println!("audio buffer: device default (low-latency request was not accepted)");
    }
    println!("playing in real time; press Enter to stop");
    let diagnostics_thread = thread::spawn(move || {
        while let Ok(diagnostic) = diagnostic_receiver.recv() {
            print_diagnostic(&diagnostic);
        }
    });
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    drop(stream);
    let _ = diagnostics_thread.join();
    Ok(())
}

fn print_diagnostic(diagnostic: &crate::synth::TuningDiagnostic) {
    let key = diagnostic
        .key
        .map_or_else(|| "unknown".to_owned(), |key| key.to_string());
    println!("MIDI update: key={key}, voices={}", diagnostic.notes.len());
    for note in &diagnostic.notes {
        println!(
            "  ch {:>2} note {:>3}: velocity {:>3.0}% -> {:>8.2} Hz ({:+.2} cents)",
            note.channel + 1,
            note.note,
            note.velocity * 100.0,
            note.frequency,
            note.cents_offset
        );
    }
}

fn parse_message(timestamp: u64, bytes: &[u8]) -> Option<MidiEvent> {
    let status = *bytes.first()?;
    let channel = status & 0x0f;
    let note = *bytes.get(1)?;
    let velocity = *bytes.get(2).unwrap_or(&0);
    let kind = match status & 0xf0 {
        0x90 if velocity != 0 => MidiKind::NoteOn { note, velocity },
        0x80 | 0x90 => MidiKind::NoteOff { note },
        _ => return None,
    };
    Some(MidiEvent {
        time: timestamp as f32 / 1_000_000.0,
        channel,
        kind,
    })
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    channels: usize,
    midi_receiver: Receiver<MidiEvent>,
    diagnostic_sender: SyncSender<crate::synth::TuningDiagnostic>,
    waveform: Waveform,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: Sample + SizedSample + FromSample<f32>,
{
    let mut synth = Synthesizer::with_waveform(config.sample_rate.0, waveform);
    device.build_output_stream(
        config,
        move |output: &mut [T], _| {
            let frames = output.len() / channels;
            for event in midi_receiver.try_iter() {
                let diagnostic = synth.handle_midi_event(event);
                let _ = diagnostic_sender.try_send(diagnostic);
            }
            for frame in 0..frames {
                let sample = T::from_sample(synth.render_sample());
                for channel in 0..channels {
                    output[frame * channels + channel] = sample;
                }
            }
        },
        |error| eprintln!("audio stream error: {error}"),
        Some(Duration::from_millis(100)),
    )
}

fn build_stream_for_format(
    device: &cpal::Device,
    config: &StreamConfig,
    channels: usize,
    midi_receiver: Receiver<MidiEvent>,
    diagnostic_sender: SyncSender<crate::synth::TuningDiagnostic>,
    waveform: Waveform,
    format: SampleFormat,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    match format {
        SampleFormat::F32 => build_stream::<f32>(
            device,
            config,
            channels,
            midi_receiver,
            diagnostic_sender,
            waveform,
        ),
        SampleFormat::I16 => build_stream::<i16>(
            device,
            config,
            channels,
            midi_receiver,
            diagnostic_sender,
            waveform,
        ),
        SampleFormat::U16 => build_stream::<u16>(
            device,
            config,
            channels,
            midi_receiver,
            diagnostic_sender,
            waveform,
        ),
        _ => Err(cpal::BuildStreamError::StreamConfigNotSupported),
    }
}

fn waveform_name(waveform: Waveform) -> &'static str {
    match waveform {
        Waveform::Sine => "sine",
        Waveform::Square => "square",
        Waveform::Saw => "saw",
        Waveform::Triangle => "triangle",
        Waveform::Pwm => "pwm",
    }
}
