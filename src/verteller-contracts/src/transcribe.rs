//! Speech to text, as the Pro features use it: plain audio in, words out.
//!
//! The speech models and the forced aligner are the app's. This is the trait it implements for them
//! ([`Transcribe`]) and the plain data it answers with.

use crate::{ApiError, Audio, MAX_REFERENCE_SECONDS};
use async_trait::async_trait;
use axum::http::StatusCode;

/// The rate the word times of a [`Transcript`] count samples at, whatever rate the audio had: the
/// rate the speech to text model and the aligner are fed.
pub const WORD_TIME_RATE: u32 = 16_000;

/// The longest recording the aligner takes in one request, in seconds. The library does not cut long
/// audio for the aligner (one request is one stretch of audio with the text said in it), so the
/// limit is the one of a recording elsewhere in the app and a longer voiceover is aligned part by
/// part. Not measured against the real aligner.
pub const MAX_ALIGN_SECONDS: f32 = MAX_REFERENCE_SECONDS;

/// What speech recognition made of a recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    /// What is said, trimmed.
    pub text: String,
    /// The language the model says it is in, when it says.
    pub language: Option<String>,
    /// Each word and where it is, when word times were asked for. Empty otherwise, and empty for
    /// a recording with no speech.
    pub words: Vec<RecognisedWord>,
}

/// One word the model wrote down, with where it sits in the audio the request was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecognisedWord {
    /// The word as the model wrote it.
    pub text: String,
    /// The first sample of the word, in the audio the request was given (at [`WORD_TIME_RATE`]).
    pub start_sample: i64,
    /// One past the last sample of the word.
    pub end_sample: i64,
}

/// Why a recording was not written down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscribeError {
    /// No speech to text model is downloaded, or no audio.cpp library build can be used.
    NotReady,
    /// Word times were asked for and the forced aligner is not downloaded.
    NoAligner,
    /// The model could not be loaded or run. The text is the sentence for the person.
    Failed(String),
}

impl From<TranscribeError> for ApiError {
    fn from(e: TranscribeError) -> Self {
        match e {
            TranscribeError::NotReady => ApiError::new(
                StatusCode::CONFLICT,
                "asr_model_not_ready",
                "Speech to text needs a model. Download one on the Speech model page, then try again.",
            ),
            TranscribeError::NoAligner => ApiError::new(
                StatusCode::CONFLICT,
                "aligner_model_not_ready",
                "Word times need the forced aligner model. Download it on the Speech model page, then try again.",
            ),
            TranscribeError::Failed(why) => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "transcription_failed",
                why,
            ),
        }
    }
}

/// Writes down what a recording says.
#[async_trait]
pub trait Transcribe: Send + Sync + 'static {
    /// Whether a request can run now: a speech to text model is downloaded and a library build can
    /// be used.
    fn ready(&self) -> bool;

    /// Writes down what `audio` says. `language` is a hint such as `English`; `None` lets the
    /// model decide. A recording with no speech in it gives an empty text.
    async fn transcribe(
        &self,
        audio: Audio,
        language: Option<String>,
    ) -> Result<Transcript, TranscribeError>;

    /// Whether word times can be made now: [`Transcribe::ready`] and the forced aligner is
    /// downloaded.
    fn words_ready(&self) -> bool {
        false
    }

    /// [`Transcribe::transcribe`] with the time of each word in the answer, in samples at
    /// [`WORD_TIME_RATE`] whatever rate `audio` has. [`TranscribeError::NoAligner`] when the forced
    /// aligner is not downloaded.
    async fn transcribe_words(
        &self,
        _audio: Audio,
        _language: Option<String>,
    ) -> Result<Transcript, TranscribeError> {
        Err(TranscribeError::NoAligner)
    }

    /// Whether a known text can be timed now: the forced aligner is downloaded and a library build
    /// can be used. The speech to text model is not needed.
    fn align_ready(&self) -> bool {
        false
    }

    /// Times each word of `text`, which is known, in `audio`, with the forced aligner alone.
    /// `language` is the language of the text, such as `English`. The answer's `text` is the text
    /// given and its words carry the times, in samples at [`WORD_TIME_RATE`] whatever rate `audio`
    /// has. [`TranscribeError::NoAligner`] when the forced aligner is not downloaded.
    async fn align(
        &self,
        _audio: Audio,
        _text: String,
        _language: String,
    ) -> Result<Transcript, TranscribeError> {
        Err(TranscribeError::NoAligner)
    }

    /// Frees the model now, if one is kept loaded. The app calls it when keep-warm is turned off and
    /// when the device changes, so no graphics memory is held. A service that keeps nothing does
    /// nothing, and a Pro feature never calls it.
    fn release(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_model_is_a_409_that_names_what_to_download() {
        let e = ApiError::from(TranscribeError::NotReady);
        assert_eq!(
            (e.status, e.code),
            (StatusCode::CONFLICT, "asr_model_not_ready")
        );
        let e = ApiError::from(TranscribeError::NoAligner);
        assert_eq!(
            (e.status, e.code),
            (StatusCode::CONFLICT, "aligner_model_not_ready")
        );
        let e = ApiError::from(TranscribeError::Failed("boom".into()));
        assert_eq!(
            (e.status, e.code, e.detail.as_str()),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "transcription_failed",
                "boom"
            )
        );
    }
}
