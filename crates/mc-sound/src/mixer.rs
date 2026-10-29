//! Software mixer feeding a `cpal` output stream: mixed as 44.1 kHz stereo, converted to the
//! device's rate/format/channels in the callback; voices resampled with linear interpolation for pitch.
use crate::vorbis::Decoded;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub const RATE: u32 = 44100;
const FRAMES: usize = 1024;

struct Voice {
    id: u64,
    pcm: Arc<Decoded>,
    pos: f64,
    step: f64,
    gain: (f32, f32),
    looping: bool,
}

#[derive(Default)]
struct Shared {
    voices: Vec<Voice>,
}

#[derive(Clone)]
pub struct Mixer {
    shared: Arc<Mutex<Shared>>,
    next: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    /// Whether an output device opened.
    pub available: bool,
}

impl Mixer {
    pub fn new() -> Mixer {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let available = start_output(shared.clone(), stop.clone());
        Mixer { shared, next: Arc::new(AtomicU64::new(1)), stop, available }
    }

    /// Starts a voice; `pitch` scales the playback rate, gains are per output channel.
    pub fn play(&self, pcm: Arc<Decoded>, pitch: f32, gain: (f32, f32), looping: bool) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let step = pcm.rate as f64 / RATE as f64 * pitch as f64;
        self.shared.lock().unwrap().voices.push(Voice { id, pcm, pos: 0.0, step, gain, looping });
        id
    }

    pub fn set_gain(&self, id: u64, gain: (f32, f32)) {
        if let Some(v) = self.shared.lock().unwrap().voices.iter_mut().find(|v| v.id == id) {
            v.gain = gain;
        }
    }

    pub fn stop(&self, id: u64) {
        self.shared.lock().unwrap().voices.retain(|v| v.id != id);
    }

    pub fn stop_all(&self) {
        self.shared.lock().unwrap().voices.clear();
    }

    pub fn is_playing(&self, id: u64) -> bool {
        self.shared.lock().unwrap().voices.iter().any(|v| v.id == id)
    }

    pub fn voice_count(&self) -> usize {
        self.shared.lock().unwrap().voices.len()
    }
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}

impl Mixer {
    /// Stops the output thread.
    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Mixes `FRAMES` stereo frames from the active voices, dropping finished ones.
fn mix(shared: &Mutex<Shared>, out: &mut [i16]) {
    let mut acc = [0f32; FRAMES * 2];
    {
        let mut s = shared.lock().unwrap();
        s.voices.retain_mut(|v| {
            let ch = v.pcm.channels.max(1);
            let frames = v.pcm.samples.len() / ch;
            if frames == 0 {
                return false;
            }
            for f in 0..FRAMES {
                let mut i = v.pos as usize;
                if i + 1 >= frames {
                    if v.looping {
                        v.pos -= (frames - 1) as f64;
                        i = v.pos as usize;
                    } else {
                        return false;
                    }
                }
                let t = (v.pos - i as f64) as f32;
                let at = |k: usize, c: usize| v.pcm.samples[k * ch + c.min(ch - 1)];
                let l = at(i, 0) + (at(i + 1, 0) - at(i, 0)) * t;
                let r = at(i, 1) + (at(i + 1, 1) - at(i, 1)) * t;
                acc[f * 2] += l * v.gain.0;
                acc[f * 2 + 1] += r * v.gain.1;
                v.pos += v.step;
            }
            true
        });
    }
    for (o, a) in out.iter_mut().zip(acc.iter()) {
        *o = (a.clamp(-1.0, 1.0) * 32767.0) as i16;
    }
}

/// Pulls stereo frames from `pull` (at `RATE`) and yields them at the device rate, linearly interpolated.
struct Resampler<F: FnMut() -> (f32, f32)> {
    pull: F,
    step: f64,
    frac: f64,
    cur: (f32, f32),
    nxt: (f32, f32),
}

impl<F: FnMut() -> (f32, f32)> Resampler<F> {
    fn new(mut pull: F, dev_rate: u32) -> Self {
        let (cur, nxt) = (pull(), pull());
        Resampler { pull, step: RATE as f64 / dev_rate as f64, frac: 0.0, cur, nxt }
    }

