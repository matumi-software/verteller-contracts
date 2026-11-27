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
//! - [`jobs`]: [`Job`] and the types around it, and the [`JobService`]
//!   trait the app's job queue implements. [`auditions`] has the short readings that go with it.
//! - [`api::ApiError`], the problem JSON every refusal is, and [`Caller`], who is asking.
//! - [`claude_desktop`]: the wire types of the Claude Desktop connection.
//! - [`bridge`]: starting the app, and the address it is found at.
//! - [`host`]: the [`Host`] trait, everything the app gives a Pro feature. [`events`] is the live
//!   push ([`EventSink`]), [`transcribe`] speech to text ([`Transcribe`]) and [`models`] what a
//!   feature may know of a speech model.
//! - [`music`]: making music ([`MakeMusic`]), which Playlist uses.
//! - [`extensions`]: the hooks a Pro build plugs in ([`Extensions`]) and what a build gives the
//!   app ([`Addons`]). A route hook gets an `Arc<dyn Host>` and answers a `Router<()>`.
//! - `testing` (the `testing` feature): a `testing::FakeHost`, so a Pro feature's unit
//!   tests do not need the app.

pub mod api;
pub mod audio;
pub mod auditions;
pub mod bridge;
pub mod caller;
pub mod cancel;
pub mod claude_desktop;
pub mod decode;
pub mod entitlements;
pub mod error;
pub mod events;
pub mod extensions;
pub mod host;
pub mod ids;
pub mod jobs;
pub mod models;
pub mod music;
pub mod script;
#[cfg(feature = "testing")]
pub mod testing;
pub mod transcribe;
pub mod voice;
pub mod word_times;

pub use api::{ApiError, ApiJson, ApiMultipart, ApiQuery, ApiResult};
pub use audio::Audio;
pub use caller::Caller;
pub use cancel::CancelToken;
pub use decode::{READABLE_FORMATS, decode_file, read_recording};
pub use entitlements::{Edition, Entitlements, Feature, FreeEntitlements, Tier};
pub use error::{Error, MAX_ECHO_CHARS, Result, echo};
pub use events::{EventSink, EventSinkExt};
pub use extensions::{Addons, Extensions};
pub use host::{Host, VoiceImport, WriteGuard};
pub use jobs::{Job, JobService, JobServiceError, now_rfc3339, safe_file_stem};
pub use models::ModelInfo;
pub use music::{MakeMusic, Music, MusicError, MusicModelInfo, MusicRequest};
pub use script::{MAX_PAUSE_MARKER_MS, MAX_PAUSE_MARKERS_TOTAL_MS, Segment, split_script};
pub use transcribe::{Transcribe, TranscribeError};
pub use voice::{
    Consent, Engine, MAX_DISPLAY_NAME_CHARS, MAX_TRANSCRIPT_CHARS, MAX_WORD_CHARS, MAX_WORDS,
    SampleSource, VoiceCleanup, VoiceProfile, Word, keep_private, read_profile, write_profile,
};

/// The model a voice has when nothing else was chosen, and the one every voice stored without a
/// model reads as.
///
/// This is the original Chatterbox, which runs through audio.cpp with all its sampling settings.
pub const DEFAULT_MODEL: &str = "qwen3-tts";

/// The pause after a segment inside a paragraph when nothing says otherwise, in milliseconds.
pub const DEFAULT_SENTENCE_PAUSE_MS: u32 = 250;

/// The pause after a segment that ends a paragraph when nothing says otherwise, in milliseconds.
pub const DEFAULT_PARAGRAPH_PAUSE_MS: u32 = 600;

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
