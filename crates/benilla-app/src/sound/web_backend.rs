//! wasm32: the mixer's kira backend in the browser — Web Audio, fed from the frame.
//!
//! The browser has no audio thread for us: whatever renders the mix runs on the main thread,
//! between frames. cpal's webaudio host rendered from two ping-pong `onended` callbacks, so at a
//! low frame rate a new sound waited a frame plus two buffers, and a frame longer than both left
//! a gap its never-resynced cursor then stacked chunks over. This backend renders at the end of
//! every frame ([`pump_frame`], in `Last`, after every system that plays a sound) and from each
//! chunk's `ended` between frames, queueing [`web_pacer`](super::web_pacer)'s lead ahead of the
//! context clock in [`CHUNK_FRAMES`]-frame `AudioBufferSourceNode`s.

use std::cell::RefCell;
use std::rc::Rc;

use bevy::prelude::*;
use kira::backend::{Backend, Renderer};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{AudioContext, AudioContextState};

use super::web_pacer::{Pacer, Plan, CHUNK_FRAMES};
use super::SoundOutput;

/// kira mixes stereo.
const CHANNELS: u16 = 2;
/// A frame longer than this drained cpal's old 2048-frame buffer at 48 kHz: the report counts them.
const LONG_FRAME_SECS: f64 = 0.043;
/// The queue report's window, in context seconds.
const REPORT_SECS: f64 = 30.0;

/// The page's audio context, opened by [`open`]; `None` when the page has no Web Audio.
pub(super) type WebBackendSettings = Option<AudioContext>;

/// Open the page's [`AudioContext`] at the output's own rate, so nothing resamples; the rate also
/// sizes the limiter and the mix tap. It starts suspended until the page's first gesture
/// (`web/index.html` resumes it).
pub(super) fn open() -> (WebBackendSettings, Option<u32>) {
    match AudioContext::new() {
        Ok(ctx) => {
            let rate = ctx.sample_rate() as u32;
            info!("audio: Web Audio at {rate} Hz, {CHUNK_FRAMES}-frame chunks, paced per frame");
            (Some(ctx), Some(rate))
        }
        Err(e) => {
            warn!("audio: no Web Audio context: {e:?}");
            (None, None)
        }
    }
}

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Last, pump_frame);
}

/// Render this frame's plays at its end, queued past the next frame.
fn pump_frame(mut out: NonSendMut<SoundOutput>, time: Res<Time<Real>>) {
    if let Some(mixer) = out.mixer.as_mut() {
        mixer.pump_web(time.delta_secs_f64());
    }
}

enum State {
    Empty,
    Ready(AudioContext),
    Running(Rc<RefCell<Scheduler>>),
}

/// kira's [`Backend`] over the page's [`AudioContext`].
pub(super) struct WebBackend {
    state: State,
}

impl WebBackend {
    /// One frame's pump; `frame_dt` is its wall time in seconds.
    pub(super) fn pump(&mut self, frame_dt: f64) {
        if let State::Running(scheduler) = &self.state {
            if let Ok(mut scheduler) = scheduler.try_borrow_mut() {
                scheduler.pump(Some(frame_dt));
            }
        }
    }
}

impl Backend for WebBackend {
    type Settings = WebBackendSettings;
    type Error = anyhow::Error;

    fn setup(
        settings: Self::Settings,
        _internal_buffer_size: usize,
    ) -> Result<(Self, u32), Self::Error> {
        let ctx = settings.ok_or_else(|| anyhow::anyhow!("no Web Audio context"))?;
        let rate = ctx.sample_rate() as u32;
        Ok((
            Self {
                state: State::Ready(ctx),
            },
            rate,
        ))
    }

    fn start(&mut self, renderer: Renderer) -> Result<(), Self::Error> {
        let State::Ready(ctx) = std::mem::replace(&mut self.state, State::Empty) else {
            anyhow::bail!("the audio backend was started twice");
        };
        let rate = ctx.sample_rate() as u32;
        let scheduler = Rc::new_cyclic(|weak: &std::rc::Weak<RefCell<Scheduler>>| {
            let weak = weak.clone();
            // Between frames a finished chunk tops the queue up; a no-op once the backend drops.
            let on_ended = Closure::<dyn FnMut()>::new(move || {
                if let Some(scheduler) = weak.upgrade() {
                    if let Ok(mut scheduler) = scheduler.try_borrow_mut() {
                        scheduler.pump(None);
                    }
                }
            })
            .into_js_value()
            .unchecked_into::<js_sys::Function>();
            let frames = CHUNK_FRAMES as usize;
            RefCell::new(Scheduler {
                ctx,
                renderer,
                pacer: Pacer::new(rate),
                rate,
                interleaved: vec![0.0; frames * usize::from(CHANNELS)],
                planar: vec![0.0; frames],
                on_ended,
                stats: Stats::default(),
                warned: false,
            })
        });
        self.state = State::Running(scheduler);
        Ok(())
    }
}

