//! The sound output of the jam page: Android's AAudio, asked for its low-latency path, which calls
//! back for every few milliseconds of sound. (What the app plays through `AudioTrack` is heard a
//! tenth of a second late on a handheld; an instrument under a finger cannot be.)

use crate::jam::SHARED;
use std::ffi::c_void;
use std::sync::Mutex;

#[repr(C)]
struct Builder {
    _private: [u8; 0],
}
#[repr(C)]
struct Stream {
    _private: [u8; 0],
}

const FORMAT_I16: i32 = 1;
const LOW_LATENCY: i32 = 12;
const USAGE_GAME: i32 = 14;

#[link(name = "aaudio")]
extern "C" {
    fn AAudio_createStreamBuilder(builder: *mut *mut Builder) -> i32;
    fn AAudioStreamBuilder_setFormat(builder: *mut Builder, format: i32);
    fn AAudioStreamBuilder_setChannelCount(builder: *mut Builder, channels: i32);
    fn AAudioStreamBuilder_setSampleRate(builder: *mut Builder, rate: i32);
    fn AAudioStreamBuilder_setPerformanceMode(builder: *mut Builder, mode: i32);
    fn AAudioStreamBuilder_setUsage(builder: *mut Builder, usage: i32);
    fn AAudioStreamBuilder_setDataCallback(builder: *mut Builder, callback: extern "C" fn(*mut Stream, *mut c_void, *mut c_void, i32) -> i32, user: *mut c_void);
    fn AAudioStreamBuilder_openStream(builder: *mut Builder, stream: *mut *mut Stream) -> i32;
    fn AAudioStreamBuilder_delete(builder: *mut Builder) -> i32;
    fn AAudioStream_requestStart(stream: *mut Stream) -> i32;
    fn AAudioStream_requestStop(stream: *mut Stream) -> i32;
    fn AAudioStream_close(stream: *mut Stream) -> i32;
    fn AAudioStream_getSampleRate(stream: *mut Stream) -> i32;
    fn AAudioStream_getFramesPerBurst(stream: *mut Stream) -> i32;
    fn AAudioStream_setBufferSizeInFrames(stream: *mut Stream, frames: i32) -> i32;
    fn AAudioStream_getXRunCount(stream: *mut Stream) -> i32;
}

struct Open(*mut Stream);
// SAFETY: the stream is only used through AAudio's functions, which may be called from any thread.
unsafe impl Send for Open {}

static STREAM: Mutex<Option<Open>> = Mutex::new(None);

/// Called by the output for the next `frames` of sound, two channels of 16 bits.
extern "C" fn fill(_stream: *mut Stream, _user: *mut c_void, data: *mut c_void, frames: i32) -> i32 {
    let frames = frames.max(0) as usize;
    let mut mono = [0f32; 1024];
    // SAFETY: AAudio hands over room for `frames` frames of two 16-bit samples.
    let out = unsafe { std::slice::from_raw_parts_mut(data as *mut i16, frames * 2) };
    let mut done = 0;
    while done < frames {
        let n = (frames - done).min(mono.len());
        SHARED.render(&mut mono[..n]);
        for i in 0..n {
            let v = (mono[i] * 32767.0) as i16;
            out[2 * (done + i)] = v;
            out[2 * (done + i) + 1] = v;
        }
        done += n;
    }
    0
}

/// Open the output and start it. The rate it runs at, or a negative number when it could not.
pub fn start() -> i32 {
    let mut open = STREAM.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = open.as_ref() {
        // SAFETY: the stream is open.
        return unsafe { AAudioStream_getSampleRate(s.0) };
    }
    // SAFETY: the builder and the stream are used as AAudio's documentation says, and only here.
    unsafe {
        let mut builder: *mut Builder = std::ptr::null_mut();
        if AAudio_createStreamBuilder(&mut builder) != 0 || builder.is_null() {
            return -1;
        }
        AAudioStreamBuilder_setFormat(builder, FORMAT_I16);
        AAudioStreamBuilder_setChannelCount(builder, 2);
        AAudioStreamBuilder_setSampleRate(builder, 48000);
        AAudioStreamBuilder_setPerformanceMode(builder, LOW_LATENCY);
        AAudioStreamBuilder_setUsage(builder, USAGE_GAME);
        AAudioStreamBuilder_setDataCallback(builder, fill, std::ptr::null_mut());
        let mut stream: *mut Stream = std::ptr::null_mut();
        let r = AAudioStreamBuilder_openStream(builder, &mut stream);
        AAudioStreamBuilder_delete(builder);
        if r != 0 || stream.is_null() {
            return r.min(-1);
        }
        let rate = AAudioStream_getSampleRate(stream);
        // two of the output's own bursts: as little as plays without gaps
        AAudioStream_setBufferSizeInFrames(stream, AAudioStream_getFramesPerBurst(stream) * 2);
        {
            let mut jam = SHARED.jam.lock().unwrap_or_else(|e| e.into_inner());
            if jam.is_none() {
                *jam = Some(crate::jam::Jam::new(rate.max(8000) as u32));
            }
        }
        if AAudioStream_requestStart(stream) != 0 {
            AAudioStream_close(stream);
            return -2;
        }
        *open = Some(Open(stream));
        rate
    }
}

pub fn stop() {
    let mut open = STREAM.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = open.take() {
        // SAFETY: the stream is open, and no longer after this.
        unsafe {
            AAudioStream_requestStop(s.0);
            AAudioStream_close(s.0);
        }
    }
}

/// How often the output ran dry, and how many frames it takes at a time (0, 0 when it is not open).
pub fn health() -> (i32, i32) {
    let open = STREAM.lock().unwrap_or_else(|e| e.into_inner());
    match open.as_ref() {
        // SAFETY: the stream is open.
        Some(s) => unsafe { (AAudioStream_getXRunCount(s.0), AAudioStream_getFramesPerBurst(s.0)) },
        None => (0, 0),
    }
}
