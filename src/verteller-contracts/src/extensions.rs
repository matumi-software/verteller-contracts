//! The hooks the private Pro crate plugs into, and the [`Addons`] a build gives the app.
//!
//! The open repo never depends on the Pro crate. An official build gives the app an
//! [`Edition`] answered from a licence and an [`Extensions`] with the Pro pieces
//! filled in. A build from source gives neither, so [`Extensions::none`] and the free edition
//! stand, and every Pro feature answers `402 needs_pro` ([`Host::require`]).
//!
//! No hook names a type of the app. A route hook gets the [`Host`] and answers a router that has
//! its own state applied (`Router<()>`, from `with_state`). The app merges it into its router
//! before the guards, so the guards still wrap every Pro route and `Extension<Caller>` still
//! comes from the app's guard layer.
//!
//! Each hook is one trait. A feature's own issue adds the methods it needs.

use crate::{Edition, host::Host, jobs::Job};
use std::sync::Arc;

/// A boxed future, for the methods of an extension that are asynchronous.
pub type BoxFuture<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>;

/// What a build gives the app: the edition and the Pro pieces, worked out from the data folder
/// (where an official build keeps the licence).
pub struct Addons {
    /// What the person may do.
    pub edition: Edition,
    /// The Pro pieces present.
    pub extensions: Extensions,
}

/// Long-form voiceovers: how long a piece each model really reads in one go, measured on the model.
///
/// The open core already cuts a script into pieces, reads them and joins them with their pauses,
/// using each model's `max_segment_chars` from the catalog (or the default). A Pro build adds the
/// measuring: the first long voiceover on a model runs [`LongFormExtension::measure`] once, the
/// extension remembers the answer, and every later voiceover on that model is cut by it.
pub trait LongFormExtension: Send + Sync {
    /// A short name for the logs.
    fn name(&self) -> &'static str;

    /// The longest piece of a script, in characters, that was measured for `model`, or `None` when
    /// it has not been measured (or could not be). Cheap: called for every new job.
    fn measured_limit(&self, model: &str) -> Option<usize>;

    /// Measures `model` and remembers the answer, so [`LongFormExtension::measured_limit`] gives it
    /// from then on. Runs on the model thread with the model loaded, once, before the first
    /// voiceover on a model that has no answer and is long enough to be cut. It reads short test
    /// passages with `voice`; nothing is saved. Check `cancel` between reads. `Err(Cancelled)`
    /// ends the voiceover, any other error is logged and the voiceover goes on with the catalog's
    /// length.
    fn measure(
        &self,
        model: &str,
        voice: &crate::VoiceProfile,
        synth: &mut dyn crate::Synthesizer,
        cancel: &crate::CancelToken,
    ) -> crate::Result<usize>;
}

/// Subtitles for a voiceover. The writer of the SRT file is Pro's: the open program serves the
/// route, the guards and the file name, and asks this for the text, so a build from source has no
/// subtitle code to switch on.
pub trait SubtitlesExtension: Send + Sync {
    /// A short name for the logs.
    fn name(&self) -> &'static str;

    /// The cues of `job` as SubRip text, or `None` when the job is not done. A cue's start and end
    /// are the part's `start_seconds` and `end_seconds`, the times in the saved file.
    fn srt(&self, job: &Job) -> Option<String>;
}

/// The per-model settings drawn from the model's schema. A marker with no methods and nothing
/// fills it: the settings routes ask the edition (`Feature::AdvancedVoice`), so a licence switch
/// is all the Pro build has to give. Kept only until the owner decides whether to remove it
/// (verteller#340).
pub trait AdvancedVoiceExtension: Send + Sync {
    /// A short name for the logs.
    fn name(&self) -> &'static str;
}

/// The MCP server, the `verteller mcp` stdio bridge and the Claude Desktop connection. MCP is a Pro
/// feature, so these come from the Pro build. When one is present the app uses it in place of
/// anything of its own.
pub trait McpExtension: Send + Sync {
    /// A short name for the logs.
    fn name(&self) -> &'static str;

    /// The routes it adds: the MCP server at `/mcp` and the Claude Desktop connection under
    /// `/api/v1/integrations/claude-desktop`. They are merged into the app's router before the
    /// guards, so they are guarded like every other route (the host check, the sign-in, the UI
    /// header on a change). `host` is what the app gives the feature, for a service that needs it
    /// to be built. The router has its state applied.
    fn routes(&self, host: Arc<dyn Host>) -> axum::Router<()>;

    /// The body of `integrations.changed` for a new event stream (the Claude Desktop connection's
    /// status), or `None` when there is nothing to say.
    fn integrations(&self) -> BoxFuture<Option<serde_json::Value>>;

    /// Runs `verteller mcp`: the stdio bridge to the running app. `locator` finds the app, and
    /// starts it when it is not running; the bridge asks it again when the app goes away. This runs
    /// as a process of its own before any server exists, so it has no [`Host`]. Answers the
    /// program's exit code.
    fn bridge(
        &self,
        locator: Arc<dyn crate::bridge::EndpointLocator>,
    ) -> BoxFuture<std::process::ExitCode>;
}

