//! Making music, as the Pro features use it: a written prompt (and optionally a style clip) in, a
//! piece of stereo audio out.
//!
//! The music model is the app's. This is the trait it implements for it ([`MakeMusic`]) and the
//! plain data it takes and answers with, as [`transcribe`](crate::transcribe) is for speech to text.
//! Playlist is the one feature that uses it.

use crate::{ApiError, Audio, CancelToken};
use async_trait::async_trait;
use axum::http::StatusCode;
use std::collections::BTreeMap;

/// The longest piece one request makes, in seconds. A longer track is made of several requests.
pub const MAX_MUSIC_SECONDS: f32 = 120.0;

/// The shortest piece one request makes, in seconds.
pub const MIN_MUSIC_SECONDS: f32 = 5.0;

/// One piece of music to make.
#[derive(Debug, Clone, PartialEq)]
pub struct MusicRequest {
    /// What the music should be like, in words. May be empty when `style` is given.
    pub prompt: String,
    /// A clip to take the style from. It transfers style; it does not continue the clip.
    pub style: Option<Audio>,
    /// How long the piece is, in seconds, from [`MIN_MUSIC_SECONDS`] to [`MAX_MUSIC_SECONDS`].
    pub seconds: f32,
    /// The seed. The same prompt, style, settings and seed make the same piece.
    pub seed: u64,
    /// The model's own settings, by key, as the model's schema keys them: what was changed from the
    /// defaults. The app turns them into the engine's request options.
    pub settings: BTreeMap<String, f64>,
    /// The music model, by id. `None` is the one the person chose (the `defaultMusic` setting).
    pub model: Option<String>,
}

/// Music a model made: interleaved samples, with the rate and the channel count the model makes
/// them at (Magenta RT 2: 48 kHz, stereo).
#[derive(Debug, Clone, PartialEq)]
pub struct Music {
    /// The samples, `channels` of them for each frame.
    pub samples: Vec<f32>,
    /// Frames per second.
    pub sample_rate: u32,
    /// How many channels each frame has.
    pub channels: u16,
}

impl Music {
    /// How long it plays, in seconds.
    pub fn seconds(&self) -> f32 {
        let per_frame = usize::from(self.channels.max(1));
        (self.samples.len() / per_frame) as f32 / self.sample_rate.max(1) as f32
    }
}

/// What a feature may know of the music model a request would run on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicModelInfo {
    /// The model's id.
    pub id: String,
    /// The name to show.
    pub name: String,
    /// Whether a request may carry a style clip.
    pub style_clip: bool,
    /// Whether the model embeds a clip's style and does not continue it or treat it as audio to
    /// edit. Only then does a clip of one piece keep the next piece in the same instrument family.
    pub clip_sets_style: bool,
}

/// Why no music was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MusicError {
    /// No music model is downloaded (or the one asked for is not), or no audio.cpp library build can
    /// be used.
    NotReady,
    /// The request was cancelled before the model started.
    Cancelled,
    /// The request is not one the model can make. The text is the sentence for the person.
    Invalid(String),
    /// The model could not be loaded or run. The text is the sentence for the person.
    Failed(String),
}

impl From<MusicError> for ApiError {
    fn from(e: MusicError) -> Self {
        match e {
            MusicError::NotReady => ApiError::new(
                StatusCode::CONFLICT,
                "music_model_not_ready",
                "Making music needs a music model. Download one on the Speech model page, then try again.",
            ),
            MusicError::Cancelled => ApiError::new(
                StatusCode::CONFLICT,
                "music_cancelled",
                "The track was cancelled.",
            ),
            MusicError::Invalid(why) => ApiError::invalid_request(why),
            MusicError::Failed(why) => {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "music_failed", why)
            }
        }
    }
}

/// Makes music.
#[async_trait]
pub trait MakeMusic: Send + Sync + 'static {
    /// Whether a request can run now: a music model is downloaded and a library build can be used.
    fn ready(&self) -> bool;

    /// Makes the piece `request` asks for. One request runs at a time. `cancel` is looked at before
    /// the model starts: the library has no way to stop a piece that is being made, so a cancel
    /// while it runs takes effect when it returns. The model is loaded for the request and freed
    /// after it.
    async fn make(&self, request: MusicRequest, cancel: CancelToken) -> Result<Music, MusicError>;

    /// The music model a request that names `model` (`None`: the one the person chose) would run
    /// on, or `None` when none can be told. A feature keeps what it learns of a model by its id,
    /// and `None` for the id it was given is a model it cannot tell apart from others.
    fn model_info(&self, _model: Option<&str>) -> Option<MusicModelInfo> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_piece_is_as_long_as_its_frames_say_not_its_samples() {
        let m = Music {
            samples: vec![0.0; 192_000],
            sample_rate: 48_000,
            channels: 2,
        };
        assert!((m.seconds() - 2.0).abs() < 1e-6);
    }

    struct Silent;

    #[async_trait]
    impl MakeMusic for Silent {
        fn ready(&self) -> bool {
            false
        }

        async fn make(&self, _: MusicRequest, _: CancelToken) -> Result<Music, MusicError> {
            Err(MusicError::NotReady)
        }
    }

    #[test]
    fn a_service_that_does_not_say_which_model_it_runs_gives_none() {
        assert_eq!(Silent.model_info(None), None);
        assert_eq!(Silent.model_info(Some("any")), None);
    }

    #[test]
    fn each_failure_is_an_answer_a_person_can_act_on() {
        let e = ApiError::from(MusicError::NotReady);
        assert_eq!(
            (e.status, e.code),
            (StatusCode::CONFLICT, "music_model_not_ready")
        );
        let e = ApiError::from(MusicError::Cancelled);
        assert_eq!(
            (e.status, e.code),
            (StatusCode::CONFLICT, "music_cancelled")
        );
        let e = ApiError::from(MusicError::Invalid("too long".into()));
        assert_eq!(
            (e.status, e.code),
            (StatusCode::BAD_REQUEST, "invalid_request")
        );
        let e = ApiError::from(MusicError::Failed("boom".into()));
        assert_eq!(
            (e.status, e.code, e.detail.as_str()),
            (StatusCode::INTERNAL_SERVER_ERROR, "music_failed", "boom")
        );
    }
}
