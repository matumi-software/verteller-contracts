//! Cutting a script into pieces the speech model can read in one go, and finding the pause
//! markers in it.
//!
//! A pause marker is `{pause 1.5s}` or `{pause 400ms}`. It is never spoken: it becomes silence in
//! the saved file at the point where it stands.

use crate::{Error, Result};

/// The longest silence one pause marker may ask for, in milliseconds.
pub const MAX_PAUSE_MARKER_MS: u32 = 30_000;

/// The most silence all the pause markers of one script may ask for together, in milliseconds.
pub const MAX_PAUSE_MARKERS_TOTAL_MS: u32 = 300_000;

/// One piece of a script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// The text to speak.
    pub text: String,
    /// The piece ends a paragraph, so the pause after it is longer.
    pub ends_paragraph: bool,
    /// Silence the script asked for before this piece with pause markers. Only the first piece of
    /// a script ever has any: a marker anywhere else is silence after the piece before it.
    pub pause_before_ms: u32,
    /// Silence the script asked for after this piece with pause markers. It comes on top of the
    /// sentence or paragraph pause, and after the last piece it is silence at the end of the file.
    pub pause_after_ms: u32,
}

/// Splits `script` into segments of at most `max_chars` characters where it can.
///
/// Paragraphs are separated by a blank line. Inside a paragraph, whole sentences are packed into a
/// segment until the next one would not fit. A sentence longer than `max_chars` is cut at word
/// boundaries. A single word longer than `max_chars` stays whole.
///
/// A pause marker ends the segment before it, so the silence sits exactly there, and the text
/// after it starts a new one. The marker is not part of any segment's text and does not count
/// toward `max_chars`. A backslash before a brace (`\{`) is a plain brace that is spoken, and the
/// backslash is dropped. Any other `{` must open a valid marker, or the script is refused with
/// [`Error::InvalidScript`], as it is when one marker is over [`MAX_PAUSE_MARKER_MS`] or all of
/// them together are over [`MAX_PAUSE_MARKERS_TOTAL_MS`].
///
/// A full stop after an abbreviation such as "Dr." counts as a sentence end. That only matters when
/// it lands on a segment boundary, and the cost is a short pause.
pub fn split_script(script: &str, max_chars: usize) -> Result<Vec<Segment>> {
    let max_chars = max_chars.max(1);
    let script = script.replace("\r\n", "\n");
    let mut out: Vec<Segment> = Vec::new();
    let mut lead = 0u32;
    let mut total = 0u32;
    // The line the next paragraph starts on, kept as a running count so a long script is read once.
    let mut line = 1;
    for paragraph in script.split("\n\n") {
        let starts_on = line;
        line += paragraph.matches('\n').count() + 2;
        let first = out.len();
        let mut run = String::new();
        for item in items(paragraph, starts_on)? {
            match item {
                Item::Text(text) => run.push_str(&text),
                Item::Pause(ms) => {
                    pack(&run, max_chars, &mut out);
                    run.clear();
                    total = total.saturating_add(ms);
                    if total > MAX_PAUSE_MARKERS_TOTAL_MS {
                        return Err(Error::InvalidScript(format!(
                            "The pauses in this script add up to more than the {} allowed for \
                             one script.",
                            describe_ms(MAX_PAUSE_MARKERS_TOTAL_MS)
                        )));
                    }
                    match out.last_mut() {
                        Some(last) => last.pause_after_ms += ms,
                        None => lead += ms,
                    }
                }
            }
        }
        pack(&run, max_chars, &mut out);
        if out.len() > first {
            out.last_mut().expect("just pushed").ends_paragraph = true;
        }
    }
    if let Some(first) = out.first_mut() {
        first.pause_before_ms = lead;
    }
    Ok(out)
}

/// What a paragraph is made of: text and pause markers.
enum Item {
    Text(String),
    Pause(u32),
}

const HOW_TO_WRITE: &str = "Write a pause as {pause 1.5s} or {pause 400ms}. To say a brace aloud, \
                            put a backslash in front of it: \\{.";

