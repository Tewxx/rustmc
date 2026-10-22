//! Software mixer feeding a WinMM `waveOut` device: 44.1 kHz stereo 16-bit, a few short
//! buffers in flight (~70 ms), voices resampled with linear interpolation for pitch.
use crate::vorbis::Decoded;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub const RATE: u32 = 44100;
const FRAMES: usize = 1024;
const BUFFERS: usize = 4;

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

#[cfg(windows)]
fn start_output(shared: Arc<Mutex<Shared>>, stop: Arc<AtomicBool>) -> bool {
    use windows_sys::Win32::Media::Audio::*;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("Sound Library".into())
        .spawn(move || unsafe {
            let fmt = WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_PCM as u16,
                nChannels: 2,
                nSamplesPerSec: RATE,
                nAvgBytesPerSec: RATE * 4,
                nBlockAlign: 4,
                wBitsPerSample: 16,
                cbSize: 0,
            };
            let mut h: HWAVEOUT = std::ptr::null_mut();
            if waveOutOpen(&mut h, WAVE_MAPPER, &fmt, 0, 0, CALLBACK_NULL) != 0 {
                let _ = tx.send(false);
                return;
            }
            let _ = tx.send(true);
            let mut bufs: Vec<Box<[i16; FRAMES * 2]>> = (0..BUFFERS).map(|_| Box::new([0i16; FRAMES * 2])).collect();
            let mut hdrs: Vec<Box<WAVEHDR>> = Vec::new();
            for b in bufs.iter_mut() {
                let mut hdr: Box<WAVEHDR> = Box::new(std::mem::zeroed());
                hdr.lpData = b.as_mut_ptr() as *mut u8;
                hdr.dwBufferLength = (FRAMES * 4) as u32;
                waveOutPrepareHeader(h, &mut *hdr, std::mem::size_of::<WAVEHDR>() as u32);
                hdr.dwFlags |= WHDR_DONE;
                hdrs.push(hdr);
            }
            while !stop.load(Ordering::Relaxed) {
                let mut wrote = false;
                for (i, hdr) in hdrs.iter_mut().enumerate() {
                    if hdr.dwFlags & WHDR_DONE != 0 {
                        mix(&shared, &mut bufs[i][..]);
                        hdr.dwFlags &= !WHDR_DONE;
                        waveOutWrite(h, &mut **hdr, std::mem::size_of::<WAVEHDR>() as u32);
                        wrote = true;
                    }
                }
                if !wrote {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
            waveOutReset(h);
            for hdr in hdrs.iter_mut() {
                waveOutUnprepareHeader(h, &mut **hdr, std::mem::size_of::<WAVEHDR>() as u32);
            }
            waveOutClose(h);
        })
        .ok();
    rx.recv().unwrap_or(false)
}

#[cfg(not(windows))]
fn start_output(_shared: Arc<Mutex<Shared>>, _stop: Arc<AtomicBool>) -> bool {
    false
}
