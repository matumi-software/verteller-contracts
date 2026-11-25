//! What this build lets the person do: the free core, or more with a licence.
//!
//! The open repo cannot depend on the private Pro crate, so it asks three questions of an
//! [`Entitlements`] value and carries its own [`FreeEntitlements`]. A build with the private crate
//! gives the app an `Entitlements` that answers from a signed licence. A build from source has
//! none, so it runs on the free answers. The names are in [`name`], in one place.
//!
//! [`Edition`] is the typed view the rest of the code uses, so no caller handles a string.

use std::sync::Arc;

use serde::Serialize;

/// The names the app asks about. The private crate grants them by these names.
pub mod name {
    /// A limit: the longest single voiceover segment, in seconds. Free has one; Pro has none unless
    /// the licence grants one.
    pub const MAX_SEGMENT_SECONDS: &str = "max_segment_seconds";
    /// A switch: model capability detection, splitting a long script and joining the pieces.
    pub const LONG_FORM: &str = "long_form";
    /// A switch: the per-model settings drawn from the model's schema.
    pub const ADVANCED_VOICE: &str = "advanced_voice";
    /// A switch: subtitles for a voiceover.
    pub const SUBTITLES: &str = "subtitles";
    /// A switch: the MCP server and the Claude Desktop connection.
    pub const MCP: &str = "mcp";
    /// A switch: the voiceover editor, with its own library of projects.
    pub const EDITOR: &str = "editor";
    /// A switch: voice export and import as a protected package file.
    pub const PACKAGES: &str = "packages";
}

/// The longest voiceover segment a free build takes, in seconds, for every model.
pub const FREE_MAX_SEGMENT_SECONDS: u64 = 20;

/// Free or Pro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Free,
    Pro,
}

/// What the person may do, asked by name. The same three questions the private licensing crate
/// answers, so it can stand behind this trait without the open repo knowing it exists.
pub trait Entitlements: Send + Sync {
    fn tier(&self) -> Tier;

    /// Whether a feature is on.
    fn is_granted(&self, name: &str) -> bool;

    /// A numeric limit. `None` means there is no limit.
    fn limit(&self, name: &str) -> Option<u64>;
}

/// The free build: the free core, a 20 second segment, and every Pro switch off.
#[derive(Debug, Clone, Copy, Default)]
pub struct FreeEntitlements;

impl Entitlements for FreeEntitlements {
    fn tier(&self) -> Tier {
        Tier::Free
    }

    fn is_granted(&self, _name: &str) -> bool {
        false
    }

    fn limit(&self, name: &str) -> Option<u64> {
        (name == name::MAX_SEGMENT_SECONDS).then_some(FREE_MAX_SEGMENT_SECONDS)
    }
}

/// A Pro feature, for a "needs Pro" answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    LongForm,
    AdvancedVoice,
    Subtitles,
    Mcp,
    Editor,
    Packages,
}

impl Feature {
    /// The entitlement name the feature is granted by.
    pub fn entitlement(self) -> &'static str {
        match self {
            Self::LongForm => name::LONG_FORM,
            Self::AdvancedVoice => name::ADVANCED_VOICE,
            Self::Subtitles => name::SUBTITLES,
            Self::Mcp => name::MCP,
            Self::Editor => name::EDITOR,
            Self::Packages => name::PACKAGES,
        }
    }

    /// The feature as a person reads it.
    pub fn label(self) -> &'static str {
        match self {
            Self::LongForm => "Long voiceovers",
            Self::AdvancedVoice => "Advanced voice settings",
            Self::Subtitles => "Subtitles",
            Self::Mcp => "Claude Desktop and MCP",
            Self::Editor => "The voiceover editor",
            Self::Packages => "Voice export and import",
        }
    }
}

/// The typed view of the entitlements. Cheap to clone.
#[derive(Clone)]
pub struct Edition {
    entitlements: Arc<dyn Entitlements>,
}

impl Default for Edition {
    fn default() -> Self {
        Self::free()
    }
}

impl std::fmt::Debug for Edition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Edition")
            .field("tier", &self.tier())
            .finish()
    }
}

impl Edition {
    /// The free build.
    pub fn free() -> Self {
        Self::new(Arc::new(FreeEntitlements))
    }

    /// An edition answered by `entitlements`, which a build with the Pro crate supplies.
    pub fn new(entitlements: Arc<dyn Entitlements>) -> Self {
        Self { entitlements }
    }

    pub fn tier(&self) -> Tier {
        self.entitlements.tier()
    }

    /// Whether `feature` is on.
    pub fn allows(&self, feature: Feature) -> bool {
        self.entitlements.is_granted(feature.entitlement())
    }

    /// The longest single segment of a voiceover, in seconds, or `None` for no limit.
    pub fn max_segment_seconds(&self) -> Option<u64> {
        self.entitlements.limit(name::MAX_SEGMENT_SECONDS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_has_a_twenty_second_segment_and_no_pro_feature() {
        let e = Edition::free();
        assert_eq!(e.tier(), Tier::Free);
        assert_eq!(e.max_segment_seconds(), Some(20));
        for f in [
            Feature::LongForm,
            Feature::AdvancedVoice,
            Feature::Subtitles,
            Feature::Mcp,
            Feature::Editor,
            Feature::Packages,
        ] {
            assert!(!e.allows(f), "{f:?}");
        }
    }

    #[test]
    fn the_default_edition_is_free() {
        assert_eq!(Edition::default().tier(), Tier::Free);
    }

    /// What a build with the Pro crate gives: every switch on, no segment limit.
    struct All;
    impl Entitlements for All {
        fn tier(&self) -> Tier {
            Tier::Pro
        }
        fn is_granted(&self, _: &str) -> bool {
            true
        }
        fn limit(&self, _: &str) -> Option<u64> {
            None
        }
    }

    #[test]
    fn an_edition_answers_from_the_entitlements_it_is_given() {
        let e = Edition::new(Arc::new(All));
        assert_eq!(e.tier(), Tier::Pro);
        assert_eq!(e.max_segment_seconds(), None);
        assert!(e.allows(Feature::LongForm));
        assert!(e.allows(Feature::Mcp));
    }

    #[test]
    fn each_feature_has_its_own_entitlement_name() {
        let names: Vec<_> = [
            Feature::LongForm,
            Feature::AdvancedVoice,
            Feature::Subtitles,
            Feature::Mcp,
            Feature::Editor,
            Feature::Packages,
        ]
        .iter()
        .map(|f| f.entitlement())
        .collect();
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len());
    }
}