    fn next(&mut self) -> (f32, f32) {
        let t = self.frac as f32;
        let out = (self.cur.0 + (self.nxt.0 - self.cur.0) * t, self.cur.1 + (self.nxt.1 - self.cur.1) * t);
        self.frac += self.step;
        while self.frac >= 1.0 {
            self.frac -= 1.0;
            self.cur = self.nxt;
            self.nxt = (self.pull)();
        }
        out
    }
}

/// Fills an interleaved device buffer: channel 0/1 get left/right, any others silence.
fn fill<T: cpal::SizedSample + cpal::FromSample<f32>>(data: &mut [T], channels: usize, rs: &mut impl FnMut() -> (f32, f32)) {
    for frame in data.chunks_mut(channels) {
        let (l, r) = rs();
        for (c, o) in frame.iter_mut().enumerate() {
            *o = T::from_sample(match c {
                0 => l,
                1 => r,
                _ => 0.0,
            });
        }
        if channels == 1 {
            frame[0] = T::from_sample((l + r) * 0.5);
        }
    }
}

fn build<T: cpal::SizedSample + cpal::FromSample<f32>>(
    dev: &cpal::Device,
    cfg: &cpal::StreamConfig,
    shared: Arc<Mutex<Shared>>,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    let channels = cfg.channels as usize;
    let mut buf = [0i16; FRAMES * 2];
    let mut idx = FRAMES;
    let mut rs = Resampler::new(
        move || {
            if idx == FRAMES {
                mix(&shared, &mut buf);
                idx = 0;
            }
            idx += 1;
            (buf[(idx - 1) * 2] as f32 / 32767.0, buf[(idx - 1) * 2 + 1] as f32 / 32767.0)
        },
        cfg.sample_rate.0,
    );
    dev.build_output_stream(cfg, move |d: &mut [T], _| fill(d, channels, &mut || rs.next()), |e| eprintln!("audio: {e}"), None)
}

/// Opens the default output device on a thread that owns the stream until `stop` is set.
fn start_output(shared: Arc<Mutex<Shared>>, stop: Arc<AtomicBool>) -> bool {
    use cpal::SampleFormat as F;
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("Sound Library".into()).spawn(move || {
        let open = || -> Option<cpal::Stream> {
            let dev = cpal::default_host().default_output_device()?;
            // Prefer a stereo config that natively supports RATE, else the device default.
            let rate = cpal::SampleRate(RATE);
            let want = dev
                .supported_output_configs()
                .ok()?
                .filter(|c| c.channels() == 2 && matches!(c.sample_format(), F::F32 | F::I16))
                .find(|c| c.min_sample_rate() <= rate && rate <= c.max_sample_rate())
                .map(|c| c.with_sample_rate(rate));
            let sup = want.or_else(|| dev.default_output_config().ok())?;
            let fmt = sup.sample_format();
            let cfg = sup.config();
            let stream = match fmt {
                F::F32 => build::<f32>(&dev, &cfg, shared),
                F::I16 => build::<i16>(&dev, &cfg, shared),
                F::U16 => build::<u16>(&dev, &cfg, shared),
                F::I32 => build::<i32>(&dev, &cfg, shared),
                _ => return None,
            }
            .ok()?;
            stream.play().ok()?;
            if std::env::var_os("MC_SOUND_DEBUG").is_some() {
                eprintln!("audio: {fmt:?} {} ch @ {} Hz", cfg.channels, cfg.sample_rate.0);
            }
            Some(stream)
        };
        let stream = open();
        let _ = tx.send(stream.is_some());
        while stream.is_some() && !stop.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    });
    spawned.is_ok() && rx.recv().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_upsamples_linearly() {
        let mut n = 0f32;
        // Source ramp 0,1,2,... at RATE; device at 2*RATE -> half steps.
        let mut r = Resampler::new(|| { n += 1.0; (n - 1.0, 0.0) }, RATE * 2);
        let got: Vec<f32> = (0..5).map(|_| r.next().0).collect();
        assert_eq!(got, vec![0.0, 0.5, 1.0, 1.5, 2.0]);
    }

    #[test]
    fn fill_converts_formats_and_channels() {
        let mut d = [0i16; 6];
        fill(&mut d, 3, &mut || (1.0, -1.0));
        assert_eq!(d, [32767, -32767, 0, 32767, -32767, 0]);
        let mut m = [0f32; 2];
        fill(&mut m, 1, &mut || (1.0, 0.0));
        assert_eq!(m, [0.5, 0.5]);
    }
}