/// Reads the pause markers out of one paragraph. `line` is the line the paragraph starts on, for
/// the messages.
fn items(paragraph: &str, mut line: usize) -> Result<Vec<Item>> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut rest = paragraph;
    while let Some(at) = rest.find(['{', '\\']) {
        let (before, here) = rest.split_at(at);
        line += before.matches('\n').count();
        text.push_str(before);
        if let Some(after) = here.strip_prefix("\\{") {
            text.push('{');
            rest = after;
        } else if let Some(after) = here.strip_prefix('\\') {
            text.push('\\');
            rest = after;
        } else {
            let Some(end) = here.find('}') else {
                return Err(Error::InvalidScript(format!(
                    "Line {line}: a {{ has no closing }}. {HOW_TO_WRITE}"
                )));
            };
            let marker = &here[..=end];
            let ms = marker_ms(marker).map_err(|why| {
                Error::InvalidScript(format!(
                    "Line {line}: {} {why} {HOW_TO_WRITE}",
                    shown(marker)
                ))
            })?;
            if ms > MAX_PAUSE_MARKER_MS {
                return Err(Error::InvalidScript(format!(
                    "Line {line}: {} asks for more than the {} one pause can be.",
                    shown(marker),
                    describe_ms(MAX_PAUSE_MARKER_MS)
                )));
            }
            if !text.is_empty() {
                out.push(Item::Text(std::mem::take(&mut text)));
            }
            out.push(Item::Pause(ms));
            line += marker.matches('\n').count();
            rest = &here[end + 1..];
        }
    }
    text.push_str(rest);
    if !text.is_empty() {
        out.push(Item::Text(text));
    }
    Ok(out)
}

/// A marker as a message shows it, cut short when it is long.
fn shown(marker: &str) -> String {
    let flat = marker.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 30 {
        let cut: String = flat.chars().take(30).collect();
        format!("\"{cut}...\"")
    } else {
        format!("\"{flat}\"")
    }
}

/// The milliseconds a marker such as `{pause 1.5s}` asks for. The error is the end of a sentence
/// that says what is wrong with the marker.
fn marker_ms(marker: &str) -> std::result::Result<u32, &'static str> {
    let inner = &marker[1..marker.len() - 1];
    let mut words = inner.split_whitespace();
    if !words
        .next()
        .is_some_and(|w| w.eq_ignore_ascii_case("pause"))
    {
        return Err("is not a pause marker.");
    }
    let (Some(amount), None) = (words.next(), words.next()) else {
        return Err("needs one length after the word pause, such as 1.5s.");
    };
    let amount = amount.to_ascii_lowercase();
    let (digits, per_unit) = match amount.strip_suffix("ms") {
        Some(d) => (d, 1u64),
        None => match amount.strip_suffix('s') {
            Some(d) => (d, 1000),
            None => return Err("needs the unit s or ms after the length."),
        },
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    let plain = |d: &str| d.bytes().all(|b| b.is_ascii_digit());
    if (whole.is_empty() && fraction.is_empty()) || !plain(whole) || !plain(fraction) {
        return Err("does not have a plain number for its length.");
    }
    if per_unit == 1 && !fraction.is_empty() {
        return Err("takes whole milliseconds. Use seconds for a fraction, as in 1.5s.");
    }
    if fraction.len() > 3 {
        return Err("takes at most three decimal places.");
    }
    // Anything this long is over the limit whatever it says, and this keeps the sum from overflowing.
    let whole = whole.trim_start_matches('0');
    if whole.len() > 9 {
        return Ok(u32::MAX);
    }
    let whole: u64 = if whole.is_empty() {
        0
    } else {
        whole.parse().expect("digits only")
    };
    let mut ms = whole * per_unit;
    if per_unit == 1000 && !fraction.is_empty() {
        ms += format!("{fraction:0<3}")
            .parse::<u64>()
            .expect("digits only");
    }
    Ok(ms.min(u64::from(u32::MAX)) as u32)
}

/// A length of silence in words: "30 seconds", "1.5 seconds", "5 minutes".
fn describe_ms(ms: u32) -> String {
    if ms >= 60_000 && ms.is_multiple_of(60_000) {
        let m = ms / 60_000;
        return format!("{m} minute{}", if m == 1 { "" } else { "s" });
    }
    let text = format!("{:.3}", f64::from(ms) / 1000.0);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{text} second{}", if text == "1" { "" } else { "s" })
}

