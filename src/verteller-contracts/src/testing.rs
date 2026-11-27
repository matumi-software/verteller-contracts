//! A stand-in for the app, for the unit tests of a Pro feature (the `testing` feature).
//!
//! [`FakeHost`] implements [`Host`] in memory: a [`StubJobs`] job service that keeps the jobs a
//! test gives it, a [`RecordingEvents`] sink that keeps what was published, a scratch folder that
//! goes away with the host, and voices kept in a list. Nothing starts a server, so a feature that
//! only needs the host can be tested without the app.

use crate::{
    ApiError, Caller, Edition, VoiceProfile,
    events::{Audience, Event, EventSink, Subscription, name},
    host::{Host, VoiceImport, WriteGuard},
    ids,
    jobs::{Job, JobService, JobServiceError},
    models::ModelInfo,
    music::{MakeMusic, Music, MusicError, MusicRequest},
    transcribe::{Transcribe, TranscribeError, Transcript},
};
use async_trait::async_trait;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;

fn locked<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A job service that keeps the jobs it is given and makes none. `create` answers `ModelNotReady`,
/// the way an app with no model downloaded does.
#[derive(Default)]
pub struct StubJobs {
    jobs: Mutex<Vec<Job>>,
    outputs: Mutex<HashMap<String, PathBuf>>,
}

impl StubJobs {
    /// Adds `job`, newest first.
    pub fn add(&self, job: Job) {
        locked(&self.jobs).insert(0, job);
    }

    /// Says that job `id` has its joined WAV at `path`.
    pub fn set_output(&self, id: &str, path: PathBuf) {
        locked(&self.outputs).insert(id.to_string(), path);
    }

    fn find(&self, id: &str) -> Result<Job, JobServiceError> {
        locked(&self.jobs)
            .iter()
            .find(|j| j.id == id)
            .cloned()
            .ok_or(JobServiceError::JobNotFound)
    }
}

#[async_trait]
impl JobService for StubJobs {
    async fn list(&self, limit: usize) -> Vec<Job> {
        locked(&self.jobs).iter().take(limit).cloned().collect()
    }

    async fn create(&self, _new: crate::jobs::NewJob) -> Result<Job, JobServiceError> {
        Err(JobServiceError::ModelNotReady)
    }

    async fn get(&self, id: &str) -> Result<Job, JobServiceError> {
        self.find(id)
    }

    async fn cancel(&self, id: &str) -> Result<Job, JobServiceError> {
        self.find(id)?;
        Err(JobServiceError::JobFinished)
    }

    async fn set_tags(&self, id: &str, _tags: Vec<String>) -> Result<Job, JobServiceError> {
        self.find(id)
    }

    async fn set_title(&self, id: &str, _title: String) -> Result<Job, JobServiceError> {
        self.find(id)
    }

    async fn delete(&self, id: &str) -> Result<(), JobServiceError> {
        self.find(id)?;
        locked(&self.jobs).retain(|j| j.id != id);
        Ok(())
    }

    async fn output_path(&self, id: &str) -> Result<PathBuf, JobServiceError> {
        self.find(id)?;
        locked(&self.outputs)
            .get(id)
            .cloned()
            .ok_or(JobServiceError::JobNotDone)
    }

    async fn segment_path(&self, id: &str, _index: usize) -> Result<PathBuf, JobServiceError> {
        self.find(id)?;
        Err(JobServiceError::SegmentNotFound)
    }

    async fn remove_owner(&self, owner: &str) {
        locked(&self.jobs).retain(|j| j.owner.as_deref() != Some(owner));
    }
}

/// An event sink that keeps every event it is given, with ids that go up from 1, and feeds live
/// subscribers.
pub struct RecordingEvents {
    events: Mutex<Vec<Event>>,
    tx: broadcast::Sender<Event>,
}

impl Default for RecordingEvents {
    fn default() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            tx: broadcast::channel(1024).0,
        }
    }
}

impl RecordingEvents {
    /// Every event published so far, oldest first.
    pub fn all(&self) -> Vec<Event> {
        locked(&self.events).clone()
    }

    /// The events published under `name`, oldest first.
    pub fn named(&self, name: &str) -> Vec<Event> {
        locked(&self.events)
            .iter()
            .filter(|e| e.name == name)
            .cloned()
            .collect()
    }
}

impl EventSink for RecordingEvents {
    fn publish_to(&self, audience: Audience, name: &str, data: String) -> Event {
        let mut events = locked(&self.events);
        let event = Event {
            id: events.len() as u64 + 1,
            name: name.to_string(),
            data,
            audience,
        };
        events.push(event.clone());
        let _ = self.tx.send(event.clone());
        event
    }

