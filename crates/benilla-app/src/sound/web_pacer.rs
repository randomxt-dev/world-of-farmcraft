//! The Web Audio scheduler's clock math ([`super::web_backend`]), apart from Web Audio so it tests
//! natively: how far ahead to queue, where the next chunk starts, and when the queue ran dry.
//!
//! The browser mixes on the main thread between frames, so the queue must outlast the longest
//! frame: it is held at 1.5x the recent worst frame, never under [`MIN_LEAD`]. A queue that ran
//! dry restarts [`MIN_LEAD`] past the clock, after everything already queued, so chunks never
//! overlap (cpal's webaudio host kept its cursor and stacked late chunks on top of each other).

/// One scheduled `AudioBufferSourceNode`, in frames: ~10.7 ms at 48 kHz.
pub(super) const CHUNK_FRAMES: u64 = 512;
/// The least queued ahead of the context clock, and where a dry queue restarts: room for the
/// start messages to reach the audio thread.
const MIN_LEAD: f64 = 0.030;
/// The most queued ahead: past this a slow frame costs more latency than it saves gaps.
const MAX_LEAD: f64 = 0.250;
/// Queued lead per second of recent worst frame.
const LEAD_PER_FRAME: f64 = 1.5;
/// Per-frame decay of the remembered worst frame, ~4 s from a 200 ms hitch back to 60 fps.
const PEAK_DECAY: f64 = 0.99;
/// A cursor closer to the clock than this counts as behind: its start message would land late.
const BEHIND: f64 = 0.005;

/// The queue's clock: frames are context sample frames, times are `AudioContext.currentTime`.
pub(super) struct Pacer {
    rate: f64,
    /// Where the next chunk starts; `None` before the first chunk or after [`Pacer::reset`].
    cursor: Option<u64>,
    /// The recent worst frame time, decaying.
    peak_dt: f64,
}

/// One pump's work: render `chunks` chunks back to back from frame `start`.
#[derive(Debug, PartialEq)]
pub(super) struct Plan {
    pub(super) start: u64,
    pub(super) chunks: u64,
    /// The queue had run dry (or nearly) and restarts past the clock.
    pub(super) resynced: bool,
    /// How far the queue reached past the clock before this pump; negative when it had run dry.
    pub(super) headroom: Option<f64>,
}

impl Pacer {
    pub(super) fn new(rate: u32) -> Self {
        Self {
            rate: f64::from(rate),
            cursor: None,
            peak_dt: 0.0,
        }
    }

    /// Note one frame's wall time.
    pub(super) fn observe_frame(&mut self, dt: f64) {
        self.peak_dt = dt.max(self.peak_dt * PEAK_DECAY);
    }

    /// How far ahead of the clock the queue is filled, in seconds.
    pub(super) fn lead(&self) -> f64 {
        (self.peak_dt * LEAD_PER_FRAME).clamp(MIN_LEAD, MAX_LEAD)
    }

    /// The clock stopped (a suspended context): the next plan starts afresh, not as a gap.
    pub(super) fn reset(&mut self) {
        self.cursor = None;
    }

    /// Plan a pump at context time `now` and advance the cursor past it.
    pub(super) fn plan(&mut self, now: f64) -> Plan {
        let headroom = self.cursor.map(|c| c as f64 / self.rate - now);
        let (start, resynced) = match self.cursor {
            Some(c) if c as f64 / self.rate >= now + BEHIND => (c, false),
            // Restarting later than `c` keeps everything queued intact: no overlap.
            prev => (((now + MIN_LEAD) * self.rate).ceil() as u64, prev.is_some()),
        };
        let target = ((now + self.lead()) * self.rate).ceil() as u64;
        let chunks = target.saturating_sub(start).div_ceil(CHUNK_FRAMES);
        self.cursor = Some(start + chunks * CHUNK_FRAMES);
        Plan {
            start,
            chunks,
            resynced,
            headroom,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;
    const CHUNK_SECS: f64 = CHUNK_FRAMES as f64 / RATE as f64;

    fn end(p: &Plan) -> f64 {
        (p.start + p.chunks * CHUNK_FRAMES) as f64 / f64::from(RATE)
    }

    #[test]
    fn steady_60fps_never_runs_dry() {
        let mut pacer = Pacer::new(RATE);
        let mut now = 0.0;
        for i in 0..600 {
            pacer.observe_frame(1.0 / 60.0);
            let plan = pacer.plan(now);
            assert!(!plan.resynced, "frame {i}");
            assert!(end(&plan) >= now + MIN_LEAD, "frame {i}");
            assert!(end(&plan) < now + MIN_LEAD + CHUNK_SECS + 1e-9, "frame {i}");
            now += 1.0 / 60.0;
        }
    }

    #[test]
    fn a_slow_frame_rate_queues_past_the_next_frame() {
        let mut pacer = Pacer::new(RATE);
        let mut now = 0.0;
        for i in 0..100 {
            pacer.observe_frame(0.1);
            let plan = pacer.plan(now);
            assert!(!plan.resynced, "frame {i}");
            // The next pump is 100 ms away; the queue must reach past it.
            assert!(end(&plan) >= now + 0.15, "frame {i}");
            now += 0.1;
        }
    }

    #[test]
    fn a_dry_queue_restarts_after_what_is_queued() {
        let mut pacer = Pacer::new(RATE);
        pacer.observe_frame(1.0 / 60.0);
        let first = pacer.plan(0.0);
        let queued_end = first.start + first.chunks * CHUNK_FRAMES;
        // A 400 ms hitch, longer than any lead.
        pacer.observe_frame(0.4);
        let late = pacer.plan(0.4);
        assert!(late.resynced);
        assert!(late.headroom.unwrap() < 0.0);
        assert!(late.start >= queued_end, "chunks overlap");
        assert!(late.start as f64 / f64::from(RATE) >= 0.4 + MIN_LEAD);
        assert!(end(&late) >= 0.4 + MAX_LEAD);
    }

    #[test]
    fn near_the_clock_counts_as_behind() {
        let mut pacer = Pacer::new(RATE);
        let first = pacer.plan(0.0);
        let cursor = end(&first);
        let plan = pacer.plan(cursor - BEHIND / 2.0);
        assert!(plan.resynced);
        assert!(plan.start as f64 / f64::from(RATE) > cursor);
    }

    #[test]
    fn a_suspended_clock_restarts_without_a_gap() {
        let mut pacer = Pacer::new(RATE);
        pacer.plan(0.0);
        pacer.reset();
        let plan = pacer.plan(5.0);
        assert!(!plan.resynced);
        assert_eq!(plan.headroom, None);
    }

    #[test]
    fn the_lead_is_bounded_and_decays() {
        let mut pacer = Pacer::new(RATE);
        assert_eq!(pacer.lead(), MIN_LEAD);
        pacer.observe_frame(2.0);
        assert_eq!(pacer.lead(), MAX_LEAD);
        for _ in 0..1000 {
            pacer.observe_frame(1.0 / 60.0);
        }
        assert!(pacer.lead() < MIN_LEAD + 1e-3);
    }
}
