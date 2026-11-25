//! Jobs as the routes and the Pro features see them, and the trait the job queue implements.
//!
//! The routes only create, read and cancel jobs through [`JobService`]. The queue, its model thread
//! and the history on disk are the app's. A queue publishes the job events itself, on the app's
//! event bus. What the app keeps to itself (the settings, the model source, the preparations a
//! voice has) is in the app's own `JobServiceSetup` trait, which has this one as its supertrait.

use crate::{
    DEFAULT_PARAGRAPH_PAUSE_MS, DEFAULT_SENTENCE_PAUSE_MS, Edition, MAX_PAUSE_MARKER_MS,
    VoiceProfile,
    auditions::{Audition, NewAudition},
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};

/// `Job.state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    /// Waiting its turn.
    Queued,
    /// The model is reading it.
    Reading,
    /// Finished, with an output file.
    Done,
    /// Ended with an error.
    Failed,
    /// Stopped by the person.
    Cancelled,
    /// Never read: a duplicate that waits as a draft. It has its parts and no audio, and a draft
    /// (`GET /jobs/{id}/draft`) that is read and saved into it.
    Draft,
}

impl JobState {
    /// True once the job has ended and can no longer be cancelled.
    pub fn is_finished(self) -> bool {
        matches!(
            self,
            Self::Done | Self::Failed | Self::Cancelled | Self::Draft
        )
    }
}

/// `Segment.state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum SegmentState {
    /// Waiting.
    Queued,
    /// The model is reading it.
    Reading,
    /// Its WAV exists.
    Ready,
    /// The model failed on it.
    Failed,
}

/// `Job.source`: who asked for the job.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum JobSource {
    /// The React UI.
    #[default]
    Ui,
    /// An MCP client.
    Mcp,
    /// The Pro editor: the voiceover is the export of a project.
    Editor,
}

/// `Job.error` and `Segment.error`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct JobError {
    /// `gpu_out_of_memory` or `backend_failed`.
    pub code: String,
    /// What went wrong.
    pub message: String,
}

/// One piece of a job's script.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    /// Starts at 0.
    pub index: usize,
    /// The text of this piece.
    pub text: String,
    /// The piece ends a paragraph, so the pause after it is the paragraph pause. Jobs saved before
    /// this field existed load as `false`.
    #[serde(default)]
    pub ends_paragraph: bool,
    /// Silence the script asked for before this piece with a pause marker, in milliseconds. Only
    /// the first piece can have any. Jobs saved before pause markers existed load as 0.
    #[serde(default)]
    pub pause_before_ms: u32,
    /// Silence the script asked for after this piece with pause markers, on top of the sentence or
    /// paragraph pause. After the last piece it is silence at the end of the file. Jobs saved
    /// before pause markers existed load as 0.
    #[serde(default)]
    pub pause_after_ms: u32,
    /// Where it is.
    pub state: SegmentState,
    /// Length of its audio, set when it is ready.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub seconds: Option<f32>,
    /// Where this piece starts in the saved file, in seconds to the millisecond. Set once this
    /// piece and every one before it are ready. It is worked out from the job's own numbers, so a
    /// job saved before cue times existed has it too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub start_seconds: Option<f32>,
    /// Where this piece ends in the saved file. Set together with `start_seconds`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub end_seconds: Option<f32>,
    /// Set when it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub error: Option<JobError>,
}

/// `Job.output`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct JobOutput {
    /// An absolute path on the machine running the app. Left out for a member: they download the
    /// audio with `GET /jobs/{id}/output.wav`.
    #[cfg_attr(feature = "ts", ts(optional, as = "Option<String>"))]
    pub path: String,
    /// Length of the joined audio.
    pub seconds: f32,
}

