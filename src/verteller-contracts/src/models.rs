//! What a Pro feature may know of a speech model: a plain [`ModelInfo`] and the rules that need
//! nothing else. The registry, the catalog and the files are the app's.

use crate::{ApiError, Feature};
use axum::http::StatusCode;
use std::collections::BTreeMap;

/// The longest piece of a script read in one go when a model names no limit of its own, in
/// characters.
pub const DEFAULT_SEGMENT_CHARS: usize = 300;

/// A speech model, as far as a Pro feature needs it. The app's registry has much more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    /// The id a voice stores.
    pub id: String,
    /// The name to show.
    pub name: String,
    /// The licence of the model's weights, as an SPDX id.
    pub licence: String,
    /// Whether the licence allows commercial use. A model that does not is blocked from every Pro
    /// feature and from MCP, and works in the free core only.
    pub commercial_use: bool,
    /// Whether it reads text aloud in a voice. A model that writes down speech or times words does
    /// not, and cannot be used for a voice.
    pub reads_aloud: bool,
    /// The longest piece of a script this model should read in one go, in characters. `None` is
    /// [`DEFAULT_SEGMENT_CHARS`].
    pub max_segment_chars: Option<u32>,
    /// The keys of the settings the two basic sliders (Expressiveness and Variation) move. Every
    /// other setting of the model is an advanced one.
    pub basic_settings: Vec<String>,
}

impl ModelInfo {
    /// The longest piece of a script to read in one go with this model, in characters.
    pub fn segment_chars(&self) -> usize {
        self.max_segment_chars
            .map_or(DEFAULT_SEGMENT_CHARS, |c| c as usize)
    }
}

/// Whether going from `current` to `new` sets an advanced setting: a key the basic sliders do not
/// move, added or changed. Taking one away (back to the model's default) is not: it only moves
/// toward the free state. A model that is not known has no basic sliders, so every key counts as
/// advanced.
pub fn sets_advanced(
    model: Option<&ModelInfo>,
    current: &BTreeMap<String, f64>,
    new: &BTreeMap<String, f64>,
) -> bool {
    new.iter().any(|(key, value)| {
        let basic = model.is_some_and(|m| m.basic_settings.iter().any(|b| b == key));
        !basic && current.get(key) != Some(value)
    })
}

/// Refuses a model whose licence limits commercial use (`403 model_not_commercial`) for a Pro
/// `feature`. `None`, a model that is not known, is not blocked here.
pub fn check_commercial(feature: Feature, model: Option<&ModelInfo>) -> Result<(), ApiError> {
    if let Some(info) = model
        && !info.commercial_use
    {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "model_not_commercial",
            format!(
                "The licence of {} ({}) limits commercial use, so it cannot be used with {}. It works in the free version of Verteller.",
                info.name,
                info.licence,
                feature.label()
            ),
        ));
    }
    Ok(())
}

/// Refuses an id the registry does not have (`400 unknown_model`, `info` is `None`) and a model
/// that writes down speech instead of reading text aloud (`400 not_a_voice_model`): a voice and a
/// voiceover are for the models that read aloud. `id` is the id that was asked for.
pub fn check_voice_model(id: &str, info: Option<&ModelInfo>) -> Result<(), ApiError> {
    let Some(info) = info else {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "unknown_model",
            format!("There is no speech model called {:?}.", crate::echo(id)),
        ));
    };
    if !info.reads_aloud {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "not_a_voice_model",
            format!(
                "{} writes down speech or times its words. It cannot be used for a voice or to read text aloud.",
                info.name
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(commercial: bool, reads: bool, basic: &[&str]) -> ModelInfo {
        ModelInfo {
            id: "m".into(),
            name: "Model M".into(),
            licence: "CC-BY-NC-4.0".into(),
            commercial_use: commercial,
            reads_aloud: reads,
            max_segment_chars: None,
            basic_settings: basic.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn map(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn a_slider_is_free_and_any_other_setting_is_advanced() {
        let m = info(true, true, &["temperature"]);
        let none = map(&[]);
        assert!(!sets_advanced(
            Some(&m),
            &none,
            &map(&[("temperature", 0.5)])
        ));
        assert!(sets_advanced(Some(&m), &none, &map(&[("top_k", 5.0)])));
        // A value that did not change is not a change, and taking one away is free.
        assert!(!sets_advanced(
            Some(&m),
            &map(&[("top_k", 5.0)]),
            &map(&[("top_k", 5.0)])
        ));
        assert!(!sets_advanced(Some(&m), &map(&[("top_k", 5.0)]), &none));
        // A model that is not known has no basic sliders.
        assert!(sets_advanced(None, &none, &map(&[("temperature", 0.5)])));
    }

    #[test]
    fn a_non_commercial_model_is_refused_for_a_pro_feature_by_name() {
        let e = check_commercial(Feature::Mcp, Some(&info(false, true, &[]))).unwrap_err();
        assert_eq!(
            (e.status, e.code),
            (StatusCode::FORBIDDEN, "model_not_commercial")
        );
        assert!(e.detail.contains("Model M") && e.detail.contains("CC-BY-NC-4.0"));
        assert!(check_commercial(Feature::Mcp, Some(&info(true, true, &[]))).is_ok());
        assert!(check_commercial(Feature::Mcp, None).is_ok());
    }

    #[test]
    fn only_a_known_model_that_reads_aloud_is_a_voice_model() {
        assert_eq!(
            check_voice_model("zzz", None).unwrap_err().code,
            "unknown_model"
        );
        assert_eq!(
            check_voice_model("m", Some(&info(true, false, &[])))
                .unwrap_err()
                .code,
            "not_a_voice_model"
        );
        assert!(check_voice_model("m", Some(&info(true, true, &[]))).is_ok());
    }

    #[test]
    fn a_model_without_a_limit_is_cut_at_the_default() {
        let mut m = info(true, true, &[]);
        assert_eq!(m.segment_chars(), DEFAULT_SEGMENT_CHARS);
        m.max_segment_chars = Some(120);
        assert_eq!(m.segment_chars(), 120);
    }
}
