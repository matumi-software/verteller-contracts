//! A voice as plain data: whose it is, the consent beside it, and the settings the person chose.
//!
//! [`VoiceProfile`] is the record kept in a voice's `profile.json`, with the methods that need
//! nothing but the record and its folder. Where voices are stored, listed and changed is the
//! app's: this crate has no store.
//!
//! A voice is a folder named after the voice:
//!
//! ```text
//! <folder>/profile.json    the display name, the transcript and the consent record
//! <folder>/reference.wav   the recording the voice is cloned from, as it was made
//! <folder>/clean.wav       the cleaned copy, once a clean-up has been applied
//! ```
//!
//! The reference is never changed. A clean-up writes `clean.wav` from it and stores the on or off
//! choice in `profile.json`. [`VoiceProfile::voice_path`] is the file a speech model uses: the
//! cleaned copy while cleaning is on, the reference otherwise.

use crate::{Audio, DEFAULT_MODEL, Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Which noise reduction made a voice's cleaned copy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    /// RNNoise. Small and fast on a CPU, made for speech.
    #[default]
    Rnnoise,
    /// DeepFilterNet 3, a stronger noise remover that runs as a shipped program.
    Deepfilternet,
}

impl Engine {
    /// True when the engine is linked into the app's library, which is when it needs no program of
    /// its own.
    pub fn runs_in_process(self) -> bool {
        matches!(self, Engine::Rnnoise)
    }
}

/// Who the voice belongs to and that they agreed to it being cloned.
///
/// The library does not judge the content. A store refuses a profile with an empty field
/// ([`Consent::check`]), so every voice has a name and a statement beside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Consent {
    /// Whose voice it is.
    pub speaker: String,
    /// What they agreed to, in their words or the app's.
    pub statement: String,
    /// When the recording was made, as the caller formats it (for example `2026-11-13T09:30:00Z`).
    pub recorded_at: String,
}

impl Consent {
    /// Whether every field of the record has something in it.
    pub fn check(&self) -> Result<()> {
        if self.speaker.trim().is_empty() {
            return Err(Error::Consent("speaker"));
        }
        if self.statement.trim().is_empty() {
            return Err(Error::Consent("statement"));
        }
        if self.recorded_at.trim().is_empty() {
            return Err(Error::Consent("recorded_at"));
        }
        Ok(())
    }
}

/// The longest display name, in characters.
pub const MAX_DISPLAY_NAME_CHARS: usize = 80;

/// The most characters a transcript takes: a few minutes of reading.
pub const MAX_TRANSCRIPT_CHARS: usize = 6000;

/// The most words a voice's list takes.
pub const MAX_WORDS: usize = 200;

/// The most characters either side of a word on the list takes.
pub const MAX_WORD_CHARS: usize = 64;

/// One entry of a voice's words list: a word the model gets wrong, and how to write it so that it
/// reads it right.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Word {
    /// The word as the script has it. Matched whole and without regard to case.
    pub from: String,
    /// What the model is given instead.
    pub say: String,
}

/// The clean-up of a voice: whether the voice is read from the cleaned copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VoiceCleanup {
    /// The voice is read from the cleaned copy. Turning this off goes back to the reference and
    /// keeps the copy.
    pub enabled: bool,
    /// The engine that made the cleaned copy. Voices cleaned before there was a choice say
    /// RNNoise, which is what made them.
    #[serde(default)]
    pub engine: Engine,
}

/// A stored voice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceProfile {
    /// The profile's name, also its folder name. It is an id: it never changes and only holds
    /// letters, digits, `-` and `_`.
    pub name: String,
    /// What the person calls the voice, for example `Office mic`. Voices stored before this
    /// existed have none and show their name instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// What the reference recording says, when it is known. Some models do better with it.
    pub transcript: Option<String>,
    /// Whose voice it is and that they agreed.
    pub consent: Consent,
    /// The clean-up, once one was applied. Voices stored before clean-up existed have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup: Option<VoiceCleanup>,
    /// Who the voice belongs to, as an id the app gives a person. The library only stores it.
    /// Voices stored before there were people have none, and so does any voice of the person who
    /// owns the install, so `None` reads as that person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// The speech model this voice is made for, by its id in the app's model registry. Voiceovers in this
    /// voice use it. Voices stored before there was a choice have none stored and read as
    /// [`DEFAULT_MODEL`].
    #[serde(default = "default_model")]
    pub model: String,
    /// What the person changed from the model's defaults, by the keys of the model's settings in
    /// the app's model registry, so a setting left alone keeps following the model. Empty for a voice that
    /// has none stored.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub settings: std::collections::BTreeMap<String, f64>,
    /// How much of the recording the model uses, in seconds from its start, or the whole of it when
    /// there is none. A recording shorter than this is used whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_seconds: Option<f32>,
    /// Words the model should be given written another way, applied to the script right before it is
    /// read ([`VoiceProfile::spoken_text`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<Word>,
    /// When a voiceover or an audition last started reading in this voice, as RFC 3339. Left out
    /// for a voice that was never used, and for voices stored before this existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
    /// The profile's folder. Set when the profile is loaded or created.
    #[serde(skip)]
    pub dir: PathBuf,
}

