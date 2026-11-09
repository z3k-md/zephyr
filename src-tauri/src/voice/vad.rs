//! Energy gate for hold-to-talk.
//!
//! The model is run on the whole utterance, so leftover silence makes it rewrite words
//! and sometimes invent a filler. This keeps speech, plus a short pad, and drops the rest
//! before those passes.
//!
//! The noise floor is the 15th percentile of the non-speech frames in this hold. Speech
//! is left out of that estimate, so a sentence that fills the hold cannot drag the floor
//! up and close the gate on itself. Until a few non-speech frames have been seen, a fixed
//! default is used. A frame also has to clear an absolute level: a dead mic, whose floor
//! would otherwise fall to the silence clamp, must not count its hiss as speech.
//!
//! Digital silence is not fed to the floor either. A stretch of pure zeros would pin the
//! percentile at the clamp and make ordinary room noise afterwards look like speech.

/// 16 kHz mono, the rate the capture buffer and the model share.
const SAMPLE_RATE: usize = 16_000;

/// Tuning. Change these by ear.
/// Frame width. 30 ms at 16 kHz is 480 samples.
const FRAME_MS: u32 = 30;
/// Assumed room level until [`MIN_NOISE_FRAMES`] non-speech frames have arrived.
/// Quiet speech (around −40 dBFS) still clears this plus [`SPEECH_MARGIN_DB`], so the
/// first words are not dropped while the floor warms up.
const DEFAULT_NOISE_FLOOR_DB: f32 = -52.0;
/// Non-speech frames required before the percentile replaces the default.
const MIN_NOISE_FRAMES: usize = 8;
/// Low percentile of the non-speech frames. A quiet minority stays the floor even
/// when most of the hold is louder noise.
const NOISE_PERCENTILE: f32 = 0.15;
/// How far above the floor a frame must sit to count as speech.
const SPEECH_MARGIN_DB: f32 = 11.0;
/// Absolute floor. Nothing quieter than this is speech, whatever the room estimate says.
const ABS_MIN_DBFS: f32 = -55.0;
/// RMS at or below this is treated as silence and kept out of the floor estimate.
const SILENCE_DBFS: f32 = -100.0;
/// Consecutive speech frames that open a segment. 3 × 30 ms = 90 ms.
const START_FRAMES: usize = 3;
/// Consecutive non-speech frames that close a segment. 10 × 30 ms = 300 ms.
const END_FRAMES: usize = 10;
/// Audio kept before each speech segment so an onset is not clipped.
const PAD_BEFORE_MS: u32 = 200;
/// Audio kept after each speech segment so a word ending is not clipped.
const PAD_AFTER_MS: u32 = 300;
/// Longest silence left between two segments. Longer gaps are cut down to this,
/// keeping the pad after the earlier segment and what remains of the pad before
/// the later one (300 ms + 100 ms when the constants above are in force).
const MAX_GAP_MS: u32 = 400;

const FRAME_SAMPLES: usize = samples_for_ms(FRAME_MS);

const fn samples_for_ms(ms: u32) -> usize {
    SAMPLE_RATE * ms as usize / 1_000
}

/// One detected run of speech, in samples of the hold buffer. `end` is exclusive
/// and stops at the last speech frame, not at the end of the hangover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeechSegment {
    pub start: usize,
    pub end: usize,
}

/// How much speech the gate has committed to. Silence changes it once, when the
/// hangover elapses and the segment closes: from then on its trailing pad is full,
/// so the model input stays the same until new speech arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpeechCursor {
    pub segments: usize,
    pub end: usize,
    pub open: bool,
}

/// Incremental energy gate over one hold. Feed it the growing 16 kHz buffer.
#[derive(Debug, Clone)]
pub struct SpeechGate {
    consumed: usize,
    /// Non-speech frame levels, sorted ascending, silence clamp excluded.
    noise_db: Vec<f32>,
    speech_run: usize,
    silence_run: usize,
    in_speech: bool,
    open_start: usize,
    open_end: usize,
    closed: Vec<SpeechSegment>,
}

