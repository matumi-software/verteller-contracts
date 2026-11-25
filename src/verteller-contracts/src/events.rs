//! The live push as Pro sees it: the events, who they are for, and the three operations the Pro
//! features have on the app's event bus ([`EventSink`]).
//!
//! Every event gets an id from a counter that only goes up, so a client that reconnects with
//! `Last-Event-ID` gets what it missed. The bus that hands out the ids and keeps the replay buffer
//! is the app's. Every event also says who it is for, its [`Audience`]: everyone signed in (the
//! model, the downloads, the settings), the one person whose voices or jobs it is about, or the
//! owner on the host (invitation and reset codes). The stream route hands each person only what
//! reaches them, live and in a replay alike. Ids stay one sequence for everyone, so a person's
//! stream skips the ids that were for someone else.

use serde::Serialize;
use tokio::sync::broadcast;

/// The event names from the app's `docs/api.md`.
pub mod name {
    /// The body of `GET /api/v1/system`.
    pub const SYSTEM_CHANGED: &str = "system.changed";
    /// The body of `GET /api/v1/settings`.
    pub const SETTINGS_CHANGED: &str = "settings.changed";
    /// The body of `GET /api/v1/integrations/claude-desktop`.
    pub const INTEGRATIONS_CHANGED: &str = "integrations.changed";
    /// Which speech models are loaded, and which is loading.
    pub const MODEL_WARM: &str = "model.warm";
    /// `{ "voices": [Voice] }`.
    pub const VOICES_CHANGED: &str = "voices.changed";
    /// `{ "voiceDrafts": [Voice] }`: the person's unsaved recordings (never voices), whenever one changes
    /// or its preparation does. Not in the snapshot: a page asks `GET /voice-drafts`.
    pub const VOICE_DRAFTS_CHANGED: &str = "voice-drafts.changed";
    /// `{ "jobs": [Job] }`, the most recent 50.
    pub const JOBS_CHANGED: &str = "jobs.changed";
    /// `{ "cleanups": [CleanupJob] }`, the 20 most recent.
    pub const CLEANUPS_CHANGED: &str = "cleanups.changed";
    /// `[ModelEntry]`, the body of `GET /api/v1/models`, whenever the person's list of models changes
    /// (a model was added, or a catalog update changed what is vetted).
    pub const MODELS_CHANGED: &str = "models.changed";
    /// The body of `GET /api/v1/audiocpp`.
    pub const AUDIOCPP_STATE: &str = "audiocpp.state";
    /// `{ "file", "bytes", "total" }`.
    pub const DOWNLOAD_PROGRESS: &str = "download.progress";
    /// `{ "job": Job }`.
    pub const JOB_CREATED: &str = "job.created";
    /// `{ "job": Job }`.
    pub const JOB_STATE: &str = "job.state";
    /// `{ "jobId", "index" }`.
    pub const SEGMENT_STARTED: &str = "segment.started";
    /// `{ "jobId", "index", "seconds", "url" }`.
    pub const SEGMENT_READY: &str = "segment.ready";
    /// `{ "job": Job }`.
    pub const JOB_DONE: &str = "job.done";
    /// `{ "job": Job }`, with `job.error` set.
    pub const JOB_FAILED: &str = "job.failed";
    /// `{ "job": Job }`.
    pub const JOB_CANCELLED: &str = "job.cancelled";
    /// `{ "audition": Audition }`, whenever an audition is made or its state changes.
    pub const AUDITION_STATE: &str = "audition.state";
    /// `{ "auditionId" }`, once an audition is cancelled, replaced or expired and its audio is gone.
    pub const AUDITION_DELETED: &str = "audition.deleted";
    /// `{ "projects": [ProjectSummary] }`, the editor's projects of one person (Pro). Not in the
    /// snapshot: the editor's pages ask `GET /api/v1/projects`.
    pub const PROJECTS_CHANGED: &str = "projects.changed";
    /// `{ "project": Project }`, a project of the editor changed (Pro).
    pub const PROJECT_CHANGED: &str = "project.changed";
    /// `{ "projectId", "state", "done", "total" }`, the progress of a render (Pro).
    pub const PROJECT_RENDER: &str = "project.render";
    /// `{ "projectId" }`, once a project and its files are gone (Pro).
    pub const PROJECT_DELETED: &str = "project.deleted";
    /// `{ "jobId" }`, once a finished job and its files are gone.
    pub const JOB_DELETED: &str = "job.deleted";
    /// `PersonDevices`: the person this stream belongs to and their devices.
    pub const ACCOUNT_CHANGED: &str = "account.changed";
    /// `{ "people": [PersonDevices] }`, to the owner.
    pub const PEOPLE_CHANGED: &str = "people.changed";
    /// `{ "invitation": Invitation | null }`, to the owner on the host.
    pub const INVITATION_CHANGED: &str = "invitation.changed";
    /// `{ "resets": [PasswordReset] }`, to the owner on the host.
    pub const RESETS_CHANGED: &str = "resets.changed";

    /// The order a new stream opens with. `download.progress` and the job deltas are not state, so
    /// they are not here. The account events follow these, the ones the person may see.
    pub const SNAPSHOT_ORDER: [&str; 7] = [
        SYSTEM_CHANGED,
        SETTINGS_CHANGED,
        INTEGRATIONS_CHANGED,
        AUDIOCPP_STATE,
        VOICES_CHANGED,
        JOBS_CHANGED,
        CLEANUPS_CHANGED,
    ];
}

