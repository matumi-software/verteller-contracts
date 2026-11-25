//! Ids for the things people make and other things link to.
//!
//! An id is a three-letter prefix for the kind of item, an underscore, and six lowercase hex
//! characters taken from a GUID: `vce_3fa9c2` is a voice, `vov_b81e07` a voiceover. The id never
//! changes, so a name can be changed without breaking a link. The prefix says what a link points
//! at, so a voice's id is never mistaken for a voiceover's. Uniqueness is checked within a kind, by
//! the caller, which knows what is taken.
//!
//! The app uses only this form for a voice. A voice stored under another name has to be migrated before the
//! app lists or loads it. [`kind_of`] reads the kind of an id and refuses anything else.

use uuid::Uuid;

/// A voice.
pub const VOICE: &str = "vce";
/// A voiceover (a job).
pub const VOICEOVER: &str = "vov";
/// A recording kept only to hear it before it is saved as a voice. It is not a voice.
pub const VOICE_DRAFT: &str = "drf";
/// A person (the owner or a member).
pub const PERSON: &str = "usr";
/// A project of the Pro editor. The things inside a project (`src`, `trk`, `clp`, `tke`) are
/// unique inside it only, and are made by the editor.
pub const PROJECT: &str = "prj";

/// The hex characters after the prefix.
pub const SUFFIX_CHARS: usize = 6;

/// A new id for `prefix` that `taken` says is free.
pub fn new_id(prefix: &str, mut taken: impl FnMut(&str) -> bool) -> String {
    loop {
        let guid = Uuid::new_v4().simple().to_string();
        let id = format!("{prefix}_{}", &guid[..SUFFIX_CHARS]);
        if !taken(&id) {
            return id;
        }
    }
}

/// The prefix of `id` when it is a new-style id (`xxx_` and six lowercase hex characters).
pub fn kind_of(id: &str) -> Option<&str> {
    let (prefix, suffix) = id.split_once('_')?;
    let prefix_ok = prefix.len() == 3 && prefix.bytes().all(|b| b.is_ascii_lowercase());
    let suffix_ok = suffix.len() == SUFFIX_CHARS
        && suffix
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    (prefix_ok && suffix_ok).then_some(prefix)
}

/// Whether `id` is a new-style id of the kind `prefix`.
pub fn is_kind(id: &str, prefix: &str) -> bool {
    kind_of(id) == Some(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_the_kind_an_underscore_and_six_lowercase_hex_characters() {
        for _ in 0..200 {
            let id = new_id(VOICE, |_| false);
            assert_eq!(id.len(), 10, "{id}");
            assert!(is_kind(&id, VOICE), "{id}");
            assert!(!is_kind(&id, VOICEOVER));
        }
    }

    #[test]
    fn a_taken_id_is_not_given_again() {
        let mut seen = std::collections::HashSet::new();
        // A tight space is tried again and again until a free one turns up.
        let id = new_id(PERSON, |candidate| {
            let first = seen.is_empty();
            seen.insert(candidate.to_string());
            first
        });
        assert!(is_kind(&id, PERSON));
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn old_ids_are_not_new_style_ids() {
        for old in [
            "office-mic",
            "my_voice",
            "6f9619ff-8b86-d011-b42d-00c04fc964ff",
            "vce_3FA9C2",
            "vce_3fa9c",
            "vce_3fa9c2a",
            "VCE_3fa9c2",
            "vc_3fa9c2",
            "",
        ] {
            assert_eq!(kind_of(old), None, "{old}");
        }
        assert_eq!(kind_of("vov_b81e07"), Some("vov"));
    }
}