    fn subscribe(&self, last_event_id: Option<u64>) -> Subscription {
        let events = locked(&self.events);
        let cursor = events.len() as u64;
        let live = self.tx.subscribe();
        match last_event_id {
            Some(last) if last <= cursor => Subscription {
                needs_snapshot: false,
                backlog: events.iter().filter(|e| e.id > last).cloned().collect(),
                cursor,
                live,
            },
            _ => Subscription {
                needs_snapshot: true,
                backlog: Vec::new(),
                cursor,
                live,
            },
        }
    }
}

/// A speech to text service with no model: nothing is ready and every request answers why.
pub struct NoTranscriber;

#[async_trait]
impl Transcribe for NoTranscriber {
    fn ready(&self) -> bool {
        false
    }

    async fn transcribe(
        &self,
        _audio: crate::Audio,
        _language: Option<String>,
    ) -> Result<Transcript, TranscribeError> {
        Err(TranscribeError::NotReady)
    }
}

/// A music service with no model: nothing is ready and every request answers why.
pub struct NoMusic;

#[async_trait]
impl MakeMusic for NoMusic {
    fn ready(&self) -> bool {
        false
    }

    async fn make(
        &self,
        _request: MusicRequest,
        _cancel: crate::CancelToken,
    ) -> Result<Music, MusicError> {
        Err(MusicError::NotReady)
    }
}

/// The app, in memory. Every field is public so a test sets what it needs; `new` is a free
/// edition, no model, no voice and a speech to text service that is not ready.
pub struct FakeHost {
    /// What the build lets the person do. Set it to a Pro edition to pass `require`.
    pub edition: Edition,
    /// The job service. Add the jobs a test reads with [`StubJobs::add`].
    pub jobs: Arc<StubJobs>,
    /// What was published.
    pub events: Arc<RecordingEvents>,
    /// Speech to text. [`NoTranscriber`] by default.
    pub transcriber: Arc<dyn Transcribe>,
    /// Making music. [`NoMusic`] by default.
    pub music: Arc<dyn MakeMusic>,
    /// The speech models the host knows.
    pub models: Vec<ModelInfo>,
    /// The `Host` header values this listener answers to.
    pub allowed_hosts: Vec<String>,
    /// The owner's user name.
    pub owner_user_name: String,
    /// The stored voices, of everyone: each says whose it is in `owner`.
    pub voices: Mutex<Vec<VoiceProfile>>,
    /// The voices [`Host::create_voices`] was asked for, with who they were for.
    pub imported: Mutex<Vec<(Option<String>, VoiceImport)>>,
    /// Held to read by [`Host::writing_for`]; take it to write to wait for the guards.
    pub gate: Arc<tokio::sync::RwLock<()>>,
    tmp: tempfile::TempDir,
}

impl Default for FakeHost {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeHost {
    /// A free edition with nothing in it.
    pub fn new() -> Self {
        Self {
            edition: Edition::free(),
            jobs: Arc::default(),
            events: Arc::default(),
            transcriber: Arc::new(NoTranscriber),
            music: Arc::new(NoMusic),
            models: Vec::new(),
            allowed_hosts: vec!["127.0.0.1".into(), "localhost".into()],
            owner_user_name: "owner".into(),
            voices: Mutex::new(Vec::new()),
            imported: Mutex::new(Vec::new()),
            gate: Arc::default(),
            tmp: tempfile::tempdir().expect("a scratch folder"),
        }
    }

    /// The same host with `edition`.
    pub fn with_edition(mut self, edition: Edition) -> Self {
        self.edition = edition;
        self
    }

    /// The same host knowing `model`.
    pub fn with_model(mut self, model: ModelInfo) -> Self {
        self.models.push(model);
        self
    }

    fn show(profile: &VoiceProfile) -> serde_json::Value {
        serde_json::json!({
            "name": profile.name,
            "displayName": profile.shown_name(),
            "speaker": profile.consent.speaker,
            "recordedAt": profile.consent.recorded_at,
            "model": profile.model,
        })
    }
}

#[async_trait]
impl Host for FakeHost {
    fn edition(&self) -> Edition {
        self.edition.clone()
    }

    async fn writing_for(&self, _caller: &Caller) -> Result<WriteGuard, ApiError> {
        Ok(WriteGuard::new(self.gate.clone().read_owned().await))
    }

    fn jobs(&self) -> Arc<dyn JobService> {
        self.jobs.clone()
    }

    fn events(&self) -> Arc<dyn EventSink> {
        self.events.clone()
    }

    fn transcriber(&self) -> Arc<dyn Transcribe> {
        self.transcriber.clone()
    }

    fn music(&self) -> Arc<dyn MakeMusic> {
        self.music.clone()
    }

    fn playlist_folder(&self) -> PathBuf {
        self.tmp.path().join("playlist")
    }

    fn tmp_dir(&self) -> PathBuf {
        self.tmp.path().to_path_buf()
    }