/// A voiceover job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Job {
    /// The voiceover's id: `vov_` and six hex characters.
    pub id: String,
    /// What the person called it.
    pub title: String,
    /// The voice's name.
    pub voice: String,
    /// The id of the speech model the voiceover was read with: the model of the voice when the job
    /// was made. Left out for a job saved before this was kept, which the UI reads as the voice's
    /// model now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub model: Option<String>,
    /// Who asked.
    pub source: JobSource,
    /// Where it is.
    pub state: JobState,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339, set when the first segment starts.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub started_at: Option<String>,
    /// RFC 3339, set when the job ends.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub finished_at: Option<String>,
    /// Silence after a piece inside a paragraph, in milliseconds. Jobs saved before it was a
    /// setting load with the default.
    #[serde(default = "default_sentence_pause_ms")]
    pub sentence_pause_ms: u32,
    /// Silence after a piece that ends a paragraph, in milliseconds. Jobs saved before it was a
    /// setting load with the default.
    #[serde(default = "default_paragraph_pause_ms")]
    pub paragraph_pause_ms: u32,
    /// One per piece of the script, from the moment the job is created.
    pub segments: Vec<Segment>,
    /// Set when the job is done.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub output: Option<JobOutput>,
    /// Set when the job failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub error: Option<JobError>,
    /// The id of the member who asked for it. Left out for the owner's jobs, and for jobs from
    /// before there were people, which are the owner's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub owner: Option<String>,
    /// The user name of the person who asked for it. For the owner's jobs, and the ones Claude
    /// Desktop asks for, that is the owner's. A job saved before this field existed is empty until
    /// the app names the owner's own jobs when it starts.
    #[serde(default)]
    pub made_by: String,
    /// What the person tagged it with: single words, shown with a hash. Left out when there are
    /// none, and for a job saved before tags existed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "ts", ts(optional, as = "Option<Vec<String>>"))]
    pub tags: Vec<String>,
    /// Set on a try-out: a voiceover read from the draft of the saved voiceover with this id. A
    /// try-out is kept apart from the saved ones: it is not in the lists and it is not counted in
    /// the history. Saving the draft makes the saved voiceover what the try-out is, under its own
    /// id. Left out for every other voiceover.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub draft_of: Option<String>,
    /// Whether the voiceover has a draft: changes kept beside it until they are saved or discarded
    /// (`GET /jobs/{id}/draft`). Left out when it has none.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(feature = "ts", ts(optional, as = "Option<bool>"))]
    pub has_draft: bool,
    /// Set on a voiceover that is the export of an editor project: which project, and when it was
    /// last exported. Such a voiceover has no parts and no script. Left out for every other one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub export_of: Option<ExportOf>,
}

/// `Job.exportOf`: the editor project a voiceover is the export of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ExportOf {
    /// The project's id, `prj_` and six hex characters. The project may be gone: this is then only a
    /// label.
    pub project_id: String,
    /// The project's title when it was exported.
    pub project_title: String,
    /// RFC 3339, the time of the latest export into this voiceover.
    pub exported_at: String,
}

fn default_sentence_pause_ms() -> u32 {
    DEFAULT_SENTENCE_PAUSE_MS
}

fn default_paragraph_pause_ms() -> u32 {
    DEFAULT_PARAGRAPH_PAUSE_MS
}

impl Job {
    /// Works out where each ready piece sits in the saved file, the way the file is built: the
    /// silence the script asks for before the first piece, then each piece followed by the
    /// paragraph or sentence pause (none after the last piece) plus the silence its markers ask
    /// for. A piece gets its times only when it and every piece before it are ready, and the
    /// others lose theirs. Times are rounded to the millisecond.
    pub fn fill_cues(&mut self) {
        let mut cursor_ms = 0.0f64;
        let mut known = true;
        let last = self.segments.len().saturating_sub(1);
        for (i, seg) in self.segments.iter_mut().enumerate() {
            let seconds = seg
                .seconds
                .filter(|_| known && seg.state == SegmentState::Ready);
            let Some(seconds) = seconds else {
                known = false;
                seg.start_seconds = None;
                seg.end_seconds = None;
                continue;
            };
            cursor_ms += f64::from(seg.pause_before_ms);
            let end_ms = cursor_ms + f64::from(seconds) * 1000.0;
            seg.start_seconds = Some(to_seconds(cursor_ms));
            seg.end_seconds = Some(to_seconds(end_ms));
            let base = match (i == last, seg.ends_paragraph) {
                (true, _) => 0,
                (false, true) => self.paragraph_pause_ms,
                (false, false) => self.sentence_pause_ms,
            };
            cursor_ms = end_ms + f64::from(base) + f64::from(seg.pause_after_ms);
        }
    }