/// The renderer and the queue it feeds.
struct Scheduler {
    ctx: AudioContext,
    renderer: Renderer,
    pacer: Pacer,
    rate: u32,
    interleaved: Vec<f32>,
    planar: Vec<f32>,
    /// Every chunk's `onended`, the between-frames pump.
    on_ended: js_sys::Function,
    stats: Stats,
    /// A Web Audio call failed once already; later failures stay quiet.
    warned: bool,
}

impl Scheduler {
    /// Fill the queue to the pacer's lead; `frame_dt` is `None` for an `ended` pump.
    fn pump(&mut self, frame_dt: Option<f64>) {
        if let Some(dt) = frame_dt {
            self.pacer.observe_frame(dt);
            self.stats.frame(dt);
        }
        // Suspended until the first gesture: no clock to queue against, so nothing renders.
        if self.ctx.state() != AudioContextState::Running {
            self.pacer.reset();
            return;
        }
        let now = self.ctx.current_time();
        let plan = self.pacer.plan(now);
        self.stats
            .plan(&plan, self.pacer.lead(), frame_dt.is_none());
        for i in 0..plan.chunks {
            let when = (plan.start + i * CHUNK_FRAMES) as f64 / f64::from(self.rate);
            if let Err(e) = self.schedule(when) {
                if !std::mem::replace(&mut self.warned, true) {
                    warn!("audio: Web Audio refused a chunk: {e:?}");
                }
                break;
            }
        }
        self.stats.report(now);
    }

    /// Render one chunk and start it at context time `when`.
    fn schedule(&mut self, when: f64) -> Result<(), JsValue> {
        self.renderer.on_start_processing();
        self.renderer.process(&mut self.interleaved, CHANNELS);
        let buffer =
            self.ctx
                .create_buffer(CHANNELS.into(), CHUNK_FRAMES as u32, self.rate as f32)?;
        for channel in 0..usize::from(CHANNELS) {
            let frames = self.interleaved.chunks_exact(usize::from(CHANNELS));
            for (sample, frame) in self.planar.iter_mut().zip(frames) {
                *sample = frame[channel];
            }
            buffer.copy_to_channel(&self.planar, channel as i32)?;
        }
        let source = self.ctx.create_buffer_source()?;
        source.set_buffer(Some(&buffer));
        source.connect_with_audio_node(&self.ctx.destination())?;
        // The base class's setter: the subclass's own is deprecated in web-sys.
        AsRef::<web_sys::AudioScheduledSourceNode>::as_ref(&source)
            .set_onended(Some(&self.on_ended));
        source.start_with_when(when)
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        let _ = self.ctx.close();
    }
}

/// The queue's numbers over one report window: what a stutter report needs.
#[derive(Default)]
struct Stats {
    since: Option<f64>,
    frames: u32,
    long_frames: u32,
    max_frame: f64,
    ended_pumps: u32,
    chunks: u64,
    max_lead: f64,
    min_headroom: Option<f64>,
    resyncs: u32,
}

impl Stats {
    fn frame(&mut self, dt: f64) {
        self.frames += 1;
        self.max_frame = self.max_frame.max(dt);
        if dt > LONG_FRAME_SECS {
            self.long_frames += 1;
        }
    }

    fn plan(&mut self, plan: &Plan, lead: f64, ended: bool) {
        self.chunks += plan.chunks;
        self.max_lead = self.max_lead.max(lead);
        self.ended_pumps += u32::from(ended);
        if plan.resynced {
            self.resyncs += 1;
        }
        if let Some(h) = plan.headroom {
            self.min_headroom = Some(self.min_headroom.map_or(h, |m| m.min(h)));
        }
    }

    /// Log and restart the window once it spans [`REPORT_SECS`] of context time.
    fn report(&mut self, now: f64) {
        let since = *self.since.get_or_insert(now);
        if now - since < REPORT_SECS {
            return;
        }
        let headroom = self
            .min_headroom
            .map_or_else(|| "n/a".to_string(), |h| format!("{:.1} ms", h * 1000.0));
        let line = format!(
            "{} frames, {} over {:.0} ms (max {:.0} ms); queue lead ≤ {:.0} ms, headroom ≥ \
             {headroom}; {} chunks, {} from `ended`",
            self.frames,
            self.long_frames,
            LONG_FRAME_SECS * 1000.0,
            self.max_frame * 1000.0,
            self.max_lead * 1000.0,
            self.chunks,
            self.ended_pumps,
        );
        if self.resyncs > 0 {
            warn!(
                "audio: the Web Audio queue ran dry {} time(s) in {:.0} s (a gap each): {line}",
                self.resyncs,
                now - since,
            );
        } else {
            info!("audio: Web Audio queue over {:.0} s: {line}", now - since);
        }
        *self = Self {
            since: Some(now),
            ..Self::default()
        };
    }
}