/// Who an event is for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Audience {
    /// Everyone signed in: what is app-wide.
    #[default]
    Everyone,
    /// The person whose data it is, by the owner key the data carries: `None` is the app's owner,
    /// `Some(id)` a member.
    Owner(Option<String>),
    /// The owner, and only on the host: the invitation and reset codes.
    OwnerOnHost,
}

/// Who is reading a stream, as far as [`Audience`] cares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewer {
    /// The owner key of their data: `None` for the owner, their id for a member.
    pub data_owner: Option<String>,
    /// The stream was opened on the host.
    pub on_host: bool,
}

impl Viewer {
    /// The app's owner on the host, as the bearer token is.
    pub fn owner_on_host() -> Self {
        Self {
            data_owner: None,
            on_host: true,
        }
    }
}

impl Audience {
    /// Whether `viewer` gets an event for this audience.
    pub fn reaches(&self, viewer: &Viewer) -> bool {
        match self {
            Self::Everyone => true,
            Self::Owner(owner) => *owner == viewer.data_owner,
            Self::OwnerOnHost => viewer.data_owner.is_none() && viewer.on_host,
        }
    }
}

/// One event: an id, a name and one line of JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Goes up by one for every published event, starting at 1.
    pub id: u64,
    /// One of the names in [`name`].
    pub name: String,
    /// One line of JSON.
    pub data: String,
    /// Who it is for.
    pub audience: Audience,
}

/// What a new stream starts from, taken in one step so no event falls between replay and live.
pub struct Subscription {
    /// True when the client has to be given the current state instead of a replay: it sent no id,
    /// or an id the buffer no longer reaches, or one from the future.
    pub needs_snapshot: bool,
    /// The events the client missed, oldest first. Empty when `needs_snapshot` is set.
    pub backlog: Vec<Event>,
    /// The id of the newest event published when the subscription was made. A snapshot is stamped
    /// with it, so a client that reconnects straight after resumes from here.
    pub cursor: u64,
    /// Events published after `cursor`.
    pub live: broadcast::Receiver<Event>,
}

/// The event bus, as far as a Pro feature may use it: publish for everyone, publish for one
/// audience, and start a stream.
///
/// The bodies are one line of JSON as text, so the trait stays usable as `dyn EventSink`.
/// [`EventSinkExt`] turns any value that serializes into that text.
pub trait EventSink: Send + Sync {
    /// Publishes an event for `audience` and returns it with its id. `data` is one line of JSON.
    fn publish_to(&self, audience: Audience, name: &str, data: String) -> Event;

    /// Publishes an event for everyone and returns it with its id.
    fn publish(&self, name: &str, data: String) -> Event {
        self.publish_to(Audience::Everyone, name, data)
    }

    /// Starts a stream at `last_event_id`, the client's `Last-Event-ID` when it sent one.
    fn subscribe(&self, last_event_id: Option<u64>) -> Subscription;
}

/// The body of an event: `data` as one line of JSON, `null` when it cannot be written.
pub fn json_line<T: Serialize + ?Sized>(data: &T) -> String {
    serde_json::to_string(data).unwrap_or_else(|_| "null".to_string())
}

/// [`EventSink`] for any value that serializes, on a `dyn EventSink` too.
pub trait EventSinkExt: EventSink {
    /// [`EventSink::publish`] with a value written as JSON.
    fn publish_json<T: Serialize + ?Sized>(&self, name: &str, data: &T) -> Event {
        self.publish(name, json_line(data))
    }

    /// [`EventSink::publish_to`] with a value written as JSON.
    fn publish_json_to<T: Serialize + ?Sized>(
        &self,
        audience: Audience,
        name: &str,
        data: &T,
    ) -> Event {
        self.publish_to(audience, name, json_line(data))
    }
}

impl<S: EventSink + ?Sized> EventSinkExt for S {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_audience_reaches_only_its_people() {
        let owner = Viewer {
            data_owner: None,
            on_host: false,
        };
        let owner_here = Viewer::owner_on_host();
        let sam = Viewer {
            data_owner: Some("sam".into()),
            on_host: false,
        };
        for v in [&owner, &owner_here, &sam] {
            assert!(Audience::Everyone.reaches(v));
        }
        assert!(Audience::Owner(None).reaches(&owner));
        assert!(!Audience::Owner(None).reaches(&sam));
        assert!(Audience::Owner(Some("sam".into())).reaches(&sam));
        assert!(!Audience::Owner(Some("sam".into())).reaches(&owner));
        assert!(!Audience::Owner(Some("kim".into())).reaches(&sam));
        assert!(Audience::OwnerOnHost.reaches(&owner_here));
        assert!(!Audience::OwnerOnHost.reaches(&owner));
        let sam_here = Viewer {
            data_owner: Some("sam".into()),
            on_host: true,
        };
        assert!(!Audience::OwnerOnHost.reaches(&sam_here));
    }

    #[test]
    fn a_value_is_written_as_one_line_of_json() {
        assert_eq!(
            json_line(&serde_json::json!({ "a": [1, 2] })),
            r#"{"a":[1,2]}"#
        );
        // An f32 keeps the digits it prints with, not the ones of its f64 widening.
        assert_eq!(json_line(&0.1f32), "0.1");
    }
}
