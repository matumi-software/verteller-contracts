//! [`Host`]: what the app gives a Pro feature, and nothing more.
//!
//! A Pro route hook gets an `Arc<dyn Host>` when the app builds its routes (see
//! [`extensions`](crate::extensions)), builds a router over it and keeps it for its handlers. The
//! app implements it over its own state. Every method is something a Pro feature calls today, and a
//! new one is added only when a feature needs it, so the trait is the whole of what Pro can reach.
//!
//! Required methods are what the app answers. The provided ones are the rules that are built on
//! them, so a stand-in (`testing::FakeHost`) gets them for free.

use crate::{
    ApiError, Caller, Consent, Edition, Feature, VoiceCleanup, VoiceProfile, Word,
    events::EventSink, jobs::JobService, models, models::ModelInfo, music::MakeMusic,
    transcribe::Transcribe,
};
use async_trait::async_trait;
use std::{any::Any, collections::BTreeMap, path::PathBuf, sync::Arc};

/// The right to write new data of a person, held until the data is written. See
/// [`Host::writing_for`]. Dropping it lets a removal of that person go on.
pub struct WriteGuard(#[allow(dead_code)] Box<dyn Any + Send + Sync>);

impl WriteGuard {
    /// A guard that holds `held` until it is dropped.
    pub fn new<G: Send + Sync + 'static>(held: G) -> Self {
        Self(Box::new(held))
    }
}

impl std::fmt::Debug for WriteGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WriteGuard")
    }
}

/// A voice to add, as plain data: what a package carries of one. The id, the owner and who made
/// it are not here: the app gives the voice a new id and the person it is added for owns it. The
/// consent record is kept exactly as it is.
#[derive(Debug, Clone)]
pub struct VoiceImport {
    /// The name to show. The app makes it free, as a duplicate is numbered (`name 2`).
    pub display_name: Option<String>,
    /// The speech model, by id.
    pub model: String,
    /// What was changed from the model's defaults.
    pub settings: BTreeMap<String, f64>,
    /// How much of the recording the model uses.
    pub sample_seconds: Option<f32>,
    /// What the recording says.
    pub transcript: Option<String>,
    /// Words the model is given written another way.
    pub words: Vec<Word>,
    /// The clean-up, when the voice has a cleaned copy (`cleaned` is then set).
    pub cleanup: Option<VoiceCleanup>,
    /// The speaker, the statement and when it was recorded, exactly as they were.
    pub consent: Consent,
    /// The reference recording, a WAV file.
    pub reference: PathBuf,
    /// The cleaned copy, a WAV file, when there is one.
    pub cleaned: Option<PathBuf>,
}

/// What the app gives a Pro feature. Implemented by the app over its state, and by
/// `testing::FakeHost` for a feature's own tests.
#[async_trait]
pub trait Host: Send + Sync + 'static {
    /// What this build lets the person do.
    fn edition(&self) -> Edition;

    /// Lets the write of new data of `caller` start: waits while a removal of people runs, then
    /// checks that they are still a person (`401 unauthorized` when they are not). Their session
    /// was checked when the request came in, which can be long before (an upload), so a member
    /// removed since writes nothing. Hold the guard until the data is written. The owner is never
    /// removed.
    async fn writing_for(&self, caller: &Caller) -> Result<WriteGuard, ApiError>;

    /// The job queue.
    fn jobs(&self) -> Arc<dyn JobService>;

    /// The live push.
    fn events(&self) -> Arc<dyn EventSink>;

    /// Speech to text and the forced aligner. Plain audio in, words out.
    fn transcriber(&self) -> Arc<dyn Transcribe>;

    /// Making music. A prompt (and a style clip) in, stereo audio out.
    fn music(&self) -> Arc<dyn MakeMusic>;

    /// The folder Playlist keeps its tracks and styles in, one sub-folder for each person. The app
    /// makes it and removes a person's folder with the person.
    fn playlist_folder(&self) -> PathBuf;

    /// The app's scratch folder, for the files of a request in flight. It is made when the app
    /// starts and cleaned at start.
    fn tmp_dir(&self) -> PathBuf;

    /// The folder the person chose to save voiceovers in (the `outputFolder` setting).
    fn output_folder(&self) -> PathBuf;

    /// The `Host` header values this listener answers to, lowercase.
    fn allowed_hosts(&self) -> Vec<String>;

    /// The owner's user name, which is who made what the owner made. Empty when there is none.
    async fn owner_user_name(&self) -> String;

    /// The speech model `id`, or `None` when the app does not know it.
    fn model_info(&self, id: &str) -> Option<ModelInfo>;

    /// The voices of `owner` (`None` is the app's owner) as the API shows them (the body of
    /// `GET /api/v1/voices`), each as JSON. Written the way the routes write them, so a number is
    /// the one a route sends.
    async fn list_voices(&self, owner: Option<&str>) -> Result<Vec<serde_json::Value>, ApiError>;

    /// The stored voices of `owner`, to look one up by the name it is shown as.
    async fn list_voice_profiles(&self, owner: Option<&str>)
    -> Result<Vec<VoiceProfile>, ApiError>;

    /// The voice of `owner` called `key`, by its id or else by the one name it is shown as.
    /// `voice_not_found` for a voice that is not theirs, as if it were not there.
    async fn load_voice(&self, owner: Option<&str>, key: &str) -> Result<VoiceProfile, ApiError>;

    /// Adds `voices` as new voices of `owner`, each with a new id. All or none: a failure removes
    /// the voices this call had already made. Tells the person's streams (`voices.changed`) and
    /// answers the new voices as the API shows them, in the order given.
    async fn create_voices(
        &self,
        owner: Option<&str>,
        voices: Vec<VoiceImport>,
    ) -> Result<Vec<serde_json::Value>, ApiError>;

    /// Lets a Pro feature through, or answers `402 needs_pro` naming it. A route or tool that
    /// belongs to a Pro feature calls this first.
    fn require(&self, feature: Feature) -> Result<(), ApiError> {
        if self.edition().allows(feature) {
            Ok(())
        } else {
            Err(ApiError::needs_pro(feature))
        }
    }

    /// Like [`Host::require`], for a voice or a job of `model`: a model whose licence limits
    /// commercial use is blocked from every Pro feature and from MCP, and works in the free core
    /// only (`403 model_not_commercial`). `None`, or a model the app does not know, is not blocked
    /// here.
    fn require_for_model(&self, feature: Feature, model: Option<&str>) -> Result<(), ApiError> {
        self.require(feature)?;
        self.require_commercial(feature, model)
    }

    /// The model half of [`Host::require_for_model`], for a path that is not gated as a whole.
    fn require_commercial(&self, feature: Feature, model: Option<&str>) -> Result<(), ApiError> {
        let info = model.and_then(|m| self.model_info(m));
        models::check_commercial(feature, info.as_ref())
    }

    /// Refuses a voice linked to a model the app does not have (`400 unknown_model`) or to one
    /// that does not read text aloud (`400 not_a_voice_model`).
    fn ensure_voice_model(&self, voice: &VoiceProfile) -> Result<(), ApiError> {
        models::check_voice_model(&voice.model, self.model_info(&voice.model).as_ref())
    }
}