    /// The job's script as text, cut the way the job was: a blank line after a part that ends a
    /// paragraph, a space after the others, its pause markers written back and a backslash before
    /// any brace (a brace is spoken in a script only with one). Reading it again gives the same
    /// parts and pauses. This is what a duplicate starts from.
    pub fn script(&self) -> String {
        fn markers(ms: u32) -> Vec<String> {
            let mut out = Vec::new();
            let mut left = ms;
            while left > 0 {
                let each = left.min(MAX_PAUSE_MARKER_MS);
                out.push(format!("{{pause {each}ms}}"));
                left -= each;
            }
            out
        }
        let mut out = String::new();
        for (i, seg) in self.segments.iter().enumerate() {
            let mut parts: Vec<String> = Vec::new();
            if i == 0 {
                parts.extend(markers(seg.pause_before_ms));
            }
            parts.push(seg.text.replace('{', "\\{"));
            parts.extend(markers(seg.pause_after_ms));
            out.push_str(&parts.join(" "));
            if i + 1 < self.segments.len() {
                out.push_str(if seg.ends_paragraph { "\n\n" } else { " " });
            }
        }
        out
    }
}

fn to_seconds(ms: f64) -> f32 {
    (ms.round() / 1000.0) as f32
}

/// What `POST /api/v1/jobs` hands the queue. The route has already found the voice and checked
/// that the script has words.
#[derive(Debug, Clone)]
pub struct NewJob {
    /// What the person called it. The queue picks a title when this is empty.
    pub title: Option<String>,
    /// The stored voice to read in.
    pub voice: VoiceProfile,
    /// The text.
    pub script: String,
    /// Who asked.
    pub source: JobSource,
    /// The pause inside a paragraph as the caller wrote it, `None` for the default. The queue
    /// checks the range.
    pub sentence_pause_ms: Option<i64>,
    /// The pause after a paragraph as the caller wrote it, `None` for the default.
    pub paragraph_pause_ms: Option<i64>,
    /// Whose job it is:`None` for the owner, the member's id otherwise. The job carries it.
    pub owner: Option<String>,
    /// The user name of the person who asks, kept on the job as `madeBy`.
    pub made_by: String,
    /// Makes a try-out of the draft of the saved voiceover with this id ([`Job::draft_of`]).
    pub draft_of: Option<String>,
}

/// What the editor hands the service to make or update the voiceover that is the export of a
/// project ([`JobService::add_export`], [`JobService::replace_export`]).
#[derive(Debug, Clone)]
pub struct NewExport {
    /// What the voiceover is called.
    pub title: String,
    /// A finished WAV file the editor rendered. The service moves it into the output folder, so the
    /// editor must not use it again.
    pub wav: PathBuf,
    /// Its length in seconds.
    pub seconds: f32,
    /// The voice's name when one voice speaks in the project, else empty.
    pub voice: String,
    /// The model of that voice, when there is one.
    pub model: Option<String>,
    /// The project this is the export of.
    pub project_id: String,
    /// The project's title.
    pub project_title: String,
    /// Whose it is: `None` for the owner, the member's id otherwise.
    pub owner: Option<String>,
    /// The user name of the person who exports, kept as `madeBy`.
    pub made_by: String,
}