fn default_model() -> String {
    DEFAULT_MODEL.to_string()
}

impl VoiceProfile {
    /// The name to show a person: the display name, or the id when there is none.
    pub fn shown_name(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.name)
    }

    /// The script text as the model is given it: the words list applied, each entry matched whole
    /// and without regard to case, the longest first. Nothing is copied when no entry matches.
    pub fn spoken_text<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        if self.words.is_empty() {
            return std::borrow::Cow::Borrowed(text);
        }
        let mut words: Vec<&Word> = self.words.iter().collect();
        words.sort_by_key(|w| std::cmp::Reverse(w.from.chars().count()));
        let mut out = String::with_capacity(text.len());
        let (mut changed, mut after_letter, mut i) = (false, false, 0);
        while i < text.len() {
            let rest = &text[i..];
            let hit = if after_letter {
                None
            } else {
                words.iter().find_map(|w| {
                    let len = match_ignoring_case(rest, &w.from)?;
                    let next = rest[len..].chars().next();
                    next.is_none_or(|c| !c.is_alphanumeric())
                        .then_some((len, w.say.as_str()))
                })
            };
            if let Some((len, say)) = hit {
                out.push_str(say);
                i += len;
                changed = true;
                after_letter = false;
            } else {
                let c = rest.chars().next().expect("not at the end");
                out.push(c);
                i += c.len_utf8();
                after_letter = c.is_alphanumeric();
            }
        }
        if changed {
            std::borrow::Cow::Owned(out)
        } else {
            std::borrow::Cow::Borrowed(text)
        }
    }

    /// Records that a reading started in this voice at `at` (RFC 3339). It rereads the stored
    /// profile and changes only this field, so a setting changed since this copy was loaded is not
    /// written back over. Fails when the voice is gone.
    pub fn mark_used(&self, at: &str) -> Result<()> {
        let mut stored = read_profile(&self.dir)?;
        stored.last_used_at = Some(at.to_string());
        write_profile(&stored)
    }

    /// Whether the model reads only the start of the recording (the sample length is shorter than
    /// the recording). Then the transcript, which is of the whole recording, no longer matches
    /// what the model hears.
    pub fn sample_is_trimmed(&self) -> bool {
        let Some(seconds) = self.sample_seconds else {
            return false;
        };
        hound::WavReader::open(self.voice_path())
            .map(|r| r.duration() as f32 / r.spec().sample_rate.max(1) as f32 > seconds)
            .unwrap_or(false)
    }

    /// The transcript to give a model that clones better with the text of the recording, or
    /// `None` when there is none or it no longer matches the audio the model reads.
    pub fn transcript_for_model(&self) -> Option<&str> {
        self.transcript
            .as_deref()
            .filter(|t| !t.trim().is_empty() && !self.sample_is_trimmed())
    }

    /// The file a speech model reads for this voice: [`VoiceProfile::voice_path`], or when the
    /// voice uses only part of its recording, a copy of the first seconds of it
    /// (`sample-<ms>ms-<source>.wav` in the voice folder), made when it is first needed and again
    /// when the recording it comes from is newer.
    pub fn sample_file(&self) -> Result<PathBuf> {
        let source = self.voice_path();
        let Some(seconds) = self.sample_seconds else {
            return Ok(source);
        };
        let reader = hound::WavReader::open(&source).map_err(Error::from)?;
        let length = reader.duration() as f32 / reader.spec().sample_rate.max(1) as f32;
        drop(reader);
        if length <= seconds {
            return Ok(source);
        }
        let stem = source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("voice");
        let name = format!("sample-{}ms-{stem}.wav", (seconds * 1000.0).round() as u64);
        let target = self.dir.join(&name);
        let modified = |p: &Path| fs::metadata(p).and_then(|m| m.modified());
        let fresh = matches!((modified(&target), modified(&source)), (Ok(t), Ok(s)) if t >= s);
        if !fresh {
            for entry in fs::read_dir(&self.dir)?.filter_map(|e| e.ok()) {
                let other = entry.file_name().to_string_lossy().into_owned();
                if other.starts_with("sample-") && other.ends_with(".wav") {
                    let _ = fs::remove_file(entry.path());
                }
            }
            let audio = Audio::read_wav(&source)?;
            let keep = ((seconds * audio.sample_rate as f32) as usize).min(audio.samples.len());
            let part = self.dir.join(format!("{name}.part"));
            let trimmed = Audio {
                samples: audio.samples[..keep].to_vec(),
                sample_rate: audio.sample_rate,
            };
            let written = trimmed
                .write_wav(&part)
                .and_then(|()| fs::rename(&part, &target).map_err(Error::from));
            if let Err(e) = written {
                let _ = fs::remove_file(&part);
                return Err(e);
            }
            keep_private(&self.dir)?;
        }
        Ok(target)
    }

    /// The reference recording, as it was made.
    pub fn reference_path(&self) -> PathBuf {
        self.dir.join("reference.wav")
    }

    /// The cleaned copy. It exists once a clean-up was applied, also while cleaning is off.
    pub fn clean_path(&self) -> PathBuf {
        self.dir.join("clean.wav")
    }

    /// The recording a speech model reads the voice from: the cleaned copy when cleaning is on and
    /// the copy exists, the reference otherwise.
    pub fn voice_path(&self) -> PathBuf {
        match self.cleanup {
            Some(c) if c.enabled && self.clean_path().is_file() => self.clean_path(),
            _ => self.reference_path(),
        }
    }
}