impl Default for SpeechGate {
    fn default() -> Self {
        Self::new()
    }
}

impl SpeechGate {
    pub fn new() -> Self {
        Self {
            consumed: 0,
            noise_db: Vec::new(),
            speech_run: 0,
            silence_run: 0,
            in_speech: false,
            open_start: 0,
            open_end: 0,
            closed: Vec::new(),
        }
    }

    /// Classifies any new 30 ms frames in `buffer`. A shorter buffer than the one
    /// already consumed is ignored, so a capture slot that was just emptied cannot
    /// rewind the hold.
    pub fn observe(&mut self, buffer: &[f32]) {
        if buffer.len() < self.consumed {
            return;
        }
        while self.consumed + FRAME_SAMPLES <= buffer.len() {
            let start = self.consumed;
            let end = start + FRAME_SAMPLES;
            let db = frame_dbfs(&buffer[start..end]);
            self.consumed = end;
            self.push_frame(db, end);
        }
    }

    /// Closed segments plus the one still open, if the speaker has not yet paused
    /// out the hangover. The open segment ends at the last speech frame.
    pub fn segments(&self) -> Vec<SpeechSegment> {
        let mut segments = self.closed.clone();
        if self.in_speech && self.open_end > self.open_start {
            segments.push(SpeechSegment {
                start: self.open_start,
                end: self.open_end,
            });
        }
        segments
    }

    pub fn segment_count(&self) -> usize {
        self.closed.len() + usize::from(self.in_speech && self.open_end > self.open_start)
    }

    pub fn cursor(&self) -> SpeechCursor {
        if self.in_speech && self.open_end > self.open_start {
            SpeechCursor {
                segments: self.closed.len() + 1,
                end: self.open_end,
                open: true,
            }
        } else if let Some(last) = self.closed.last() {
            SpeechCursor {
                segments: self.closed.len(),
                end: last.end,
                open: false,
            }
        } else {
            SpeechCursor::default()
        }
    }

    /// Trimmed audio the model should see. Empty when nothing has been classified
    /// as speech. `buffer` should already have been passed to [`Self::observe`].
    pub fn model_input(&self, buffer: &[f32]) -> Vec<f32> {
        model_input(buffer, &self.segments())
    }

    pub fn model_input_len(&self, buffer_len: usize) -> usize {
        model_input_len(buffer_len, &self.segments())
    }

    fn push_frame(&mut self, db: f32, frame_end: usize) {
        let speech = is_speech(db, self.floor_db());
        if speech {
            self.silence_run = 0;
            self.speech_run += 1;
            if self.in_speech {
                self.open_end = frame_end;
            } else if self.speech_run >= START_FRAMES {
                self.in_speech = true;
                self.open_start = frame_end - self.speech_run * FRAME_SAMPLES;
                self.open_end = frame_end;
            }
            return;
        }
        self.speech_run = 0;
        // Pure zeros are not a room. Leaving them out keeps a muted stretch from
        // pinning the floor at the clamp.
        if db > SILENCE_DBFS + 1.0 {
            insert_sorted(&mut self.noise_db, db);
        }
        if !self.in_speech {
            return;
        }
        self.silence_run += 1;
        if self.silence_run >= END_FRAMES {
            self.closed.push(SpeechSegment {
                start: self.open_start,
                end: self.open_end,
            });
            self.in_speech = false;
            self.silence_run = 0;
        }
    }

    fn floor_db(&self) -> f32 {
        if self.noise_db.len() < MIN_NOISE_FRAMES {
            DEFAULT_NOISE_FLOOR_DB
        } else {
            self.noise_db[percentile_index(self.noise_db.len())]
        }
    }
}

/// Builds the model buffer: pad each segment, then keep at most [`MAX_GAP_MS`] of
/// the silence between two segments. Leading and trailing audio beyond the pads
/// is dropped. `segments` are in order and do not overlap.
pub fn model_input(buffer: &[f32], segments: &[SpeechSegment]) -> Vec<f32> {
    let ranges = kept_ranges(buffer.len(), segments);
    let mut out = Vec::with_capacity(ranges.iter().map(|(start, end)| end - start).sum());
    for (start, end) in ranges {
        out.extend_from_slice(&buffer[start..end]);
    }
    out
}