/// Why a [`JobService`] call failed. Each one is a problem code in `docs/api.md`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JobServiceError {
    /// `409 model_not_ready`.
    #[error("the model is not loaded yet")]
    ModelNotReady,
    /// `400 script_empty`.
    #[error("the script has no words")]
    ScriptEmpty,
    /// `400 invalid_script`: a pause marker that is malformed or over a limit. The text is the
    /// sentence for the person.
    #[error("{0}")]
    InvalidScript(String),
    /// `400 script_too_large`: too many parts, or too much silence. The text is the sentence for
    /// the person.
    #[error("{0}")]
    ScriptTooLarge(String),
    /// `402 needs_pro`: a voiceover longer than the free version reads. The text is the sentence
    /// for the person.
    #[error("{0}")]
    NeedsPro(String),
    /// `400 invalid_pause`: a sentence or paragraph pause outside its range.
    #[error("{0}")]
    InvalidPause(String),
    /// `400 invalid_tag`: a tag that is not one word of the allowed characters, too long, or too
    /// many of them. The text is the sentence for the person.
    #[error("{0}")]
    InvalidTag(String),
    /// `400 invalid_title`: a title that is empty, too long or has a control character in it. The
    /// text is the sentence for the person.
    #[error("{0}")]
    InvalidTitle(String),
    /// `404 job_not_found`.
    #[error("no such job")]
    JobNotFound,
    /// `404 draft_not_found`: the voiceover has no draft.
    #[error("this voiceover has no draft")]
    DraftNotFound,
    /// `409 draft_busy`: the try-out of the draft is still being read.
    #[error("the try-out of this draft is still being read: stop it or wait for it")]
    DraftBusy,
    /// `409 draft_not_tried`: the draft was changed after its last try-out, or has none that is
    /// done, and saving it would save audio that is not its own.
    #[error("generate the draft again before saving it, so the saved audio is the draft's own")]
    DraftNotTried,
    /// `409 draft_out_of_date`: the saved voiceover changed since the draft began.
    #[error("the saved voiceover changed after this draft was started")]
    DraftOutOfDate,
    /// `409 job_finished`: cancel on a job that already ended.
    #[error("the job already ended")]
    JobFinished,
    /// `409 job_running`: a delete on a job that is queued or being read. It is cancelled first.
    #[error("the job is still running, cancel it first")]
    JobRunning,
    /// `409 job_not_done`: output asked for before the job is done.
    #[error("the job is not done")]
    JobNotDone,
    /// `404 segment_not_found`: no such segment, or its WAV is not ready.
    #[error("that segment is not ready")]
    SegmentNotFound,
    /// `404 audition_not_found`: no such audition, or it expired or was replaced.
    #[error("no such audition")]
    AuditionNotFound,
    /// `409 audition_not_ready`: audio asked before the audition is ready.
    #[error("the audition is not ready")]
    AuditionNotReady,
    /// `400 audition_too_large`: more text, parts or silence than an audition takes. The text is the
    /// sentence for the person.
    #[error("{0}")]
    AuditionTooLarge(String),
    /// `500 internal_error`.
    #[error("{0}")]
    Internal(String),
}

