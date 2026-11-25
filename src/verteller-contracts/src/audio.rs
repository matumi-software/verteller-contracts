//! Mono audio in memory, and writing it as a WAV file.

use crate::Result;
use std::path::Path;

// The dead air check, [`Audio::collapse_stalls`]. Every number below comes from the four voiceovers
// of the issue #75 hand test (CUDA from the original and the cleaned recording, and the CPU),
// measured in 50 ms frames.

/// The length of one frame whose level the check measures.
const STALL_FRAME_MS: u32 = 50;
/// The part's speech level is this percentile of its frame levels. Speech filled at least 38 % of
/// the frames of every part, so this sits in speech (-17.6, -17.8 and -13.5 dBFS in the original,
/// the cleaned and the CPU file), and one loud click cannot move it.
const SPEECH_LEVEL_PERCENTILE: f32 = 0.95;
/// A frame is quiet when it is at least this far under the speech level. The model's dead air in
/// the original file sat 33 dB under on average, with its loudest frame 25.6 dB under, so 30 dB
/// would cut the 16 s block in two. In the cleaned file it sat 46 to 48 dB under.
const QUIET_BELOW_SPEECH_DB: f32 = 25.0;
/// A quiet stretch must last at least this long to be dead air. The natural pauses inside a part
/// lasted 0.3 to 0.55 s in all four files, and the dead air 3.1, 3.85, 4.3 and 16 s. The longest
/// stretch in between, 2.0 s in the cleaned file and half of it digital silence, stays under this.
const MIN_STALL_MS: u32 = 2_500;
/// A quiet stretch must also be steady: the standard deviation of its frame levels at most this.
/// The dead air measured 1.2 to 2.5 dB, the natural pauses, which fade out, 4.1 to 7.4 dB, and the
/// 2.0 s stretch 8.7 dB.
const MAX_STALL_SPREAD_DB: f32 = 5.0;
/// Frame levels are counted no lower than this in the spread, so digital silence is steady.
const LEVEL_FLOOR_DB: f32 = -90.0;

/// A stretch of dead air found by [`Audio::find_stalls`], as sample indexes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stall {
    /// The first sample of the stretch.
    pub start: usize,
    /// One past the last sample.
    pub end: usize,
    /// True when the stretch runs to the end of the audio, with no speech after it.
    pub at_end: bool,
}

/// Mono samples between -1.0 and 1.0.
#[derive(Debug, Clone, PartialEq)]
pub struct Audio {
    /// The samples.
    pub samples: Vec<f32>,
    /// Samples per second.
    pub sample_rate: u32,
}

impl Audio {
    /// Audio with no samples.
    pub fn empty(sample_rate: u32) -> Self {
        Self {
            samples: Vec::new(),
            sample_rate,
        }
    }

    /// `ms` milliseconds of silence.
    pub fn silence(sample_rate: u32, ms: u32) -> Self {
        let count = (u64::from(sample_rate) * u64::from(ms) / 1000) as usize;
        Self {
            samples: vec![0.0; count],
            sample_rate,
        }
    }

    /// Length in seconds.
    pub fn seconds(&self) -> f32 {
        self.samples.len() as f32 / self.sample_rate as f32
    }

    /// Adds `other` to the end. Both must have the same sample rate.
    ///
    /// # Panics
    ///
    /// Panics when the sample rates differ. Mixing them is a bug in the caller.
    pub fn append(&mut self, other: &Audio) {
        assert_eq!(self.sample_rate, other.sample_rate, "sample rates differ");
        self.samples.extend_from_slice(&other.samples);
    }

    /// Scales the audio so its loudest sample is at `target_dbfs` (for example -1.0).
    /// Silent audio is left alone.
    pub fn normalize_peak(&mut self, target_dbfs: f32) {
        let peak = self.samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        if peak <= f32::EPSILON {
            return;
        }
        let gain = 10_f32.powf(target_dbfs / 20.0) / peak;
        for s in &mut self.samples {
            *s *= gain;
        }
    }