    fn output_folder(&self) -> PathBuf {
        self.tmp.path().join("output")
    }

    fn allowed_hosts(&self) -> Vec<String> {
        self.allowed_hosts.clone()
    }

    async fn owner_user_name(&self) -> String {
        self.owner_user_name.clone()
    }

    fn model_info(&self, id: &str) -> Option<ModelInfo> {
        self.models.iter().find(|m| m.id == id).cloned()
    }

    async fn list_voices(&self, owner: Option<&str>) -> Result<Vec<serde_json::Value>, ApiError> {
        Ok(locked(&self.voices)
            .iter()
            .filter(|v| v.owner.as_deref() == owner)
            .map(Self::show)
            .collect())
    }

    async fn list_voice_profiles(
        &self,
        owner: Option<&str>,
    ) -> Result<Vec<VoiceProfile>, ApiError> {
        Ok(locked(&self.voices)
            .iter()
            .filter(|v| v.owner.as_deref() == owner)
            .cloned()
            .collect())
    }

    async fn load_voice(&self, owner: Option<&str>, key: &str) -> Result<VoiceProfile, ApiError> {
        locked(&self.voices)
            .iter()
            .find(|v| v.owner.as_deref() == owner && (v.name == key || v.shown_name() == key))
            .cloned()
            .ok_or_else(|| ApiError::voice_not_found(key))
    }