/// Reads `profile.json` from a voice's folder. The folder's name is the voice's name when the file
/// is missing, which is [`Error::ProfileNotFound`].
pub fn read_profile(dir: &Path) -> Result<VoiceProfile> {
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    let bytes = match fs::read(dir.join("profile.json")) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::ProfileNotFound(name));
        }
        Err(e) => return Err(e.into()),
    };
    let mut profile: VoiceProfile = serde_json::from_slice(&bytes)?;
    profile.dir = dir.to_path_buf();
    Ok(profile)
}

/// Writes `profile.json` into the voice's folder as a whole file or not at all.
pub fn write_profile(profile: &VoiceProfile) -> Result<()> {
    let part = profile.dir.join("profile.json.part");
    let bytes = serde_json::to_vec_pretty(profile)?;
    if let Err(e) =
        fs::write(&part, bytes).and_then(|()| fs::rename(&part, profile.dir.join("profile.json")))
    {
        let _ = fs::remove_file(&part);
        return Err(e.into());
    }
    keep_private(&profile.dir)
}

/// On Unix, makes a voice's folder the user's alone (0700) and its files 0600: a recording is
/// personal. Every write of a voice ends with `profile.json`, so this runs after each one. Elsewhere
/// it does nothing, and the folder keeps the rights it inherits.
///
/// It is best effort and never fails the write: a drive that keeps no Unix rights (vfat, exFAT, a
/// CIFS share) refuses every change of them, and the voice must still be kept. The library has no
/// log; the app checks the rights of every voice when it starts and logs what it cannot change.
pub fn keep_private(dir: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|t| t.is_file()) {
                    let _ = fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o600));
                }
            }
        }
    }
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