    /// The model's dead air cut out: a copy of the audio in which every stall is shortened to
    /// `pause_ms`, or removed when it runs to the end. Audio with no stall comes back unchanged.
    ///
    /// A stall is a quiet stretch after speech that is long and steady: every 50 ms frame at least
    /// `QUIET_BELOW_SPEECH_DB` under the part's speech level, `MIN_STALL_MS` or longer, and with
    /// frame levels that vary by no more than `MAX_STALL_SPREAD_DB`. Speech, the pauses between
    /// words and sentences, any quiet before the first word and any quiet stretch shorter than
    /// `MIN_STALL_MS` are never touched. A shortened stall keeps its first and its last
    /// `pause_ms / 2`, so the fade of one word and the start of the next stay as they were.
    ///
    /// Issue #75: from a reference with room noise, a model kept making that noise for
    /// up to 16 s after a sentence, then went on reading.
    pub fn collapse_stalls(&self, pause_ms: u32) -> Audio {
        let stalls = self.find_stalls();
        if stalls.is_empty() {
            return self.clone();
        }
        let keep = (u64::from(pause_ms) * u64::from(self.sample_rate) / 1000) as usize;
        let mut samples = Vec::with_capacity(self.samples.len());
        let mut from = 0;
        for stall in stalls {
            samples.extend_from_slice(&self.samples[from..stall.start]);
            if !stall.at_end {
                let keep = keep.min(stall.end - stall.start);
                let head = keep / 2;
                samples.extend_from_slice(&self.samples[stall.start..stall.start + head]);
                samples.extend_from_slice(&self.samples[stall.end - (keep - head)..stall.end]);
            }
            from = stall.end;
        }
        samples.extend_from_slice(&self.samples[from..]);
        Audio {
            samples,
            sample_rate: self.sample_rate,
        }
    }

    /// The stalls [`Audio::collapse_stalls`] cuts, in order.
    pub(crate) fn find_stalls(&self) -> Vec<Stall> {
        let frame = (self.sample_rate as usize * STALL_FRAME_MS as usize / 1000).max(1);
        let levels: Vec<f32> = self
            .samples
            .chunks(frame)
            .map(|chunk| {
                let mean = chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32;
                10.0 * (mean + 1e-12).log10()
            })
            .collect();
        if levels.is_empty() {
            return Vec::new();
        }
        let mut sorted = levels.clone();
        sorted.sort_by(f32::total_cmp);
        let speech = sorted[((sorted.len() - 1) as f32 * SPEECH_LEVEL_PERCENTILE) as usize];
        let quiet = |l: f32| l < speech - QUIET_BELOW_SPEECH_DB;
        let min_frames = (MIN_STALL_MS / STALL_FRAME_MS) as usize;
        let mut stalls = Vec::new();
        let mut i = 0;
        while i < levels.len() {
            if !quiet(levels[i]) {
                i += 1;
                continue;
            }
            let start = i;
            while i < levels.len() && quiet(levels[i]) {
                i += 1;
            }
            // A quiet start has no speech before it, so it is not a stall.
            if start == 0 || i - start < min_frames {
                continue;
            }
            let run: Vec<f32> = levels[start..i]
                .iter()
                .map(|l| l.max(LEVEL_FLOOR_DB))
                .collect();
            let mean = run.iter().sum::<f32>() / run.len() as f32;
            let spread =
                (run.iter().map(|l| (l - mean).powi(2)).sum::<f32>() / run.len() as f32).sqrt();
            if spread <= MAX_STALL_SPREAD_DB {
                stalls.push(Stall {
                    start: start * frame,
                    end: (i * frame).min(self.samples.len()),
                    at_end: i == levels.len(),
                });
            }
        }
        stalls
    }