/// Cuts one run of text (no markers in it) into segments and adds them to `out`.
fn pack(text: &str, max_chars: usize, out: &mut Vec<Segment>) {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.is_empty() {
        return;
    }
    let mut current = String::new();
    for sentence in sentences(&flat) {
        for piece in cut_long(&sentence, max_chars) {
            if !current.is_empty()
                && current.chars().count() + 1 + piece.chars().count() > max_chars
            {
                out.push(segment(std::mem::take(&mut current)));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(&piece);
        }
    }
    if !current.is_empty() {
        out.push(segment(current));
    }
}

fn segment(text: String) -> Segment {
    Segment {
        text,
        ends_paragraph: false,
        pause_before_ms: 0,
        pause_after_ms: 0,
    }
}

fn sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut current = String::new();
    for (i, &c) in chars.iter().enumerate() {
        current.push(c);
        let ends = matches!(c, '.' | '!' | '?' | '\u{2026}');
        if ends && chars.get(i + 1).is_none_or(|n| n.is_whitespace()) {
            out.push(current.trim().to_string());
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

fn cut_long(sentence: &str, max_chars: usize) -> Vec<String> {
    if sentence.chars().count() <= max_chars {
        return vec![sentence.to_string()];
    }
    let mut out = Vec::new();
    let mut current = String::new();
    for word in sentence.split(' ') {
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > max_chars {
            out.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(script: &str, max: usize) -> Vec<Segment> {
        split_script(script, max).unwrap()
    }

    fn texts(segments: &[Segment]) -> Vec<&str> {
        segments.iter().map(|s| s.text.as_str()).collect()
    }

    fn refused(script: &str) -> String {
        match split_script(script, 300) {
            Err(Error::InvalidScript(m)) => m,
            other => panic!("expected a refused script, got {other:?}"),
        }
    }

    #[test]
    fn short_script_is_one_segment() {
        let s = split("Hello there. How are you?", 300);
        assert_eq!(texts(&s), ["Hello there. How are you?"]);
        assert!(s[0].ends_paragraph);
    }

    #[test]
    fn sentences_pack_up_to_the_limit() {
        let s = split("One two. Three four. Five six.", 20);
        assert_eq!(texts(&s), ["One two. Three four.", "Five six."]);
        assert!(!s[0].ends_paragraph);
        assert!(s[1].ends_paragraph);
    }

    #[test]
    fn blank_line_starts_a_new_paragraph() {
        let s = split("First part.\n\nSecond part.", 300);
        assert_eq!(texts(&s), ["First part.", "Second part."]);
        assert!(s.iter().all(|x| x.ends_paragraph));
    }

    #[test]
    fn single_newline_is_just_a_space() {
        let s = split("Line one\nline two.", 300);
        assert_eq!(texts(&s), ["Line one line two."]);
    }

    #[test]
    fn long_sentence_is_cut_at_words() {
        let s = split("aaa bbb ccc ddd eee", 8);
        assert_eq!(texts(&s), ["aaa bbb", "ccc ddd", "eee"]);
    }

    #[test]
    fn decimals_do_not_end_a_sentence() {
        let s = split("It costs 3.5 dollars. Fine.", 21);
        assert_eq!(texts(&s), ["It costs 3.5 dollars.", "Fine."]);
    }

    #[test]
    fn empty_and_blank_scripts_give_nothing() {
        assert!(split("", 300).is_empty());
        assert!(split("  \n\n \r\n ", 300).is_empty());
    }

    #[test]
    fn a_word_longer_than_the_limit_stays_whole() {
        let s = split("supercalifragilistic", 5);
        assert_eq!(texts(&s), ["supercalifragilistic"]);
    }

    fn pauses(segments: &[Segment]) -> Vec<(u32, u32)> {
        segments
            .iter()
            .map(|s| (s.pause_before_ms, s.pause_after_ms))
            .collect()
    }

    #[test]
    fn a_marker_between_sentences_ends_the_segment_and_is_not_spoken() {
        let s = split("One. {pause 1.5s} Two.", 300);
        assert_eq!(texts(&s), ["One.", "Two."]);
        assert_eq!(pauses(&s), [(0, 1500), (0, 0)]);
        assert!(!s[0].ends_paragraph);
        assert!(s[1].ends_paragraph);
    }

    #[test]
    fn a_marker_on_its_own_line_does_not_start_a_paragraph() {
        let s = split("One.\n{pause 400ms}\nTwo.", 300);
        assert_eq!(texts(&s), ["One.", "Two."]);
        assert_eq!(pauses(&s), [(0, 400), (0, 0)]);
        assert!(!s[0].ends_paragraph);
    }

    #[test]
    fn a_marker_inside_a_sentence_cuts_it_in_two() {
        let s = split("And the winner is {pause 2s} Tom.", 300);
        assert_eq!(texts(&s), ["And the winner is", "Tom."]);
        assert_eq!(s[0].pause_after_ms, 2000);
    }

    #[test]
    fn markers_in_a_row_add_up() {
        let s = split("One. {pause 1s}{pause 250ms}\n{pause 0.5s} Two.", 300);
        assert_eq!(texts(&s), ["One.", "Two."]);
        assert_eq!(s[0].pause_after_ms, 1750);
    }

    #[test]
    fn a_marker_at_the_start_is_silence_before_the_first_segment() {
        let s = split("{pause 2s}\n\nHello.", 300);
        assert_eq!(texts(&s), ["Hello."]);
        assert_eq!(pauses(&s), [(2000, 0)]);
    }

    #[test]
    fn a_marker_at_the_end_is_silence_after_the_last_segment() {
        let s = split("Hello. {pause 3s}", 300);
        assert_eq!(texts(&s), ["Hello."]);
        assert_eq!(pauses(&s), [(0, 3000)]);
        assert!(s[0].ends_paragraph);
    }

    #[test]
    fn a_marker_across_a_paragraph_break_goes_with_the_paragraph_before() {
        let s = split("One.\n\n{pause 1s}\n\nTwo.", 300);
        assert_eq!(texts(&s), ["One.", "Two."]);
        assert_eq!(pauses(&s), [(0, 1000), (0, 0)]);
        assert!(s[0].ends_paragraph && s[1].ends_paragraph);
    }

    #[test]
    fn a_script_of_only_markers_has_no_segments() {
        assert!(split("{pause 1s}\n\n{pause 2s}", 300).is_empty());
    }

    #[test]
    fn markers_do_not_count_toward_the_length_limit_or_split_sentences() {
        // Both sentences fit in 20 characters once the marker is gone, but the marker ends the
        // segment, so they are two. Without the marker the same text packs into one.
        let with = split("One two. {pause 1s} Three four.", 20);
        assert_eq!(texts(&with), ["One two.", "Three four."]);
        let without = split("One two. Three four.", 20);
        assert_eq!(texts(&without), ["One two. Three four."]);
        // A long marker next to a short text does not push a segment over its limit.
        let s = split("aaa bbb {pause 29.999s} ccc ddd", 8);
        assert_eq!(texts(&s), ["aaa bbb", "ccc ddd"]);
    }

    #[test]
    fn units_and_case_are_forgiven_and_decimals_are_exact() {
        for (marker, ms) in [
            ("{pause 1s}", 1000),
            ("{PAUSE 1.5S}", 1500),
            ("{ pause   400ms }", 400),
            ("{pause 0.25s}", 250),
            ("{pause .5s}", 500),
            ("{pause 1.001s}", 1001),
            ("{pause 0s}", 0),
            ("{pause 30s}", 30_000),
            ("{pause 30000ms}", 30_000),
            ("{pause 0000000001s}", 1000),
            ("{pause 000000000000000500ms}", 500),
        ] {
            let s = split(&format!("A. {marker} B."), 300);
            assert_eq!(s[0].pause_after_ms, ms, "{marker}");
        }
    }

    #[test]
    fn a_backslash_makes_a_brace_plain() {
        let s = split("Say \\{pause 2s} out loud.", 300);
        assert_eq!(texts(&s), ["Say {pause 2s} out loud."]);
        assert_eq!(pauses(&s), [(0, 0)]);
        // Another backslash stays as written.
        let s = split("A C:\\dir and \\{ x.", 300);
        assert_eq!(texts(&s), ["A C:\\dir and { x."]);
    }

    #[test]
    fn a_marker_that_is_not_valid_is_refused_with_its_line() {
        for bad in [
            "{pause}",
            "{pause soon}",
            "{pause 2}",
            "{pause 2m}",
            "{pause -1s}",
            "{pause 1.2.3s}",
            "{pause 1.5ms}",
            "{pause 1.2345s}",
            "{pause 1s 2s}",
            "{wait 1s}",
            "{}",
            "{name}",
            "{pause 1s",
        ] {
            let m = refused(&format!("Fine.\n\nSecond line {bad} here."));
            assert!(m.starts_with("Line 3: "), "{bad}: {m}");
            assert!(m.contains("{pause 1.5s}"), "{bad}: {m}");
            assert!(!m.contains('\u{2014}') && !m.contains('\u{2013}'));
        }
    }

    #[test]
    fn the_limits_are_per_marker_and_per_script() {
        assert!(refused("A. {pause 30.001s} B.").contains("30 seconds"));
        assert!(refused("A. {pause 99999999999999s} B.").contains("30 seconds"));
        split("A. {pause 30s} B.", 300);
        let ten = "A. {pause 30s} B. ".repeat(10);
        split(&ten, 300);
        let eleven = "A. {pause 30s} B. ".repeat(11);
        assert!(refused(&eleven).contains("5 minutes"));
    }

    #[test]
    fn line_numbers_count_windows_line_ends_once() {
        let m = refused("One.\r\nTwo.\r\n\r\nFour {oops}.");
        assert!(m.starts_with("Line 4: "), "{m}");
    }

    #[test]
    fn a_script_of_many_paragraphs_and_markers_is_cut_in_linear_time() {
        let n = 200_000;
        let mut script = "x {pause 0s}\n\n".repeat(n);
        let ok = script.clone();
        script.push_str("bad {oops}");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let segments = split_script(&ok, 300).unwrap();
            let error = match split_script(&script, 300) {
                Err(Error::InvalidScript(m)) => m,
                other => panic!("expected a refused script, got {other:?}"),
            };
            let _ = tx.send((segments.len(), error));
        });
        let (count, error) = rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("splitting took far too long");
        assert_eq!(count, n);
        // Each paragraph is one line and one blank line, so the bad one is on line 2n + 1.
        assert!(
            error.starts_with(&format!("Line {}: ", 2 * n + 1)),
            "{error}"
        );
    }
}