pub fn model_input_len(buffer_len: usize, segments: &[SpeechSegment]) -> usize {
    kept_ranges(buffer_len, segments)
        .into_iter()
        .map(|(start, end)| end - start)
        .sum()
}

/// Cheap identity of a model buffer. Pair it with the length; audio itself is not logged.
pub(crate) fn fingerprint(samples: &[f32]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for sample in samples {
        for byte in sample.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

fn kept_ranges(buffer_len: usize, segments: &[SpeechSegment]) -> Vec<(usize, usize)> {
    let pad_before = samples_for_ms(PAD_BEFORE_MS);
    let pad_after = samples_for_ms(PAD_AFTER_MS);
    let max_gap = samples_for_ms(MAX_GAP_MS);
    let mut ranges = Vec::new();
    let mut previous_end = None;
    for segment in segments {
        let start = segment.start.min(buffer_len);
        let end = segment.end.min(buffer_len);
        if end <= start {
            continue;
        }
        if let Some(previous) = previous_end {
            if start > previous {
                let gap = start - previous;
                if gap <= max_gap {
                    push_range(&mut ranges, previous, start);
                } else {
                    // Prefer the trailing pad (word endings). The rest of the budget
                    // is the leading pad of the next segment.
                    let head = pad_after.min(max_gap);
                    let tail = pad_before.min(max_gap.saturating_sub(head));
                    push_range(&mut ranges, previous, previous + head);
                    push_range(&mut ranges, start - tail, start);
                }
            }
        } else {
            push_range(&mut ranges, start.saturating_sub(pad_before), start);
        }
        push_range(&mut ranges, start, end);
        previous_end = Some(end);
    }
    if let Some(end) = previous_end {
        push_range(&mut ranges, end, (end + pad_after).min(buffer_len));
    }
    ranges
}

fn push_range(ranges: &mut Vec<(usize, usize)>, start: usize, end: usize) {
    if end > start {
        ranges.push((start, end));
    }
}

fn is_speech(db: f32, floor: f32) -> bool {
    db >= floor + SPEECH_MARGIN_DB && db >= ABS_MIN_DBFS
}

fn insert_sorted(values: &mut Vec<f32>, db: f32) {
    if !db.is_finite() {
        return;
    }
    let index = values.partition_point(|value| *value < db);
    values.insert(index, db);
}

fn percentile_index(len: usize) -> usize {
    if len <= 1 {
        return 0;
    }
    let rank = (NOISE_PERCENTILE * (len - 1) as f32).round();
    if rank <= 0.0 {
        0
    } else {
        (rank as usize).min(len - 1)
    }
}

/// RMS of one frame, in dBFS, clamped at the silence floor. Full scale is 0 dB.
pub(crate) fn frame_dbfs(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return SILENCE_DBFS;
    }
    let mut sum_sq = 0.0f32;
    for sample in frame {
        sum_sq += sample * sample;
    }
    let rms = (sum_sq / frame.len() as f32).sqrt();
    if rms <= 1.0e-5 {
        return SILENCE_DBFS;
    }
    (20.0 * rms.log10()).max(SILENCE_DBFS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn append(buffer: &mut Vec<f32>, frames: usize, db: f32) {
        let amp = 10f32.powf(db / 20.0);
        buffer.resize(buffer.len() + frames * FRAME_SAMPLES, amp);
    }

    fn silence(buffer: &mut Vec<f32>, frames: usize) {
        buffer.resize(buffer.len() + frames * FRAME_SAMPLES, 0.0);
    }

    fn run(buffer: &[f32]) -> SpeechGate {
        let mut gate = SpeechGate::new();
        gate.observe(buffer);
        gate
    }

    #[test]
    fn silence_is_not_speech() {
        let mut buffer = Vec::new();
        silence(&mut buffer, 70);
        let gate = run(&buffer);
        assert_eq!(gate.segment_count(), 0);
        assert!(gate.model_input(&buffer).is_empty());
        assert_eq!(gate.cursor(), SpeechCursor::default());
    }

    #[test]
    fn low_steady_noise_is_not_speech() {
        // −48 dBFS sits under the default threshold (−41) and, once it is the
        // floor, under floor + 11 dB as well.
        let mut buffer = Vec::new();
        append(&mut buffer, 70, -48.0);
        let gate = run(&buffer);
        assert_eq!(gate.segment_count(), 0);
        assert!(gate.model_input(&buffer).is_empty());
    }

    #[test]
    fn very_quiet_mic_is_not_speech() {
        let mut buffer = Vec::new();
        append(&mut buffer, 70, -70.0);
        assert_eq!(run(&buffer).segment_count(), 0);
    }

    #[test]
    fn noise_above_a_fallen_floor_but_under_the_absolute_minimum_is_not_speech() {
        // Floor learns −80. floor + margin is −69, so −60 would be speech without
        // the absolute minimum at −55.
        let mut buffer = Vec::new();
        append(&mut buffer, 12, -80.0);
        append(&mut buffer, 12, -60.0);
        append(&mut buffer, 12, -80.0);
        assert_eq!(run(&buffer).segment_count(), 0);
    }

    #[test]
    fn a_level_under_the_margin_is_not_speech() {
        let mut buffer = Vec::new();
        append(&mut buffer, 12, -48.0);
        append(&mut buffer, 12, -42.0);
        append(&mut buffer, 12, -48.0);
        assert_eq!(run(&buffer).segment_count(), 0);
    }

    #[test]
    fn a_tone_burst_over_a_noise_floor_is_speech_and_padding_is_kept() {
        let mut buffer = Vec::new();
        append(&mut buffer, 12, -48.0);
        let burst_at = buffer.len();
        append(&mut buffer, 8, -24.0);
        let burst_end = buffer.len();
        append(&mut buffer, 20, -48.0);

        let gate = run(&buffer);
        let segments = gate.segments();
        assert_eq!(
            segments,
            vec![SpeechSegment {
                start: burst_at,
                end: burst_end,
            }]
        );

        let pad_before = samples_for_ms(PAD_BEFORE_MS);
        let pad_after = samples_for_ms(PAD_AFTER_MS);
        let input = gate.model_input(&buffer);
        let expected = buffer[burst_at - pad_before..burst_end + pad_after].to_vec();
        assert_eq!(input, expected);
        assert_eq!(gate.model_input_len(buffer.len()), input.len());
        // Room tone before the pad, and the tail past the pad, are gone.
        assert!(input.len() < buffer.len());
    }

    #[test]
    fn a_sine_burst_over_noise_is_speech() {
        let mut buffer = Vec::new();
        append(&mut buffer, 12, -48.0);
        let start = buffer.len();
        let frames = 10;
        // RMS of a sine is amplitude / √2, so this sits near −20 dBFS.
        let amp = 10f32.powf(-20.0 / 20.0) * std::f32::consts::SQRT_2;
        for i in 0..frames * FRAME_SAMPLES {
            let t = i as f32 / SAMPLE_RATE as f32;
            buffer.push((2.0 * std::f32::consts::PI * 220.0 * t).sin() * amp);
        }
        let end = buffer.len();
        append(&mut buffer, 16, -48.0);
        assert_eq!(run(&buffer).segments(), vec![SpeechSegment { start, end }]);
    }

    #[test]
    fn speech_from_the_first_sample_is_kept() {
        let mut buffer = Vec::new();
        append(&mut buffer, 8, -24.0);
        let end = buffer.len();
        silence(&mut buffer, 12);
        let gate = run(&buffer);
        assert_eq!(gate.segments(), vec![SpeechSegment { start: 0, end }]);
    }

    #[test]
    fn a_short_click_does_not_open_a_segment() {
        let mut buffer = Vec::new();
        append(&mut buffer, 12, -48.0);
        append(&mut buffer, 1, -24.0);
        append(&mut buffer, 6, -48.0);
        append(&mut buffer, 2, -24.0);
        append(&mut buffer, 16, -48.0);
        assert_eq!(run(&buffer).segment_count(), 0);

        append(&mut buffer, 3, -24.0);
        let burst_at = buffer.len() - 3 * FRAME_SAMPLES;
        let burst_end = buffer.len();
        append(&mut buffer, 16, -48.0);
        let gate = run(&buffer);
        assert_eq!(
            gate.segments(),
            vec![SpeechSegment {
                start: burst_at,
                end: burst_end,
            }]
        );
    }

    #[test]
    fn a_short_pause_stays_one_segment_and_a_long_gap_collapses() {
        let mut buffer = Vec::new();
        append(&mut buffer, 10, -48.0);
        append(&mut buffer, 5, -24.0);
        // 6 frames = 180 ms, inside the 300 ms hangover.
        append(&mut buffer, 6, -48.0);
        append(&mut buffer, 5, -24.0);
        append(&mut buffer, 16, -48.0);
        let bridged = run(&buffer);
        assert_eq!(bridged.segment_count(), 1);
        let only = bridged.segments()[0];
        assert_eq!(only.end - only.start, (5 + 6 + 5) * FRAME_SAMPLES);

        let mut split = Vec::new();
        append(&mut split, 10, -48.0);
        let first = split.len();
        append(&mut split, 6, -24.0);
        let first_end = split.len();
        // 20 frames = 600 ms, past the hangover, and longer than the 400 ms cap.
        append(&mut split, 20, -48.0);
        let second = split.len();
        append(&mut split, 6, -24.0);
        let second_end = split.len();
        append(&mut split, 16, -48.0);

        let gate = run(&split);
        assert_eq!(
            gate.segments(),
            vec![
                SpeechSegment {
                    start: first,
                    end: first_end,
                },
                SpeechSegment {
                    start: second,
                    end: second_end,
                },
            ]
        );
        let input = gate.model_input(&split);
        let span =
            (second_end + samples_for_ms(PAD_AFTER_MS)) - (first - samples_for_ms(PAD_BEFORE_MS));
        assert!(input.len() < span);
        assert_eq!(input.len(), gate.model_input_len(split.len()));
        // Speech samples themselves are all present.
        assert!(
            input
                .windows(first_end - first)
                .any(|window| window == &split[first..first_end])
        );
        assert!(
            input
                .windows(second_end - second)
                .any(|window| window == &split[second..second_end])
        );
    }

    #[test]
    fn model_input_pads_edges_and_caps_the_gap() {
        // Index-valued samples, so a wrong slice shows up as a wrong number.
        let buffer: Vec<f32> = (0..24_000).map(|index| index as f32).collect();
        let segments = [
            SpeechSegment {
                start: 3_200,
                end: 6_400,
            },
            SpeechSegment {
                start: 16_000,
                end: 19_200,
            },
        ];
        let expected: Vec<f32> = (0..3_200)
            .chain(3_200..6_400)
            .chain(6_400..11_200)
            .chain(14_400..16_000)
            .chain(16_000..19_200)
            .chain(19_200..24_000)
            .map(|index| index as f32)
            .collect();
        assert_eq!(model_input(&buffer, &segments), expected);
        assert_eq!(model_input_len(buffer.len(), &segments), expected.len());
        assert!(model_input(&buffer, &[]).is_empty());
    }

    #[test]
    fn a_gap_shorter_than_the_cap_is_kept_whole() {
        let buffer: Vec<f32> = (0..12_000).map(|index| index as f32).collect();
        let segments = [
            SpeechSegment {
                start: 0,
                end: 4_800,
            },
            SpeechSegment {
                start: 6_400,
                end: 9_600,
            },
        ];
        // Gap is 1_600 samples, under 400 ms. Trail pad runs into the buffer end.
        assert_eq!(model_input(&buffer, &segments), buffer);
    }

    #[test]
    fn trailing_silence_stops_changing_the_model_input() {
        let mut buffer = Vec::new();
        append(&mut buffer, 20, -24.0);
        // Just past the 300 ms pad.
        silence(&mut buffer, 11);
        let gate = run(&buffer);
        let stable = gate.model_input(&buffer);
        let cursor = gate.cursor();
        assert!(!stable.is_empty());

        silence(&mut buffer, 40);
        let mut later = gate.clone();
        later.observe(&buffer);
        assert_eq!(later.cursor(), cursor);
        assert_eq!(later.model_input(&buffer), stable);
        assert_eq!(
            fingerprint(&stable),
            fingerprint(&later.model_input(&buffer))
        );
    }

    #[test]
    fn closing_the_hangover_moves_the_cursor_once() {
        let mut buffer = Vec::new();
        append(&mut buffer, 6, -24.0);
        silence(&mut buffer, 9);
        let mut gate = SpeechGate::new();
        gate.observe(&buffer);
        let open = gate.cursor();
        assert_eq!(open.segments, 1);
        assert!(open.open);
        silence(&mut buffer, 1);
        gate.observe(&buffer);
        let closed = gate.cursor();
        assert_eq!((closed.segments, closed.end), (open.segments, open.end));
        assert!(!closed.open);
        assert_eq!(gate.segment_count(), 1);
        let settled = gate.model_input(&buffer);
        silence(&mut buffer, 30);
        gate.observe(&buffer);
        assert_eq!(gate.cursor(), closed);
        assert_eq!(gate.model_input(&buffer), settled);
    }

    #[test]
    fn incremental_observe_matches_one_shot() {
        let mut buffer = Vec::new();
        append(&mut buffer, 12, -48.0);
        append(&mut buffer, 8, -24.0);
        silence(&mut buffer, 14);
        append(&mut buffer, 6, -24.0);
        silence(&mut buffer, 20);
        let once = run(&buffer);

        let mut chunked = SpeechGate::new();
        let mut at = 0;
        // Uneven chunks, including cuts in the middle of a frame.
        for step in [100usize, 479, 480, 50, 1_000, 9_600] {
            if at >= buffer.len() {
                break;
            }
            at = (at + step).min(buffer.len());
            chunked.observe(&buffer[..at]);
        }
        while at < buffer.len() {
            at = (at + 700).min(buffer.len());
            chunked.observe(&buffer[..at]);
        }
        assert_eq!(chunked.segments(), once.segments());
        assert_eq!(chunked.model_input(&buffer), once.model_input(&buffer));
    }

    #[test]
    fn a_shorter_buffer_does_not_rewind() {
        let mut buffer = Vec::new();
        append(&mut buffer, 8, -24.0);
        let mut gate = SpeechGate::new();
        gate.observe(&buffer);
        let cursor = gate.cursor();
        gate.observe(&buffer[..100]);
        assert_eq!(gate.cursor(), cursor);
        silence(&mut buffer, 20);
        gate.observe(&buffer);
        assert_eq!(gate.cursor().end, cursor.end);
    }

    #[test]
    fn fingerprint_changes_when_a_sample_changes() {
        let audio = vec![0.1f32, -0.2, 0.3, 0.0];
        assert_eq!(fingerprint(&audio), fingerprint(&audio));
        let mut nudged = audio.clone();
        nudged[2] = 0.31;
        assert_ne!(fingerprint(&audio), fingerprint(&nudged));
        assert_ne!(fingerprint(&audio), fingerprint(&audio[..3]));
    }

    #[test]
    fn frame_energy_matches_a_constant_level() {
        let frame = vec![0.1f32; FRAME_SAMPLES];
        let db = frame_dbfs(&frame);
        assert!((db - (-20.0)).abs() < 0.05, "{db}");
        assert_eq!(frame_dbfs(&vec![0.0; FRAME_SAMPLES]), SILENCE_DBFS);
    }
}
