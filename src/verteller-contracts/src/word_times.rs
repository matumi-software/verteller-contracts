//! Word times: where each spoken word starts and ends in a recording.
//!
//! The speech model gives times in samples of the audio it was fed (16 kHz). The editor wants
//! frames of the source, which is the rate of the file the person sent. [`frames_of`] converts and
//! tidies the list so it always meets the rules the editor checks when it stores words: in order,
//! not overlapping, inside the source, every word at least one frame, a word of 80 characters at
//! most and 30000 words at most.

use serde::{Deserialize, Serialize};

/// Words on one source, at most. The editor refuses more.
pub const MAX_TIMED_WORDS: usize = 30_000;

/// The longest word, in characters. The editor refuses a longer one.
pub const MAX_TIMED_WORD_CHARS: usize = 80;

/// One spoken word and the stretch of the recording that holds it. The shape of the editor's
/// `Word` (`{ text, start, end }`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimedWord {
    /// The word as the model wrote it, with its punctuation when the model keeps it.
    pub text: String,
    /// First frame of the word.
    pub start: u64,
    /// One past the last frame of the word.
    pub end: u64,
}

/// Turns the model's words into frames of a recording of `total_frames` frames at `to_rate`.
///
/// `raw` is each word's text and its start and end in samples at `from_rate`. The result is sorted
/// by start, every time is clamped into the recording, a word that starts before the one before it
/// ends is moved to start where that one ends, and a word left with no length (the model gave it
/// none, or it fell outside the recording) is dropped, as is one with no text. A word longer than
/// [`MAX_TIMED_WORD_CHARS`] is cut. Past [`MAX_TIMED_WORDS`] the rest is left out.
pub fn frames_of<'a>(
    raw: impl IntoIterator<Item = (&'a str, i64, i64)>,
    from_rate: u32,
    to_rate: u32,
    total_frames: u64,
) -> Vec<TimedWord> {
    let from = i128::from(from_rate.max(1));
    let to = i128::from(to_rate.max(1));
    let convert = |sample: i64| -> u64 {
        // Rounded to the nearest frame. A negative time is the start of the recording.
        let frames = (i128::from(sample.max(0)) * to + from / 2) / from;
        u64::try_from(frames).unwrap_or(u64::MAX).min(total_frames)
    };
    let mut words: Vec<(String, u64, u64)> = raw
        .into_iter()
        .filter_map(|(text, start, end)| {
            let text = text.trim();
            if text.is_empty() {
                return None;
            }
            let text: String = text.chars().take(MAX_TIMED_WORD_CHARS).collect();
            Some((text, convert(start), convert(end)))
        })
        .collect();
    // A stable sort keeps the model's order for words that start together.
    words.sort_by_key(|w| w.1);
    let mut out: Vec<TimedWord> = Vec::with_capacity(words.len().min(MAX_TIMED_WORDS));
    let mut last_end = 0;
    for (text, start, end) in words {
        let start = start.max(last_end);
        if end <= start {
            continue;
        }
        if out.len() == MAX_TIMED_WORDS {
            break;
        }
        last_end = end;
        out.push(TimedWord { text, start, end });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(raw: &[(&str, i64, i64)], from: u32, to: u32, total: u64) -> Vec<TimedWord> {
        frames_of(raw.iter().copied(), from, to, total)
    }

    fn word(text: &str, start: u64, end: u64) -> TimedWord {
        TimedWord {
            text: text.into(),
            start,
            end,
        }
    }

    #[test]
    fn samples_at_16_khz_become_frames_at_the_sources_rate() {
        // 0.5 s to 1 s at 16 kHz, in a 48 kHz source of 2 s.
        let got = words(&[("hello", 8_000, 16_000)], 16_000, 48_000, 96_000);
        assert_eq!(got, vec![word("hello", 24_000, 48_000)]);
        // The same rate changes nothing.
        let got = words(&[("hello", 8_000, 16_000)], 16_000, 16_000, 32_000);
        assert_eq!(got, vec![word("hello", 8_000, 16_000)]);
        // A lower rate rounds to the nearest frame: 3 samples at 16 kHz is 1.5 frames at 8 kHz.
        let got = words(&[("a", 1, 3)], 16_000, 8_000, 100);
        assert_eq!(got, vec![word("a", 1, 2)]);
    }

    #[test]
    fn times_are_clamped_into_the_recording_and_a_word_outside_it_is_dropped() {
        let got = words(
            &[
                ("in", -5, 100),
                ("past", 900, 2_000),
                ("gone", 1_500, 2_000),
            ],
            16_000,
            16_000,
            1_000,
        );
        assert_eq!(got, vec![word("in", 0, 100), word("past", 900, 1_000)]);
    }

    #[test]
    fn words_come_out_in_order_and_do_not_overlap() {
        let got = words(
            &[("two", 150, 400), ("one", 0, 200), ("three", 390, 500)],
            16_000,
            16_000,
            1_000,
        );
        assert_eq!(
            got,
            vec![
                word("one", 0, 200),
                word("two", 200, 400),
                word("three", 400, 500)
            ]
        );
        for pair in got.windows(2) {
            assert!(pair[0].end <= pair[1].start);
        }
    }

    #[test]
    fn a_word_with_no_length_or_no_text_is_dropped() {
        let got = words(
            &[
                ("empty", 100, 100),
                ("backwards", 300, 200),
                ("  ", 0, 50),
                ("kept", 400, 500),
            ],
            16_000,
            16_000,
            1_000,
        );
        assert_eq!(got, vec![word("kept", 400, 500)]);
    }

    #[test]
    fn a_long_word_is_cut_and_the_list_is_capped() {
        let long = "x".repeat(200);
        let got = words(&[(long.as_str(), 0, 10)], 16_000, 16_000, 100);
        assert_eq!(got[0].text.chars().count(), MAX_TIMED_WORD_CHARS);

        let many: Vec<(&str, i64, i64)> = (0..MAX_TIMED_WORDS as i64 + 50)
            .map(|i| ("w", i * 2, i * 2 + 1))
            .collect();
        let got = words(&many, 16_000, 16_000, u64::MAX);
        assert_eq!(got.len(), MAX_TIMED_WORDS);
    }

    #[test]
    fn the_shape_is_the_editors_word() {
        let json = serde_json::to_value(word("hi", 1, 2)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "text": "hi", "start": 1, "end": 2 })
        );
    }
}