    /// Keeps the voices in the list and in [`FakeHost::imported`] without copying their files, and
    /// publishes `voices.changed` to `owner`.
    async fn create_voices(
        &self,
        owner: Option<&str>,
        voices: Vec<VoiceImport>,
    ) -> Result<Vec<serde_json::Value>, ApiError> {
        let mut shown = Vec::new();
        for v in voices {
            let id = ids::new_id(ids::VOICE, |id| {
                locked(&self.voices).iter().any(|p| p.name == id)
            });
            let profile = VoiceProfile {
                dir: self.tmp.path().join("voices").join(&id),
                name: id,
                display_name: v.display_name.clone(),
                transcript: v.transcript.clone(),
                consent: v.consent.clone(),
                cleanup: v.cleanup,
                owner: owner.map(str::to_string),
                model: v.model.clone(),
                settings: v.settings.clone(),
                sample_seconds: v.sample_seconds,
                sample_source: v.sample_source,
                words: v.words.clone(),
                last_used_at: None,
            };
            shown.push(Self::show(&profile));
            locked(&self.voices).push(profile);
            locked(&self.imported).push((owner.map(str::to_string), v));
        }
        self.events.publish_to(
            Audience::Owner(owner.map(str::to_string)),
            name::VOICES_CHANGED,
            crate::events::json_line(&serde_json::json!({ "voices": shown })),
        );
        Ok(shown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Consent, Feature,
        events::EventSinkExt,
        jobs::{JobSource, JobState},
    };

    fn consent() -> Consent {
        Consent {
            speaker: "Sam Speaker".into(),
            statement: "I agree.".into(),
            recorded_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn import(display: &str) -> VoiceImport {
        VoiceImport {
            display_name: Some(display.into()),
            model: "m".into(),
            settings: Default::default(),
            sample_seconds: None,
            sample_source: Default::default(),
            transcript: None,
            words: Vec::new(),
            cleanup: None,
            consent: consent(),
            reference: PathBuf::from("reference.wav"),
            cleaned: None,
        }
    }

    #[test]
    fn a_free_host_refuses_a_pro_feature_and_a_pro_one_lets_it_through() {
        let free = FakeHost::new();
        let e = free.require(Feature::Editor).unwrap_err();
        assert_eq!((e.status.as_u16(), e.code), (402, "needs_pro"));
        struct All;
        impl crate::Entitlements for All {
            fn tier(&self) -> crate::Tier {
                crate::Tier::Pro
            }
            fn is_granted(&self, _: &str) -> bool {
                true
            }
            fn limit(&self, _: &str) -> Option<u64> {
                None
            }
        }
        let pro = FakeHost::new().with_edition(Edition::new(Arc::new(All)));
        assert!(pro.require(Feature::Editor).is_ok());
    }

    #[test]
    fn a_non_commercial_model_is_refused_by_name_and_an_unknown_one_is_not() {
        let host = FakeHost::new().with_model(ModelInfo {
            id: "nc".into(),
            name: "Model NC".into(),
            licence: "CC-BY-NC-4.0".into(),
            commercial_use: false,
            reads_aloud: true,
            max_segment_chars: None,
            basic_settings: Vec::new(),
        });
        let e = host
            .require_commercial(Feature::Mcp, Some("nc"))
            .unwrap_err();
        assert_eq!(e.code, "model_not_commercial");
        assert!(host.require_commercial(Feature::Mcp, Some("other")).is_ok());
        assert!(host.require_commercial(Feature::Mcp, None).is_ok());
    }

    #[tokio::test]
    async fn the_events_a_feature_publishes_are_recorded_in_order() {
        let host = FakeHost::new();
        let mut live = host.events().subscribe(None).live;
        host.events()
            .publish_json("project.changed", &serde_json::json!({ "n": 1 }));
        host.events().publish_json_to(
            Audience::Owner(Some("sam".into())),
            "project.deleted",
            &serde_json::json!({ "n": 2 }),
        );
        let all = host.events.all();
        assert_eq!(all.iter().map(|e| e.id).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(host.events.named("project.deleted").len(), 1);
        assert_eq!(live.recv().await.unwrap().name, "project.changed");
        let again = host.events().subscribe(Some(1));
        assert!(!again.needs_snapshot);
        assert_eq!(again.backlog.len(), 1);
    }

    #[tokio::test]
    async fn voices_come_back_by_id_or_shown_name_and_only_to_their_owner() {
        let host = FakeHost::new();
        let made = host
            .create_voices(Some("sam"), vec![import("Office mic"), import("Phone")])
            .await
            .unwrap();
        assert_eq!(made.len(), 2);
        let id = made[0]["name"].as_str().unwrap().to_string();
        assert!(id.starts_with("vce_"));
        assert!(host.load_voice(Some("sam"), &id).await.is_ok());
        assert!(host.load_voice(Some("sam"), "Phone").await.is_ok());
        let e = host.load_voice(None, &id).await.unwrap_err();
        assert_eq!(e.code, "voice_not_found");
        assert_eq!(host.list_voices(Some("sam")).await.unwrap().len(), 2);
        assert!(host.list_voices(None).await.unwrap().is_empty());
        assert_eq!(host.events.named("voices.changed").len(), 1);
        assert_eq!(host.imported.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn the_stub_jobs_keep_what_they_are_given_and_make_none() {
        let host = FakeHost::new();
        assert!(matches!(
            host.jobs().get("vov_aaaaaa").await,
            Err(JobServiceError::JobNotFound)
        ));
        let job: Job = serde_json::from_value(serde_json::json!({
            "id": "vov_aaaaaa",
            "title": "t",
            "voice": "vce_aaaaaa",
            "source": "mcp",
            "state": "done",
            "createdAt": "2026-01-01T00:00:00Z",
            "segments": [],
            "owner": "sam",
        }))
        .unwrap();
        assert_eq!((job.state, job.source), (JobState::Done, JobSource::Mcp));
        host.jobs.add(job);
        host.jobs
            .set_output("vov_aaaaaa", host.tmp_dir().join("out.wav"));
        let jobs = host.jobs();
        assert!(
            crate::jobs::owned_job(jobs.as_ref(), "vov_aaaaaa", Some("sam"))
                .await
                .is_ok()
        );
        // Someone else's job is not there.
        assert!(matches!(
            crate::jobs::owned_output(jobs.as_ref(), "vov_aaaaaa", None).await,
            Err(JobServiceError::JobNotFound)
        ));
        let (_, path) = crate::jobs::owned_output(jobs.as_ref(), "vov_aaaaaa", Some("sam"))
            .await
            .unwrap();
        assert!(path.ends_with("out.wav"));
    }

    #[tokio::test]
    async fn a_removal_waits_for_a_writer_to_finish() {
        let host = FakeHost::new();
        let caller = Caller {
            person_id: "usr_aaaaaa".into(),
            user_name: "owner".into(),
            owner: true,
            has_password: false,
            session: None,
            on_host: true,
        };
        let guard = host.writing_for(&caller).await.unwrap();
        assert!(host.gate.try_write().is_err());
        drop(guard);
        assert!(host.gate.try_write().is_ok());
    }

    #[tokio::test]
    async fn music_is_not_ready_until_a_test_says_so_and_playlist_has_a_folder_of_its_own() {
        let host = FakeHost::new();
        let m = host.music();
        assert!(!m.ready());
        let e = m
            .make(
                MusicRequest {
                    prompt: "calm".into(),
                    style: None,
                    seconds: 10.0,
                    seed: 1,
                    settings: Default::default(),
                    model: None,
                },
                crate::CancelToken::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(e, MusicError::NotReady);
        assert!(host.playlist_folder().starts_with(host.tmp_dir()));
        assert_ne!(host.playlist_folder(), host.output_folder());
    }

    #[tokio::test]
    async fn speech_to_text_is_not_ready_until_a_test_says_so() {
        let host = FakeHost::new();
        let t = host.transcriber();
        assert!(!t.ready() && !t.words_ready() && !t.align_ready());
        let e = t
            .transcribe(
                crate::Audio {
                    samples: vec![],
                    sample_rate: 16_000,
                },
                None,
            )
            .await
            .unwrap_err();
        assert_eq!(e, TranscribeError::NotReady);
    }
}