/// The job queue, as the routes and the MCP tools use it.
///
/// A real implementation owns a model thread and talks to it over channels, so every method only
/// waits on a channel or reads a file. Implementations publish the job events on the event bus
/// (`job.created`, `job.state`, `segment.started`, `segment.ready`, `job.done`, `job.failed`,
/// `job.cancelled`, `jobs.changed`), each to the job's owner only, and
/// `jobs.changed` with that owner's jobs only. The routes publish none of those.
///
/// The service itself does not check who asks: `get`, `cancel` and the rest take any id, and the
/// routes and the MCP tools check the job's `owner` first.
#[async_trait]
pub trait JobService: Send + Sync + 'static {
    /// Hands the service the edition, so a new job is checked against the free limit on a
    /// voiceover's length. Without it there is no limit.
    fn use_edition(&self, _edition: Edition) {}

    /// Hands the service what to call when a person's voices changed by something the service did
    /// (a voice was prepared, lost its preparation, or was read), so the server can send
    /// `voices.changed`. Nothing by default.
    fn use_voice_news(&self, _news: VoiceNews) {}

    /// Which models are loaded, and which is loading. Nothing is loaded by default.
    fn warm_state(&self) -> WarmState {
        WarmState::default()
    }

    /// Loads the model `model` in the background, and prepares `voice` in it when one is given.
    /// `ModelNotReady` when the model's files are not there. The result is the `model.warm` event.
    async fn warm_model(
        &self,
        _model: &str,
        _voice: Option<VoiceProfile>,
    ) -> Result<(), JobServiceError> {
        Err(JobServiceError::ModelNotReady)
    }

    /// Stops the preparation of `voice` (of `owner`) that is queued or running, and shows the voice as
    /// not prepared. False when it is not being prepared.
    fn cancel_prepare(&self, _owner: Option<&str>, _voice: &str) -> bool {
        false
    }

    /// Unloads the model `model`. Nothing happens when it is not loaded.
    fn unload_model(&self, _model: &str) {}

    /// Unloads every loaded model, for a device switch: each loads again on the new device the next
    /// time a voice of it is read. Nothing happens when none is loaded.
    fn unload_all_models(&self) {}

    /// Whether a job in a voice of `model` could start now: the model is loaded, or its files are
    /// in place. False by default.
    fn can_run_model(&self, _model: &str) -> bool {
        false
    }

    /// The most recent jobs of everyone, newest first, at most `limit`. For the tray, which shows
    /// the whole computer's work.
    async fn list(&self, limit: usize) -> Vec<Job>;

    /// The most recent jobs stored under `owner` (`None` is the app's owner), newest first, at
    /// most `limit`.
    async fn list_owned(&self, owner: Option<&str>, limit: usize) -> Vec<Job> {
        self.list(usize::MAX)
            .await
            .into_iter()
            .filter(|j| j.owner.as_deref() == owner)
            .take(limit)
            .collect()
    }

    /// Queues a job. The returned job is `queued` and already has its segments.
    async fn create(&self, new: NewJob) -> Result<Job, JobServiceError>;

    /// Makes a voiceover that waits as a draft: the script cut into its parts, `tags`, and no
    /// audio. It is not read, so the free limit and the model do not matter yet (they are checked
    /// when the draft is generated). The returned job is `draft` and has a draft. This is what a
    /// duplicate of a voiceover starts as.
    async fn create_draft(&self, _new: NewJob, _tags: Vec<String>) -> Result<Job, JobServiceError> {
        Err(JobServiceError::Internal(
            "this job service keeps no drafts".into(),
        ))
    }

    /// One job.
    async fn get(&self, id: &str) -> Result<Job, JobServiceError>;

    /// Cancels a queued or reading job and returns it as it stands.
    async fn cancel(&self, id: &str) -> Result<Job, JobServiceError>;

    /// Replaces a job's tags with `tags`, which the caller has already checked, and returns
    /// the job. Any job can be tagged, running or finished. The history keeps them. Publishes
    /// `job.state` and `jobs.changed`.
    async fn set_tags(&self, id: &str, tags: Vec<String>) -> Result<Job, JobServiceError>;

    /// Sets a job's title to `title`, which the caller has already checked, and returns
    /// the job. Any job can be renamed, running or finished. Its id, its audio and its file name
    /// stay as they were. Publishes `job.state` and `jobs.changed`.
    async fn set_title(&self, id: &str, title: String) -> Result<Job, JobServiceError>;

    /// Sets whether the voiceover `id` shows as having a draft, saves it and publishes `job.state` and
    /// `jobs.changed`. The draft itself is a file the routes keep.
    async fn set_has_draft(&self, _id: &str, _has: bool) -> Result<Job, JobServiceError> {
        Err(JobServiceError::JobNotFound)
    }

    /// Makes the saved voiceover `original` what its done try-out `try_out` is: the title, voice,
    /// pauses, parts and audio, under the id it has. The old audio goes, the try-out is removed and
    /// the id, the creation time, the tags and the owner stay. `JobNotDone` unless the try-out is
    /// done, and `DraftNotFound` when it is not a try-out of `original`. Publishes `job.state` and
    /// `jobs.changed`.
    async fn commit_draft(&self, _original: &str, _try_out: &str) -> Result<Job, JobServiceError> {
        Err(JobServiceError::Internal(
            "this job service keeps no drafts".into(),
        ))
    }

    /// Names the jobs of the owner that were saved before `madeBy` existed. Nothing by default.
    fn name_owner_jobs(&self, _user_name: &str) {}

    /// Deletes a finished job: its entry in the history, its segment audio and its joined WAV in
    /// the output folder. A queued or reading job is `JobRunning`. Publishes `job.deleted` and
    /// `jobs.changed`.
    async fn delete(&self, id: &str) -> Result<(), JobServiceError>;

    /// The path of the joined WAV of a done job.
    async fn output_path(&self, id: &str) -> Result<PathBuf, JobServiceError>;

    /// Adds a done voiceover that is the export of an editor project ([`Job::export_of`]), from the
    /// WAV the editor rendered. It has no parts. Publishes `job.created`, `job.done` and
    /// `jobs.changed`.
    async fn add_export(&self, _new: NewExport) -> Result<Job, JobServiceError> {
        Err(JobServiceError::Internal(
            "this job service keeps no exports".into(),
        ))
    }

    /// Swaps the audio of the export `id` for a newly rendered one, keeping its id, creation time,
    /// tags and owner, and removes the old audio. `JobNotFound` when there is no such voiceover or it
    /// is not an export. Publishes `job.state` and `jobs.changed`.
    async fn replace_export(&self, _id: &str, _new: NewExport) -> Result<Job, JobServiceError> {
        Err(JobServiceError::Internal(
            "this job service keeps no exports".into(),
        ))
    }

    /// The path of the WAV of segment `index` once it is ready.
    async fn segment_path(&self, id: &str, index: usize) -> Result<PathBuf, JobServiceError>;

    /// Queues an audition for its person and returns it, `queued`, or `ready` at once when every
    /// part was read before and is still kept (see `docs/architecture.md`, "Auditions"). It
    /// replaces that person's audition from before, if any. By default there is no model.
    async fn create_audition(&self, _new: NewAudition) -> Result<Audition, JobServiceError> {
        Err(JobServiceError::ModelNotReady)
    }

    /// One audition.
    async fn get_audition(&self, _id: &str) -> Result<Audition, JobServiceError> {
        Err(JobServiceError::AuditionNotFound)
    }

    /// The WAV of a ready audition, from memory.
    async fn audition_audio(&self, _id: &str) -> Result<axum::body::Bytes, JobServiceError> {
        Err(JobServiceError::AuditionNotFound)
    }

    /// Cancels an audition if it is queued or reading, and forgets it and its audio. Publishes
    /// `audition.deleted`.
    async fn delete_audition(&self, _id: &str) -> Result<(), JobServiceError> {
        Err(JobServiceError::AuditionNotFound)
    }

    /// Everything stored under `owner`, a member who was removed, goes: their audition and the
    /// speech kept for it are dropped, their queued jobs are
    /// cancelled, a job of theirs that is being read is stopped, and every job of theirs is deleted
    /// with its history entry, its segment audio and its joined WAV in the output folder. Nothing is
    /// published, because nobody is left to see it. A file that cannot be removed is logged and
    /// left; nobody can see it either.
    async fn remove_owner(&self, owner: &str);
}

