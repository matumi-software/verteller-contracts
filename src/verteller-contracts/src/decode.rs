//! Reading a recording that is not a WAV: MP3, M4A (AAC), OGG (Vorbis) and FLAC, through
//! `symphonia`. The result is mono, at the file's own sample rate, like a WAV read by
//! [`Audio::read_wav`].

use crate::{Audio, Error, Result};
use std::path::Path;
use symphonia::core::{
    codecs::audio::AudioDecoderOptions,
    errors::Error as SymphoniaError,
    formats::{FormatOptions, TrackType, probe::Hint},
    io::MediaSourceStream,
    meta::MetadataOptions,
};

/// What the person is told when a file cannot be read. It names the formats this build decodes.
pub const READABLE_FORMATS: &str = "WAV, MP3, M4A (AAC), OGG (Vorbis) and FLAC";

fn unreadable(why: impl std::fmt::Display) -> Error {
    Error::Decode(format!(
        "That file is not audio verteller can read ({why}). Use {READABLE_FORMATS}."
    ))
}

/// Reads a recording of any format [`READABLE_FORMATS`] names, as mono audio at its own sample
/// rate: a WAV as it is, anything else decoded. A file longer than `max_seconds` is
/// [`Error::ReferenceTooLong`].
pub fn read_recording(path: &Path, max_seconds: f32) -> Result<Audio> {
    if hound::WavReader::open(path).is_ok() {
        let audio = Audio::read_wav(path)?;
        if audio.seconds() > max_seconds {
            return Err(Error::ReferenceTooLong { max: max_seconds });
        }
        return Ok(audio);
    }
    decode_file(path, max_seconds)
}

/// Decodes `path` to mono. Several channels are averaged. Stops with
/// [`Error::ReferenceTooLong`] once the audio passes `max_seconds`, so a huge file cannot fill the
/// memory.
pub fn decode_file(path: &Path, max_seconds: f32) -> Result<Audio> {
    let file = std::fs::File::open(path).map_err(|_| unreadable("no file"))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|_| unreadable("unknown format"))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| unreadable("no audio in it"))?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| unreadable("no audio in it"))?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|_| unreadable("that audio codec is not supported"))?;

    let mut mono: Vec<f32> = Vec::new();
    let mut rate = 0_u32;
    let mut scratch: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            // Some streams end with an IO error instead of a clean end.
            Err(SymphoniaError::IoError(_)) => break,
            Err(e) => return Err(unreadable(e)),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buffer = match decoder.decode(&packet) {
            Ok(buffer) => buffer,
            // One bad packet is skipped, as players do.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(unreadable(e)),
        };
        let channels = buffer.spec().channels().count().max(1);
        rate = buffer.spec().rate();
        scratch.resize(buffer.samples_interleaved(), 0.0);
        buffer.copy_to_slice_interleaved(&mut scratch);
        mono.extend(
            scratch
                .chunks_exact(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );
        if rate > 0 && mono.len() as f32 / rate as f32 > max_seconds {
            return Err(Error::ReferenceTooLong { max: max_seconds });
        }
    }
    if rate == 0 || mono.is_empty() {
        return Err(unreadable("no audio in it"));
    }
    Ok(Audio {
        samples: mono,
        sample_rate: rate,
    })
}
