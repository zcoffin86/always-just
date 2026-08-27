use std::{error::Error, fmt, fs, path::Path};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MidiKind {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MidiEvent {
    pub time: f32,
    pub channel: u8,
    pub kind: MidiKind,
}

#[derive(Debug)]
pub struct MidiError(String);

impl fmt::Display for MidiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for MidiError {}

fn read_u16(data: &[u8], pos: &mut usize) -> Result<u16, MidiError> {
    if *pos + 2 > data.len() {
        return Err(MidiError("unexpected end of MIDI data".into()));
    }
    let value = u16::from_be_bytes([data[*pos], data[*pos + 1]]);
    *pos += 2;
    Ok(value)
}

fn read_u32(data: &[u8], pos: &mut usize) -> Result<u32, MidiError> {
    if *pos + 4 > data.len() {
        return Err(MidiError("unexpected end of MIDI data".into()));
    }
    let value = u32::from_be_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]);
    *pos += 4;
    Ok(value)
}

fn read_vlq(data: &[u8], pos: &mut usize) -> Result<u32, MidiError> {
    let mut value = 0;
    for _ in 0..4 {
        let byte = *data
            .get(*pos)
            .ok_or_else(|| MidiError("invalid MIDI variable-length value".into()))?;
        *pos += 1;
        value = (value << 7) | u32::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(MidiError("MIDI variable-length value is too long".into()))
}

pub fn read_file(path: impl AsRef<Path>) -> Result<Vec<MidiEvent>, Box<dyn Error>> {
    let data = fs::read(path)?;
    parse(&data).map_err(|error| Box::new(error) as Box<dyn Error>)
}

pub fn parse(data: &[u8]) -> Result<Vec<MidiEvent>, MidiError> {
    let mut pos = 0;
    if data.get(0..4) != Some(b"MThd") {
        return Err(MidiError("missing MIDI header".into()));
    }
    pos += 4;
    let header_len = read_u32(data, &mut pos)?;
    if header_len != 6 {
        return Err(MidiError("unsupported MIDI header length".into()));
    }
    let format = read_u16(data, &mut pos)?;
    let tracks = read_u16(data, &mut pos)?;
    let division = read_u16(data, &mut pos)?;
    if format == 2 || division & 0x8000 != 0 || tracks == 0 {
        return Err(MidiError(
            "MIDI format or time division is unsupported".into(),
        ));
    }
    let ticks_per_quarter = u32::from(division);
    let mut raw: Vec<(u64, u8, RawKind)> = Vec::new();
    for track_index in 0..tracks {
        if data.get(pos..pos + 4) != Some(b"MTrk") {
            return Err(MidiError(format!(
                "missing MIDI track header {track_index}"
            )));
        }
        pos += 4;
        let length = read_u32(data, &mut pos)? as usize;
        let end = pos
            .checked_add(length)
            .ok_or_else(|| MidiError("track length overflow".into()))?;
        if end > data.len() {
            return Err(MidiError("MIDI track extends past file".into()));
        }
        let mut tick = 0_u64;
        let mut running_status = None;
        while pos < end {
            tick += u64::from(read_vlq(data, &mut pos)?);
            let mut status = *data
                .get(pos)
                .ok_or_else(|| MidiError("missing MIDI event status".into()))?;
            if status & 0x80 != 0 {
                pos += 1;
                if status < 0xf0 {
                    running_status = Some(status);
                }
            } else {
                status = running_status
                    .ok_or_else(|| MidiError("MIDI event has no running status".into()))?;
            }
            match status {
                0x80..=0x9f => {
                    let note = *data
                        .get(pos)
                        .ok_or_else(|| MidiError("truncated note event".into()))?;
                    let velocity = *data
                        .get(pos + 1)
                        .ok_or_else(|| MidiError("truncated note event".into()))?;
                    pos += 2;
                    let channel = status & 0x0f;
                    let kind = if status & 0xf0 == 0x90 && velocity != 0 {
                        MidiKind::NoteOn { note, velocity }
                    } else {
                        MidiKind::NoteOff { note }
                    };
                    raw.push((tick, channel, RawKind::Event(kind)));
                }
                0xa0..=0xef => {
                    let length = if status & 0xe0 == 0xc0 { 1 } else { 2 };
                    pos += length;
                    if pos > end {
                        return Err(MidiError("truncated MIDI channel event".into()));
                    }
                }
                0xff => {
                    let meta = *data
                        .get(pos)
                        .ok_or_else(|| MidiError("truncated MIDI meta event".into()))?;
                    pos += 1;
                    let length = read_vlq(data, &mut pos)? as usize;
                    let meta_end = pos
                        .checked_add(length)
                        .ok_or_else(|| MidiError("meta event length overflow".into()))?;
                    if meta_end > end {
                        return Err(MidiError("truncated MIDI meta event".into()));
                    }
                    if meta == 0x51 && length == 3 {
                        let micros = (u32::from(data[pos]) << 16)
                            | (u32::from(data[pos + 1]) << 8)
                            | u32::from(data[pos + 2]);
                        raw.push((tick, 16, RawKind::Tempo(micros)));
                    }
                    pos = meta_end;
                    if meta == 0x2f {
                        pos = end;
                    }
                }
                0xf0 | 0xf7 => {
                    let length = read_vlq(data, &mut pos)? as usize;
                    pos = pos
                        .checked_add(length)
                        .ok_or_else(|| MidiError("system event length overflow".into()))?;
                    if pos > end {
                        return Err(MidiError("truncated MIDI system event".into()));
                    }
                }
                _ => return Err(MidiError("invalid MIDI status byte".into())),
            }
        }
        pos = end;
    }
    raw.sort_by_key(|event| event.0);
    let mut result = Vec::new();
    let mut last_tick = 0_u64;
    let mut seconds = 0.0_f32;
    let mut tempo = 500_000_u32;
    for (tick, channel, kind) in raw {
        seconds +=
            (tick - last_tick) as f32 * tempo as f32 / 1_000_000.0 / ticks_per_quarter as f32;
        last_tick = tick;
        match kind {
            RawKind::Tempo(micros) => tempo = micros.max(1),
            RawKind::Event(kind) => result.push(MidiEvent {
                time: seconds,
                channel,
                kind,
            }),
        }
    }
    Ok(result)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum RawKind {
    Event(MidiKind),
    Tempo(u32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_note_events_and_converts_ticks_to_seconds() {
        let bytes = [
            b'M', b'T', b'h', b'd', 0, 0, 0, 6, 0, 0, 0, 1, 0, 96, b'M', b'T', b'r', b'k', 0, 0, 0,
            12, 0, 0x90, 60, 100, 96, 0x80, 60, 0, 0, 0xff, 0x2f, 0,
        ];
        let events = parse(&bytes).expect("valid MIDI");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].time, 0.0);
        assert!((events[1].time - 0.5).abs() < 0.001);
    }
}
