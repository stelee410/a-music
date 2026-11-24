//! Web instruments: plug-ins of the web player, written in JavaScript (web/instruments/, AGBW.md §6).
//!
//! In a WebAssembly build the page hosting the engine provides the functions of module `agb_host`
//! below. Everywhere else no web instrument is available, and a patch that asks for one falls back.

use crate::NoteEvent;

/// Engine id of a patch played by a web instrument.
pub const ENGINE_ID: &str = "plugin.web";
/// Inside the engine a web instrument is named `web:<id>`, apart from the names of VST 3 bundles.
pub const PREFIX: &str = "web:";

#[cfg(target_arch = "wasm32")]
mod host {
    #[link(wasm_import_module = "agb_host")]
    extern "C" {
        pub fn plugin_has(id: *const u8, len: usize) -> i32;
        /// A handle (≥ 0), or a negative number when the instrument cannot be started.
        pub fn plugin_open(id: *const u8, len: usize, instrument: i32, sample_rate: u32) -> i32;
        /// Index of a parameter, or −1.
        pub fn plugin_param(handle: i32, key: *const u8, len: usize) -> i32;
        pub fn plugin_has_param(id: *const u8, len: usize, key: *const u8, key_len: usize) -> i32;
        pub fn plugin_set(handle: i32, param: i32, value: f64);
        pub fn plugin_takes_audio(handle: i32) -> i32;
        pub fn plugin_note(handle: i32, on: i32, pitch: i32, velocity: f32);
        /// Mono, in place: an effect reads `len` samples at `ptr`, an instrument finds zeros.
        pub fn plugin_process(handle: i32, ptr: *mut f32, len: usize);
        pub fn plugin_reset(handle: i32);
        pub fn plugin_close(handle: i32);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::missing_safety_doc)]
mod host {
    pub unsafe fn plugin_has(_: *const u8, _: usize) -> i32 {
        0
    }
    pub unsafe fn plugin_open(_: *const u8, _: usize, _: i32, _: u32) -> i32 {
        -1
    }
    pub unsafe fn plugin_param(_: i32, _: *const u8, _: usize) -> i32 {
        -1
    }
    pub unsafe fn plugin_has_param(_: *const u8, _: usize, _: *const u8, _: usize) -> i32 {
        0
    }
    pub unsafe fn plugin_set(_: i32, _: i32, _: f64) {}
    pub unsafe fn plugin_takes_audio(_: i32) -> i32 {
        0
    }
    pub unsafe fn plugin_note(_: i32, _: i32, _: i32, _: f32) {}
    pub unsafe fn plugin_process(_: i32, _: *mut f32, _: usize) {}
    pub unsafe fn plugin_reset(_: i32) {}
    pub unsafe fn plugin_close(_: i32) {}
}

/// Whether the host has the instrument `id`.
pub fn available(id: &str) -> bool {
    unsafe { host::plugin_has(id.as_ptr(), id.len()) != 0 }
}

/// The first of `keys` that names no parameter of the instrument.
pub fn unknown_param<'a>(id: &str, mut keys: impl Iterator<Item = &'a String>) -> Option<&'a String> {
    keys.find(|k| unsafe { host::plugin_has_param(id.as_ptr(), id.len(), k.as_ptr(), k.len()) == 0 })
}

/// One running web instrument or effect.
pub struct Instance {
    handle: i32,
}

impl Instance {
    pub fn open(id: &str, instrument: bool, sample_rate: u32) -> Result<Instance, String> {
        let handle = unsafe { host::plugin_open(id.as_ptr(), id.len(), instrument as i32, sample_rate) };
        if handle < 0 {
            return Err(format!("web instrument {id:?} is not available in this player"));
        }
        Ok(Instance { handle })
    }

    pub fn has_audio_input(&self) -> bool {
        unsafe { host::plugin_takes_audio(self.handle) != 0 }
    }

    pub fn param_id(&self, key: &str) -> Option<u32> {
        let i = unsafe { host::plugin_param(self.handle, key.as_ptr(), key.len()) };
        (i >= 0).then_some(i as u32)
    }

    pub fn set_param(&mut self, id: u32, value: f64) {
        unsafe { host::plugin_set(self.handle, id as i32, value) }
    }

    pub fn reset(&mut self) {
        unsafe { host::plugin_reset(self.handle) }
    }

    /// Play `io.len()` samples; each note takes effect exactly at its offset.
    pub fn process(&mut self, io: &mut [f32], events: &[NoteEvent]) {
        let mut at = 0usize;
        for e in events {
            let offset = e.offset.min(io.len());
            if offset > at {
                unsafe { host::plugin_process(self.handle, io[at..offset].as_mut_ptr(), offset - at) };
                at = offset;
            }
            unsafe { host::plugin_note(self.handle, e.on as i32, e.pitch as i32, e.velocity) };
        }
        if io.len() > at {
            unsafe { host::plugin_process(self.handle, io[at..].as_mut_ptr(), io.len() - at) };
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe { host::plugin_close(self.handle) }
    }
}