/// How many bytes of `hay` from its start match `needle` without regard to case, or `None`.
fn match_ignoring_case(hay: &str, needle: &str) -> Option<usize> {
    let mut chars = hay.char_indices();
    let mut used = 0;
    for n in needle.chars() {
        let (at, c) = chars.next()?;
        if !c.to_lowercase().eq(n.to_lowercase()) {
            return None;
        }
        used = at + c.len_utf8();
    }
    (used > 0).then_some(used)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(dir: &Path) -> VoiceProfile {
        VoiceProfile {
            name: "a".into(),
            display_name: None,
            transcript: None,
            consent: Consent {
                speaker: "Test Speaker".into(),
                statement: "I agree to my voice being cloned.".into(),
                recorded_at: "2026-11-13T09:30:00Z".into(),
            },
            cleanup: None,
            owner: None,
            model: DEFAULT_MODEL.into(),
            settings: Default::default(),
            sample_seconds: None,
            words: Vec::new(),
            last_used_at: None,
            dir: dir.to_path_buf(),
        }
    }

    fn word(from: &str, say: &str) -> Word {
        Word {
            from: from.into(),
            say: say.into(),
        }
    }

    #[test]
    fn the_words_list_changes_whole_words_without_regard_to_case_and_longest_first() {
        let mut v = voice(Path::new("."));
        assert!(matches!(
            v.spoken_text("Hi"),
            std::borrow::Cow::Borrowed("Hi")
        ));
        v.words = vec![
            word("GIF", "jiff"),
            word("Dr. Who", "Doctor Who"),
            word("Dr.", "Doctor"),
            word("M\u{fc}ller", "Mueller"),
        ];
        assert_eq!(
            v.spoken_text("A gif, a GIF. Giffy gifs. Dr. Who met Dr. Smith and M\u{dc}LLER."),
            "A jiff, a jiff. Giffy gifs. Doctor Who met Doctor Smith and Mueller."
        );
        assert!(matches!(
            v.spoken_text("The agifted one"),
            std::borrow::Cow::Borrowed(_)
        ));
        assert_eq!(v.spoken_text("gif"), "jiff");
    }

    #[test]
    fn consent_needs_every_field() {
        let tmp = tempfile::tempdir().unwrap();
        let ok = voice(tmp.path()).consent;
        assert!(ok.check().is_ok());
        for field in ["speaker", "statement", "recorded_at"] {
            let mut c = ok.clone();
            match field {
                "speaker" => c.speaker = " ".into(),
                "statement" => c.statement.clear(),
                _ => c.recorded_at.clear(),
            }
            assert!(matches!(c.check(), Err(Error::Consent(f)) if f == field));
        }
    }

    #[test]
    fn shown_name_is_the_display_name_or_the_id() {
        let mut v = voice(Path::new("."));
        assert_eq!(v.shown_name(), "a");
        v.display_name = Some("Office mic".into());
        assert_eq!(v.shown_name(), "Office mic");
    }

    #[test]
    fn a_profile_round_trips_and_a_missing_one_is_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("a");
        fs::create_dir(&dir).unwrap();
        assert!(matches!(read_profile(&dir), Err(Error::ProfileNotFound(n)) if n == "a"));
        let mut v = voice(&dir);
        v.transcript = Some("Hello there.".into());
        write_profile(&v).unwrap();
        // Nothing extra is written for fields a voice does not have, and the part file is gone.
        let text = fs::read_to_string(dir.join("profile.json")).unwrap();
        assert!(!text.contains("last_used_at") && !text.contains("display_name"));
        assert!(!dir.join("profile.json.part").exists());
        assert_eq!(read_profile(&dir).unwrap(), v);
    }

    #[test]
    fn the_time_a_voice_was_last_used_is_kept_without_touching_anything_else() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("a");
        fs::create_dir(&dir).unwrap();
        let v = voice(&dir);
        write_profile(&v).unwrap();
        // A copy loaded before a change must not write that change back.
        let stale = read_profile(&dir).unwrap();
        let mut changed = v.clone();
        changed.transcript = Some("Hello there.".into());
        write_profile(&changed).unwrap();
        stale.mark_used("2026-11-19T10:00:00Z").unwrap();
        let now = read_profile(&dir).unwrap();
        assert_eq!(now.last_used_at.as_deref(), Some("2026-11-19T10:00:00Z"));
        assert_eq!(now.transcript.as_deref(), Some("Hello there."));
        // A voice that is gone is an error and nothing is written.
        fs::remove_dir_all(&dir).unwrap();
        assert!(matches!(
            stale.mark_used("2026-11-19T11:00:00Z"),
            Err(Error::ProfileNotFound(_))
        ));
        assert!(!dir.exists());
    }

    #[test]
    fn the_voice_is_read_from_the_cleaned_copy_only_while_cleaning_is_on() {
        let tmp = tempfile::tempdir().unwrap();
        let mut v = voice(tmp.path());
        assert_eq!(v.voice_path(), v.reference_path());
        v.cleanup = Some(VoiceCleanup {
            enabled: true,
            engine: Engine::Rnnoise,
        });
        // No copy yet: still the reference.
        assert_eq!(v.voice_path(), v.reference_path());
        fs::write(v.clean_path(), b"x").unwrap();
        assert_eq!(v.voice_path(), v.clean_path());
        v.cleanup = Some(VoiceCleanup {
            enabled: false,
            engine: Engine::Rnnoise,
        });
        assert_eq!(v.voice_path(), v.reference_path());
    }

    #[test]
    fn a_voice_can_use_only_the_start_of_its_recording() {
        let tmp = tempfile::tempdir().unwrap();
        let mut v = voice(tmp.path());
        let audio = Audio {
            samples: vec![0.1; 8_000 * 10],
            sample_rate: 8_000,
        };
        audio.write_wav(v.reference_path()).unwrap();
        // No sample length: the recording itself.
        assert_eq!(v.sample_file().unwrap(), v.reference_path());
        // Longer than the recording: still the recording.
        v.sample_seconds = Some(20.0);
        assert_eq!(v.sample_file().unwrap(), v.reference_path());
        assert!(!v.sample_is_trimmed());
        // Shorter: a copy of the first seconds.
        v.sample_seconds = Some(4.0);
        let part = v.sample_file().unwrap();
        assert_ne!(part, v.reference_path());
        assert_eq!(Audio::read_wav(&part).unwrap().samples.len(), 4 * 8_000);
        assert!(v.sample_is_trimmed());
        // A transcript of the whole recording is no longer what the model hears.
        v.transcript = Some("Hello.".into());
        assert_eq!(v.transcript_for_model(), None);
        v.sample_seconds = None;
        assert_eq!(v.transcript_for_model(), Some("Hello."));
    }
}
