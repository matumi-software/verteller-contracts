//! The wire types of an audition: a short reading of a few parts, to hear the pacing while a script
//! is written.
//!
//! An audition is a small, short-lived job the app keeps in memory only. The rules, with the
//! reasons, are in the app's `docs/architecture.md` ("Auditions"). The queue that reads it and the
//! limits it keeps are the app's.

use crate::{VoiceProfile, jobs::JobError};
use serde::{Deserialize, Serialize};

/// `Audition.state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum AuditionState {
    /// Waiting its turn in the queue.
    Queued,
    /// The model is reading it.
    Reading,
    /// Finished, with audio to play.
    Ready,
    /// Ended with an error.
    Failed,
}

/// `PauseAfter`: the pause after the last part of an audition that a voiceover would have.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum PauseAfter {
    /// None: the audio ends where the last part ends.
    #[default]
    None,
    /// The sentence pause, as after a part inside a paragraph.
    Sentence,
    /// The paragraph pause, as after a part that ends a paragraph.
    Paragraph,
}

/// A short reading to hear, as the routes and the events carry it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Audition {
    /// A UUID.
    pub id: String,
    /// The voice's name.
    pub voice: String,
    /// Where it is.
    pub state: AuditionState,
    /// RFC 3339.
    pub created_at: String,
    /// How many parts the text was cut into.
    pub parts: usize,
    /// Length of the audio, set when it is ready.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub seconds: Option<f32>,
    /// RFC 3339, set when it is ready or failed: when the app forgets it. After that it answers
    /// `audition_not_found`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub expires_at: Option<String>,
    /// Set when it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub error: Option<JobError>,
    /// The id of the member who asked. Left out for the owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub owner: Option<String>,
}

/// What `POST /api/v1/auditions` hands the queue. The route has already found the voice and checked
/// that the text has words.
#[derive(Debug, Clone)]
pub struct NewAudition {
    /// The stored voice to read in.
    pub voice: VoiceProfile,
    /// The text of the chosen parts, with their pause markers.
    pub text: String,
    /// The pause inside a paragraph as the caller wrote it, `None` for the default.
    pub sentence_pause_ms: Option<i64>,
    /// The pause after a paragraph as the caller wrote it, `None` for the default.
    pub paragraph_pause_ms: Option<i64>,
    /// The pause to add after the last part.
    pub pause_after: PauseAfter,
    /// Whose it is: `None` for the owner, the member's id otherwise.
    pub owner: Option<String>,
}