    /// Reads a WAV file as mono: 8, 16, 24 or 32 bit integer or 32 bit float, and several channels
    /// are averaged into one.
    pub fn read_wav(path: impl AsRef<Path>) -> Result<Self> {
        let mut reader = hound::WavReader::open(path)?;
        let spec = reader.spec();
        let channels = usize::from(spec.channels.max(1));
        let all: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader
                .samples::<f32>()
                .collect::<std::result::Result<_, _>>()?,
            hound::SampleFormat::Int => {
                let scale = 1.0 / (1_u64 << (spec.bits_per_sample.clamp(2, 32) - 1)) as f32;
                reader
                    .samples::<i32>()
                    .map(|s| s.map(|v| v as f32 * scale))
                    .collect::<std::result::Result<_, _>>()?
            }
        };
        let samples = all
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect();
        Ok(Self {
            samples,
            sample_rate: spec.sample_rate,
        })
    }

    /// Writes a 16-bit mono WAV file. A sample is scaled by 32768, the factor [`Audio::read_wav`]
    /// divides 16-bit samples by, so reading a 16-bit file and writing it again gives the same
    /// samples. 1.0 itself is one step past the largest 16-bit sample and is kept to it.
    pub fn write_wav(&self, path: impl AsRef<Path>) -> Result<()> {
        let mut writer = hound::WavWriter::create(path, self.wav_spec())?;
        self.write_samples(&mut writer)?;
        writer.finalize()?;
        Ok(())
    }

    /// The same WAV file [`Audio::write_wav`] writes, as bytes in memory.
    pub fn to_wav_bytes(&self) -> Result<Vec<u8>> {
        let mut cursor = std::io::Cursor::new(Vec::with_capacity(44 + self.samples.len() * 2));
        let mut writer = hound::WavWriter::new(&mut cursor, self.wav_spec())?;
        self.write_samples(&mut writer)?;
        writer.finalize()?;
        Ok(cursor.into_inner())
    }

    fn wav_spec(&self) -> hound::WavSpec {
        hound::WavSpec {
            channels: 1,
            sample_rate: self.sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        }
    }

    fn write_samples<W: std::io::Write + std::io::Seek>(
        &self,
        writer: &mut hound::WavWriter<W>,
    ) -> Result<()> {
        for s in &self.samples {
            // `as` saturates, so 32768.0 becomes 32767.
            writer.write_sample((s.clamp(-1.0, 1.0) * 32_768.0) as i16)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_has_the_right_length() {
        assert_eq!(Audio::silence(24_000, 250).samples.len(), 6_000);
    }

    #[test]
    fn append_joins_in_order() {
        let mut a = Audio {
            samples: vec![0.1],
            sample_rate: 8,
        };
        a.append(&Audio {
            samples: vec![0.2],
            sample_rate: 8,
        });
        assert_eq!(a.samples, vec![0.1, 0.2]);
    }

    #[test]
    fn wav_bytes_are_the_file_write_wav_writes() {
        let a = Audio {
            samples: vec![0.0, 0.5, -0.5, 1.0, -1.0],
            sample_rate: 8_000,
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        a.write_wav(&path).unwrap();
        assert_eq!(a.to_wav_bytes().unwrap(), std::fs::read(path).unwrap());
    }

    #[test]
    #[should_panic(expected = "sample rates differ")]
    fn append_refuses_other_rates() {
        let mut a = Audio::empty(8);
        a.append(&Audio::empty(16));
    }

    #[test]
    fn normalize_sets_the_peak() {
        let mut a = Audio {
            samples: vec![0.25, -0.5],
            sample_rate: 8,
        };
        a.normalize_peak(-6.0);
        let peak = a.samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.501_187).abs() < 1e-4, "peak was {peak}");
    }

    /// Builds test audio at 1 kHz, so a millisecond is a sample and a 50 ms frame is 50 samples.
    struct Signal {
        samples: Vec<f32>,
        seed: u32,
    }

    impl Signal {
        fn new() -> Self {
            Self {
                samples: Vec::new(),
                seed: 75,
            }
        }

        /// White noise between -`amp` and `amp`, the same every run.
        fn noise(mut self, ms: usize, amp: f32) -> Self {
            for _ in 0..ms {
                self.seed = self
                    .seed
                    .wrapping_mul(1_664_525)
                    .wrapping_add(1_013_904_223);
                let unit = (self.seed >> 8) as f32 / (1 << 24) as f32;
                self.samples.push((unit * 2.0 - 1.0) * amp);
            }
            self
        }

        /// Speech-like: 150 ms bursts around -15 dBFS with a 50 ms dip around -31 dBFS between.
        fn speech(mut self, ms: usize) -> Self {
            for _ in 0..ms / 200 {
                self = self.noise(150, 0.3).noise(50, 0.05);
            }
            self
        }

        /// Steady room noise around -55 dBFS, the kind the model made in issue #75.
        fn room_noise(self, ms: usize) -> Self {
            self.noise(ms, 0.003)
        }

        /// A natural pause: the noise fades from -45 to -75 dBFS in 50 ms steps.
        fn fade(mut self, ms: usize) -> Self {
            let steps = ms / 50;
            for step in 0..steps {
                let db = -45.0 - 30.0 * step as f32 / steps.max(1) as f32;
                self = self.noise(50, 10_f32.powf(db / 20.0) * 3_f32.sqrt());
            }
            self
        }

        fn audio(self) -> Audio {
            Audio {
                samples: self.samples,
                sample_rate: 1_000,
            }
        }
    }

    const PAUSE_MS: u32 = 250;

    #[test]
    fn a_stall_between_speech_becomes_a_sentence_pause() {
        let a = Signal::new()
            .speech(1_000)
            .room_noise(14_000)
            .speech(1_000)
            .audio();
        let out = a.collapse_stalls(PAUSE_MS);
        assert_eq!(out.samples.len(), 1_000 + 250 + 1_000);
        // Speech on both sides is untouched, and the pause is the start and end of the noise.
        assert_eq!(out.samples[..1_125], a.samples[..1_125]);
        assert_eq!(out.samples[1_125..], a.samples[a.samples.len() - 1_125..]);
        assert_eq!(out.sample_rate, a.sample_rate);
    }

    #[test]
    fn a_stall_after_the_last_speech_is_removed() {
        let a = Signal::new().speech(1_000).room_noise(3_000).audio();
        let out = a.collapse_stalls(PAUSE_MS);
        assert_eq!(out.samples, a.samples[..1_000]);
    }

    #[test]
    fn several_stalls_and_a_tail_are_all_collapsed() {
        let a = Signal::new()
            .speech(1_000)
            .room_noise(3_000)
            .speech(800)
            .fade(400)
            .speech(600)
            .room_noise(5_000)
            .speech(1_000)
            .room_noise(2_500)
            .audio();
        let stalls = a.find_stalls();
        let starts: Vec<_> = stalls.iter().map(|s| (s.start, s.end, s.at_end)).collect();
        assert_eq!(
            starts,
            [
                (1_000, 4_000, false),
                (5_800, 10_800, false),
                (11_800, 14_300, true)
            ]
        );
        let out = a.collapse_stalls(PAUSE_MS);
        // Every sample is accounted for: what went is the stalls, minus the pauses kept inside.
        let cut: usize = stalls.iter().map(|s| s.end - s.start).sum();
        let kept = 250 * stalls.iter().filter(|s| !s.at_end).count();
        assert_eq!(out.samples.len(), a.samples.len() - cut + kept);
        assert_eq!(out.seconds(), out.samples.len() as f32 / 1_000.0);
        assert!(out.find_stalls().is_empty());
    }

    #[test]
    fn a_quiet_stretch_under_the_cutoff_is_left_alone() {
        // 2.45 s of the same room noise, one frame short of the cutoff.
        let a = Signal::new()
            .speech(1_000)
            .room_noise(2_450)
            .speech(1_000)
            .room_noise(2_450)
            .audio();
        assert!(a.find_stalls().is_empty());
        assert_eq!(a.collapse_stalls(PAUSE_MS), a);
        // At the cutoff it is a stall.
        let b = Signal::new()
            .speech(1_000)
            .room_noise(2_500)
            .speech(1_000)
            .audio();
        assert_eq!(b.collapse_stalls(PAUSE_MS).samples.len(), 2_250);
    }

    #[test]
    fn natural_pauses_and_speech_are_left_alone() {
        let a = Signal::new()
            .speech(2_000)
            .fade(400)
            .speech(1_600)
            .fade(550)
            .speech(2_000)
            .fade(300)
            .audio();
        assert_eq!(a.collapse_stalls(PAUSE_MS), a);
    }

    #[test]
    fn quiet_before_the_first_word_is_left_alone() {
        let a = Signal::new().room_noise(3_000).speech(1_000).audio();
        assert_eq!(a.collapse_stalls(PAUSE_MS), a);
    }

    #[test]
    fn a_long_quiet_stretch_that_is_not_steady_is_left_alone() {
        // 3 s that switches between -50 and -75 dBFS every 50 ms: quiet, but not the model's
        // steady noise.
        let mut s = Signal::new().speech(1_000);
        for _ in 0..30 {
            s = s.noise(50, 0.005).noise(50, 0.0003);
        }
        let a = s.speech(1_000).audio();
        assert_eq!(a.collapse_stalls(PAUSE_MS), a);
    }

    #[test]
    fn silence_noise_and_nothing_are_left_alone() {
        let silent = Audio::silence(1_000, 5_000);
        assert_eq!(silent.collapse_stalls(PAUSE_MS), silent);
        let hiss = Signal::new().room_noise(5_000).audio();
        assert_eq!(hiss.collapse_stalls(PAUSE_MS), hiss);
        let empty = Audio::empty(1_000);
        assert_eq!(empty.collapse_stalls(PAUSE_MS), empty);
    }

    #[test]
    fn collapsing_is_deterministic_and_settles() {
        let a = Signal::new()
            .speech(1_000)
            .room_noise(6_000)
            .speech(1_000)
            .room_noise(3_000)
            .audio();
        let once = a.collapse_stalls(PAUSE_MS);
        assert_eq!(a.collapse_stalls(PAUSE_MS), once);
        assert_eq!(once.collapse_stalls(PAUSE_MS), once);
    }

    #[test]
    fn a_pause_longer_than_the_stall_keeps_the_whole_stall() {
        let a = Signal::new()
            .speech(1_000)
            .room_noise(3_000)
            .speech(1_000)
            .audio();
        assert_eq!(a.collapse_stalls(10_000), a);
    }

    /// The four voiceovers of the issue #75 hand test, read from `VERTELLER_ISSUE75_DIR` or the
    /// folder the hand test left them in. Each is a whole voiceover (its parts joined with the
    /// app's pauses) from before the dead air was cut, so the check runs over the whole file.
    #[test]
    fn the_issue_75_voiceovers() {
        let dir = std::env::var_os("VERTELLER_ISSUE75_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"D:\testing\verteller\final2"));
        let path = |name: &str| dir.join(name);
        let names = [
            "voiceover-original-cuda.wav",
            "voiceover-cleaned-cuda.wav",
            "voiceover-cpu.wav",
            "voiceover-directml.wav",
        ];
        if names.iter().any(|n| !path(n).is_file()) {
            eprintln!(
                "skipped: the issue #75 voiceovers are not in {} (set VERTELLER_ISSUE75_DIR)",
                dir.display()
            );
            return;
        }
        let seconds = |stalls: &[Stall], rate: u32| -> Vec<(f32, f32, bool)> {
            stalls
                .iter()
                .map(|s| {
                    let at = |i: usize| (i as f32 / rate as f32 * 100.0).round() / 100.0;
                    (at(s.start), at(s.end), s.at_end)
                })
                .collect()
        };

        // The original: the 16 s block in part 1 and a 3.85 s one in part 2. The 1.8 s stretch
        // at 35.0 s is under the cutoff and stays.
        let original = Audio::read_wav(path(names[0])).unwrap();
        let stalls = original.find_stalls();
        assert_eq!(
            seconds(&stalls, original.sample_rate),
            [(7.8, 23.8, false), (38.6, 42.45, false)]
        );
        let out = original.collapse_stalls(250);
        // 24 kHz: 16 s and 3.85 s cut, two 250 ms pauses kept.
        assert_eq!(
            out.samples.len(),
            original.samples.len() - 384_000 - 92_400 + 2 * 6_000
        );
        assert!((out.seconds() - 25.13).abs() < 0.01, "{}", out.seconds());
        assert!(out.find_stalls().is_empty());

        // The cleaned voice has dead air too, quieter (-64 dBFS): 3.1 s in part 1, 4.3 s in
        // part 2. Its 2.0 s stretch at 22.3 s, half digital silence, stays.
        let cleaned = Audio::read_wav(path(names[1])).unwrap();
        assert_eq!(
            seconds(&cleaned.find_stalls(), cleaned.sample_rate),
            [(7.65, 10.75, false), (26.35, 30.65, false)]
        );
        let out = cleaned.collapse_stalls(250);
        assert!((out.seconds() - 25.94).abs() < 0.01, "{}", out.seconds());

        // CPU and DirectML: one part with a 0.45 s pause, returned sample for sample.
        for name in &names[2..] {
            let a = Audio::read_wav(path(name)).unwrap();
            assert!(a.find_stalls().is_empty(), "{name}");
            assert_eq!(a.collapse_stalls(250), a, "{name}");
        }
    }

    #[test]
    fn normalize_leaves_silence_alone() {
        let mut a = Audio::silence(8, 1000);
        a.normalize_peak(-1.0);
        assert!(a.samples.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn read_wav_averages_channels_and_reads_float() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 8_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for s in [16_384_i16, 0, -16_384, -16_384] {
            w.write_sample(s).unwrap();
        }
        w.finalize().unwrap();
        let a = Audio::read_wav(&path).unwrap();
        assert_eq!(a.sample_rate, 8_000);
        assert_eq!(a.samples.len(), 2);
        assert!((a.samples[0] - 0.25).abs() < 1e-4 && (a.samples[1] + 0.5).abs() < 1e-4);

        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        w.write_sample(0.5_f32).unwrap();
        w.finalize().unwrap();
        assert_eq!(Audio::read_wav(&path).unwrap().samples, vec![0.5]);
    }

    #[test]
    fn reading_and_writing_a_16_bit_file_changes_no_sample() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("in.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 24_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let samples = [i16::MIN, -12_345, -2, -1, 0, 1, 2, 12_345, i16::MAX];
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for s in samples {
            w.write_sample(s).unwrap();
        }
        w.finalize().unwrap();
        let out = dir.path().join("out.wav");
        Audio::read_wav(&path).unwrap().write_wav(&out).unwrap();
        let mut reader = hound::WavReader::open(&out).unwrap();
        let again: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
        assert_eq!(again, samples);
        assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(&path).unwrap());
        // Full scale either way stays inside 16 bits.
        let loud = Audio {
            samples: vec![1.0, -1.0, 2.0, -2.0],
            sample_rate: 24_000,
        };
        loud.write_wav(&out).unwrap();
        let mut reader = hound::WavReader::open(&out).unwrap();
        let written: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
        assert_eq!(written, [i16::MAX, i16::MIN, i16::MAX, i16::MIN]);
    }

    #[test]
    fn wav_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let a = Audio {
            samples: vec![0.0, 0.5, -0.5],
            sample_rate: 24_000,
        };
        a.write_wav(&path).unwrap();
        let mut reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().sample_rate, 24_000);
        assert_eq!(reader.samples::<i16>().count(), 3);
    }
}
