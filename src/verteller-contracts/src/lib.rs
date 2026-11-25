//! The shared contracts of Verteller: the types, traits and pure code its open app and its Pro
//! features both build on. Nothing here keeps state or opens a socket.
//!
//! - [`entitlements`]: what a build lets the person do ([`Feature`], [`Tier`], [`Edition`],
//!   [`Entitlements`]).
//! - [`Error`] and [`Result`], [`CancelToken`], and the [`ids`] of the things the app stores.
//! - [`Audio`] with WAV reading and writing, and [`read_recording`] for the other formats.
//! - [`split_script`], which cuts a script into pieces a model can read, and [`word_times`].
//! - [`VoiceProfile`] and [`Consent`]: a voice and the record of whose it is, as plain data.
//! - [`Synthesizer`]: the trait a speech model implements.

pub mod audio;
pub mod cancel;
pub mod decode;
pub mod entitlements;
pub mod error;
pub mod ids;
pub mod script;
pub mod voice;
pub mod word_times;

pub use audio::Audio;
pub use cancel::CancelToken;
pub use decode::{READABLE_FORMATS, decode_file, read_recording};
pub use entitlements::{Edition, Entitlements, Feature, FreeEntitlements, Tier};
pub use error::{Error, Result};
pub use script::{MAX_PAUSE_MARKER_MS, MAX_PAUSE_MARKERS_TOTAL_MS, Segment, split_script};
pub use voice::{
    Consent, Engine, MAX_DISPLAY_NAME_CHARS, MAX_TRANSCRIPT_CHARS, MAX_WORD_CHARS, MAX_WORDS,
    VoiceCleanup, VoiceProfile, Word, keep_private, read_profile, write_profile,
};

/// The model a voice has when nothing else was chosen, and the one every voice stored without a
/// model reads as.
///
/// This is the original Chatterbox, which runs through audio.cpp with all its sampling settings.
pub const DEFAULT_MODEL: &str = "qwen3-tts";

/// The shortest reference recording a profile accepts, in seconds.
pub const MIN_REFERENCE_SECONDS: f32 = 3.0;

/// The longest reference recording a profile accepts, in seconds. A file is decoded into memory,
/// and a voice needs far less than this. Five minutes.
pub const MAX_REFERENCE_SECONDS: f32 = 300.0;

/// A speech model that can read text in a stored voice.
pub trait Synthesizer {
    /// The sample rate of the audio it returns.
    fn sample_rate(&self) -> u32;

    /// Reads `text` in `voice`. `text` is one segment from [`split_script`], short enough for the
    /// model to take in one go.
    ///
    /// Check `cancel` between model steps where the backend can, and return [`Error::Cancelled`]
    /// when it is set. A backend that cannot stop mid-segment may ignore it, and the caller checks
    /// again when this returns. When the GPU runs out of memory, return [`Error::OutOfMemory`], for
    /// example through [`Error::from_backend_message`].
    fn synthesize(
        &mut self,
        voice: &VoiceProfile,
        text: &str,
        cancel: &CancelToken,
    ) -> Result<Audio>;

    /// Gets `voice` ready, so the first [`Synthesizer::synthesize`] in it does not pay for it: the
    /// recording is read and the model's own preparation of it is done and kept, in memory only.
    /// Does nothing by default. A prepared voice is kept for as long as the model is loaded (a
    /// backend may keep a limited number), and a recording that changed is prepared again.
    fn prepare(&mut self, _voice: &VoiceProfile, _cancel: &CancelToken) -> Result<()> {
        Ok(())
    }
}