/// What the queue calls when a person's voices changed by something it did: a voice was prepared,
/// lost its preparation, or was read. The server turns it into `voices.changed`.
pub type VoiceNews = Arc<dyn Fn(Option<String>) + Send + Sync>;

/// What the app tells the UI about the loaded models (`model.warm`, and the `warm` fields of
/// `GET /api/v1/models`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct WarmState {
    /// The ids of the loaded models, most recently used first.
    pub warm: Vec<String>,
    /// The id of the model being loaded now, if one is.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub warming: Option<String>,
    /// The id of the model being unloaded now to make room for another, if one is.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub unloading: Option<String>,
}

/// The job `id` when it is stored under `owner`, else `job_not_found`. Every route that takes a
/// job id asks this first.
pub async fn owned_job(
    jobs: &dyn JobService,
    id: &str,
    owner: Option<&str>,
) -> Result<Job, JobServiceError> {
    let job = jobs.get(id).await?;
    if job.owner.as_deref() == owner {
        Ok(job)
    } else {
        Err(JobServiceError::JobNotFound)
    }
}

/// The job `id` when it is stored under `owner`, with the path of its joined WAV. This is the one
/// file `GET /jobs/{id}/output.wav` serves and the MCP voiceover tool and resources read, and the
/// path comes from the queue, never from the caller. `job_not_found` for someone else's job,
/// `job_not_done` before it is done.
pub async fn owned_output(
    jobs: &dyn JobService,
    id: &str,
    owner: Option<&str>,
) -> Result<(Job, PathBuf), JobServiceError> {
    let job = owned_job(jobs, id, owner).await?;
    let path = jobs.output_path(id).await?;
    Ok((job, path))
}

