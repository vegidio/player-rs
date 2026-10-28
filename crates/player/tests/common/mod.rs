//! Helpers shared by the integration tests.

use std::path::{Path, PathBuf};

/// Writes a mono, 16-bit, 8 kHz PCM WAV of `seconds` seconds of a 440 Hz tone to `dir/name` and returns its path.
///
/// The 44-byte header is written by hand, so no audio crate is needed. Unlike mpv's generated `lavfi` sources, the
/// file has a known duration and seeks backwards.
pub fn write_wav(dir: &Path, name: &str, seconds: u32) -> PathBuf {
    const RATE: u32 = 8000;
    let samples = RATE * seconds;
    let data_len = samples * 2;

    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // channels
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for n in 0..samples {
        let phase = 2.0 * std::f64::consts::PI * 440.0 * f64::from(n) / f64::from(RATE);
        wav.extend_from_slice(&((phase.sin() * 8000.0) as i16).to_le_bytes());
    }

    let path = dir.join(name);
    std::fs::write(&path, wav).expect("the WAV file is written");
    path
}