/// The voiceover editor and its library of projects. The editor is a Pro feature, so the project
/// store, the edit commands, the render engine and the routes come from the Pro build; the open
/// program holds this hook and the voiceover side (the Library's export, see
/// [`JobService::add_export`](crate::jobs::JobService::add_export)).
pub trait ProjectsExtension: Send + Sync {
    /// A short name for the logs.
    fn name(&self) -> &'static str;

    /// The routes it adds, under `/api/v1/projects`. They are merged into the app's router before
    /// the guards, so they are guarded like every other route. Each route asks
    /// `Host::require(Feature::Editor)` first. The router has its state applied.
    fn routes(&self, host: Arc<dyn Host>) -> axum::Router<()>;
}

/// Voice export and import as a password protected package file (#305). The format, the
/// encryption and the routes are Pro's: the open program has no reader and no writer of a voice
/// package, so a free build cannot move a voice in or out.
pub trait PackagesExtension: Send + Sync {
    /// A short name for the logs.
    fn name(&self) -> &'static str;

    /// The routes it adds: `POST /api/v1/voices/export`, `POST /api/v1/voices/import/open` and
    /// `POST /api/v1/voices/import/confirm`. They are merged into the app's router before the
    /// guards. Each asks `Host::require(Feature::Packages)` first, checks that the voice is the
    /// caller's own ([`Host::load_voice`]) and never moves a voice without its consent record and
    /// a password. The router has its state applied.
    fn routes(&self, host: Arc<dyn Host>) -> axum::Router<()>;
}

/// The Pro pieces that are present in this build. Cheap to clone.
#[derive(Clone, Default)]
pub struct Extensions {
    pub long_form: Option<Arc<dyn LongFormExtension>>,
    pub subtitles: Option<Arc<dyn SubtitlesExtension>>,
    pub advanced_voice: Option<Arc<dyn AdvancedVoiceExtension>>,
    pub mcp: Option<Arc<dyn McpExtension>>,
    pub projects: Option<Arc<dyn ProjectsExtension>>,
    pub packages: Option<Arc<dyn PackagesExtension>>,
}

impl Extensions {
    /// A build with no Pro crate.
    pub fn none() -> Self {
        Self::default()
    }

    /// Whether any Pro piece is present.
    pub fn any(&self) -> bool {
        self.long_form.is_some()
            || self.subtitles.is_some()
            || self.advanced_voice.is_some()
            || self.mcp.is_some()
            || self.projects.is_some()
            || self.packages.is_some()
    }
}

impl std::fmt::Debug for Extensions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Extensions")
            .field("long_form", &self.long_form.is_some())
            .field("subtitles", &self.subtitles.is_some())
            .field("advanced_voice", &self.advanced_voice.is_some())
            .field("mcp", &self.mcp.is_some())
            .field("projects", &self.projects.is_some())
            .field("packages", &self.packages.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Subs;
    impl SubtitlesExtension for Subs {
        fn name(&self) -> &'static str {
            "subs"
        }
        fn srt(&self, _job: &Job) -> Option<String> {
            None
        }
    }

    #[test]
    fn needs_pro_is_a_402_that_names_the_feature() {
        let e = crate::ApiError::needs_pro(crate::Feature::Subtitles);
        assert_eq!(e.status, axum::http::StatusCode::PAYMENT_REQUIRED);
        assert_eq!(e.code, "needs_pro");
        assert!(e.detail.contains("Subtitles"), "{}", e.detail);
    }

    #[test]
    fn a_build_with_no_pro_crate_has_no_extension() {
        let e = Extensions::none();
        assert!(!e.any());
        assert!(e.subtitles.is_none() && e.mcp.is_none());
    }

    struct Editor;
    impl ProjectsExtension for Editor {
        fn name(&self) -> &'static str {
            "editor"
        }
        fn routes(&self, _: Arc<dyn Host>) -> axum::Router<()> {
            axum::Router::new()
        }
    }

    #[test]
    fn the_editor_hook_counts_as_a_pro_piece() {
        let e = Extensions {
            projects: Some(Arc::new(Editor)),
            ..Extensions::none()
        };
        assert!(e.any());
        assert_eq!(e.projects.as_ref().unwrap().name(), "editor");
        assert!(Extensions::none().projects.is_none());
    }

    struct Packages;
    impl PackagesExtension for Packages {
        fn name(&self) -> &'static str {
            "packages"
        }
        fn routes(&self, _: Arc<dyn Host>) -> axum::Router<()> {
            axum::Router::new()
        }
    }

    #[test]
    fn the_packages_hook_counts_as_a_pro_piece_and_a_free_build_has_none() {
        let e = Extensions {
            packages: Some(Arc::new(Packages)),
            ..Extensions::none()
        };
        assert!(e.any());
        assert_eq!(e.packages.as_ref().unwrap().name(), "packages");
        assert!(Extensions::none().packages.is_none());
    }

    #[test]
    fn one_registered_hook_is_found_and_the_rest_are_not() {
        let e = Extensions {
            subtitles: Some(Arc::new(Subs)),
            ..Extensions::none()
        };
        assert!(e.any());
        assert_eq!(e.subtitles.as_ref().unwrap().name(), "subs");
        assert!(e.long_form.is_none());
    }
}
