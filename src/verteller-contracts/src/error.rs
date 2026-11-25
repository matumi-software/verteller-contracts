//! The error type for the whole crate.

/// Everything that can go wrong in this crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A profile name that is empty, too long or has characters outside letters, digits, `-` and `_`.
    #[error("invalid profile name {0:?}: use 1 to 64 letters, digits, '-' or '_'")]
    InvalidName(String),
    /// A display name that is over 80 characters or has a control character in it.
    #[error("invalid display name {0:?}: use at most 80 characters and no control characters")]
    InvalidDisplayName(String),
    /// A profile with this name is already stored.
    #[error("profile {0:?} already exists")]
    ProfileExists(String),
    /// Another voice of the same person already has this display name.
    #[error("another voice is already called {0:?}")]
    DisplayNameTaken(String),
    /// No profile with this name is stored.
    #[error("profile {0:?} not found")]
    ProfileNotFound(String),
    /// A draft was saved or looked at for a voice that has none.
    #[error("voice {0:?} has no draft")]
    NoDraft(String),
    /// The saved voice changed since its draft began.
    #[error("voice {0:?} was changed after its draft was started")]
    DraftOutOfDate(String),
    /// A field of the consent record is empty.
    #[error("the consent record is incomplete: {0} is empty")]
    Consent(&'static str),
    /// The reference recording is shorter than [`crate::MIN_REFERENCE_SECONDS`].
    #[error("the reference recording is {got:.1} seconds, at least {min:.1} are needed")]
    ReferenceTooShort {
        /// Length of the recording.
        got: f32,
        /// The minimum.
        min: f32,
    },
    /// The reference recording is longer than [`crate::MAX_REFERENCE_SECONDS`].
    #[error("the recording is longer than {max:.0} seconds, which is the most a voice takes")]
    ReferenceTooLong {
        /// The maximum.
        max: f32,
    },
    /// A voice was linked to a model the registry does not have.
    #[error("there is no speech model called {0:?}")]
    UnknownModel(String),
    /// A voice's settings did not fit the schema of its model. The text is a sentence for the person.
    #[error("{0}")]
    InvalidSetting(String),
    /// The recording is not a WAV and could not be decoded. The text is a sentence for the person.
    #[error("{0}")]
    Decode(String),
    /// Cleaning was turned on for a voice that has no cleaned copy yet.
    #[error("profile {0:?} has no cleaned copy yet")]
    NotCleaned(String),
    /// The script has no words in it.
    #[error("the script is empty")]
    EmptyScript,
    /// The script has a pause marker that is malformed or over a limit, or a stray brace. The text
    /// is a sentence for the person.
    #[error("{0}")]
    InvalidScript(String),
    /// A file operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A profile file could not be read or written.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// A WAV file could not be read or written.
    #[error(transparent)]
    Wav(#[from] hound::Error),
    /// The speech model failed.
    #[error("speech backend: {0}")]
    Backend(String),
    /// The speech model ran out of GPU memory.
    #[error("the GPU ran out of memory: {0}")]
    OutOfMemory(String),
    /// The job was cancelled before it finished. No partial result is returned.
    #[error("cancelled")]
    Cancelled,
}

impl Error {
    /// Wraps a backend's error text: [`Error::OutOfMemory`] when the text says the GPU ran out of
    /// memory, [`Error::Backend`] otherwise. The match is on the text, case-insensitive, because
    /// CUDA and the library report it as a string.
    pub fn from_backend_message(message: impl Into<String>) -> Self {
        let message = message.into();
        if is_out_of_memory(&message) {
            Self::OutOfMemory(message)
        } else {
            Self::Backend(message)
        }
    }
}

/// Phrases that CUDA and cuDNN use when a GPU allocation fails.
fn is_out_of_memory(message: &str) -> bool {
    const MARKERS: [&str; 5] = [
        "out of memory",
        "out_of_memory",
        "cudaerrormemoryallocation",
        "failed to allocate memory",
        "cuda_error_out_of_memory",
    ];
    let lower = message.to_lowercase();
    MARKERS.iter().any(|m| lower.contains(m))
}

/// A `Result` with this crate's [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// The most characters of a refused name an error repeats.
pub const MAX_ECHO_CHARS: usize = 80;

/// A refused name as an error repeats it: the first [`MAX_ECHO_CHARS`] characters, and `...` when
/// there were more. A name can be as long as a request, and the error goes back in the answer.
pub fn echo(text: &str) -> String {
    match text.char_indices().nth(MAX_ECHO_CHARS) {
        Some((at, _)) => format!("{}...", &text[..at]),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_out_of_memory_texts() {
        for text in [
            "CUDA failure 2: out of memory ; GPU=0",
            "CUDA error: Out Of Memory",
            "cudaErrorMemoryAllocation: out of memory",
            "CUDA_ERROR_OUT_OF_MEMORY",
            "Failed to allocate memory for requested buffer of size 1073741824",
        ] {
            assert!(
                matches!(Error::from_backend_message(text), Error::OutOfMemory(m) if m == text),
                "{text}"
            );
        }
    }

    #[test]
    fn other_texts_stay_backend_errors() {
        for text in [
            "A tokenizer error occurred: bad input",
            "A model error occurred: file not found",
            "",
        ] {
            assert!(matches!(
                Error::from_backend_message(text),
                Error::Backend(_)
            ));
        }
    }
}