/// The current time as RFC 3339, for `createdAt` and friends.
pub fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// A file name from a title: letters and digits are kept, runs of anything else become one `-`,
/// at most 60 characters. `voiceover` when nothing is left.
pub fn safe_file_stem(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        if out.chars().count() >= 60 {
            break;
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        "voiceover".into()
    } else {
        out.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job_of(segments: Vec<Segment>) -> Job {
        Job {
            id: "vov_aaaaaa".into(),
            title: "t".into(),
            voice: "v".into(),
            model: None,
            source: JobSource::Ui,
            state: JobState::Done,
            created_at: String::new(),
            started_at: None,
            finished_at: None,
            sentence_pause_ms: 250,
            paragraph_pause_ms: 600,
            segments,
            output: None,
            error: None,
            owner: None,
            made_by: String::new(),
            tags: Vec::new(),
            draft_of: None,
            has_draft: false,
            export_of: None,
        }
    }

    fn part(index: usize, text: &str, ends_paragraph: bool, seconds: Option<f32>) -> Segment {
        Segment {
            index,
            text: text.into(),
            ends_paragraph,
            pause_before_ms: 0,
            pause_after_ms: 0,
            state: if seconds.is_some() {
                SegmentState::Ready
            } else {
                SegmentState::Queued
            },
            seconds,
            start_seconds: None,
            end_seconds: None,
            error: None,
        }
    }

    #[test]
    fn file_stems_are_safe() {
        assert_eq!(safe_file_stem("Intro: part 1/2?"), "Intro-part-1-2");
        assert_eq!(safe_file_stem("  ..  "), "voiceover");
        assert_eq!(safe_file_stem(r"..\..\evil"), "evil");
        assert_eq!(safe_file_stem("CON"), "CON");
        assert!(safe_file_stem(&"a".repeat(200)).len() <= 60);
    }

    #[test]
    fn cues_run_through_the_parts_with_the_pauses_between_and_stop_at_the_first_unknown() {
        let mut job = job_of(vec![
            part(0, "One.", false, Some(1.0)),
            part(1, "Two.", true, Some(2.0)),
            part(2, "Three.", false, Some(1.5)),
            part(3, "Four.", false, None),
            part(4, "Five.", false, Some(1.0)),
        ]);
        job.fill_cues();
        let at = |i: usize| (job.segments[i].start_seconds, job.segments[i].end_seconds);
        assert_eq!(at(0), (Some(0.0), Some(1.0)));
        // A sentence pause of 250 ms follows the first part.
        assert_eq!(at(1), (Some(1.25), Some(3.25)));
        // The second part ends a paragraph: 600 ms.
        assert_eq!(at(2), (Some(3.85), Some(5.35)));
        // An unknown length ends the cues, also for the part after it.
        assert_eq!(at(3), (None, None));
        assert_eq!(at(4), (None, None));
    }

    #[test]
    fn a_jobs_script_has_its_markers_and_breaks_back() {
        let mut first = part(0, "A brace { here.", true, None);
        first.pause_before_ms = 2_000;
        first.pause_after_ms = 500;
        let job = job_of(vec![first, part(1, "Next.", false, None)]);
        assert_eq!(
            job.script(),
            "{pause 2000ms} A brace \\{ here. {pause 500ms}\n\nNext."
        );
    }

    #[test]
    fn a_job_saved_without_pause_settings_reads_with_the_defaults() {
        let json = r#"{"id":"vov_aaaaaa","title":"t","voice":"v","source":"ui","state":"done",
            "createdAt":"","segments":[]}"#;
        let job: Job = serde_json::from_str(json).unwrap();
        assert_eq!(job.sentence_pause_ms, DEFAULT_SENTENCE_PAUSE_MS);
        assert_eq!(job.paragraph_pause_ms, DEFAULT_PARAGRAPH_PAUSE_MS);
        assert!(job.tags.is_empty());
    }

    #[test]
    fn the_warm_state_goes_out_in_camel_case_and_leaves_out_what_is_not_happening() {
        let state = WarmState {
            warm: vec!["qwen3-tts".into()],
            warming: None,
            unloading: Some("other".into()),
        };
        assert_eq!(
            serde_json::to_string(&state).unwrap(),
            r#"{"warm":["qwen3-tts"],"unloading":"other"}"#
        );
    }

    #[test]
    fn an_rfc3339_time_has_a_t_and_a_zone() {
        let now = now_rfc3339();
        assert!(now.contains('T') && now.ends_with('Z'), "{now}");
    }
}
