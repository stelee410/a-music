//! VST 3 plug-ins as sound sources and effects (FORMAT.md §8.6).
//!
//! A plug-in is found by the name of its bundle (`Dexed` → `Dexed.vst3`) in the usual folders (never
//! by a path: what gets loaded is native code, and only what the user installed may be), loaded
//! into this process, and driven block by block without its editor: notes and parameter values go in,
//! audio comes out. The engine is mono, so a mono signal is fed to every input channel and the output
//! channels are averaged.
//!
//! Loading is implemented for macOS. Elsewhere [`available`] is `false` and a song plays the patch's
//! fallback.

#![allow(non_snake_case, non_upper_case_globals)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComWrapper, Interface};

/// Engine id of a track played by a plug-in.
pub mod web;

pub const ENGINE_ID: &str = "plugin.vst3";
/// Largest block handed to a plug-in.
pub const BLOCK: usize = 512;

/// One audio class of a plug-in bundle.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassInfo {
    pub name: String,
    pub vendor: String,
    /// 32 hex digits.
    pub uid: String,
    /// `Instrument|Synth`, `Fx|Distortion`, …
    pub sub_categories: String,
    cid: TUID,
}

impl ClassInfo {
    pub fn is_instrument(&self) -> bool {
        self.sub_categories.contains("Instrument")
    }
}

/// One parameter of a plug-in. Values are normalised: 0…1.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamInfo {
    pub id: u32,
    pub title: String,
    pub units: String,
    /// 0 = continuous, n = n + 1 positions.
    pub steps: i32,
    pub default: f64,
    /// The default as the plug-in prints it (`"440 Hz"`).
    pub default_text: String,
}

/// A note going to a plug-in, `offset` samples into the buffer being processed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoteEvent {
    pub offset: usize,
    pub on: bool,
    pub pitch: u8,
    pub velocity: f32,
    pub id: i32,
}

// ---------------------------------------------------------------------------------------------
// Finding bundles

/// Folders searched for `.vst3` bundles: `AGB_VST3_PATH` (separated by `:`), then the user's and the
/// system's plug-in folders.
pub fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = vec![];
    if let Ok(p) = std::env::var("AGB_VST3_PATH") {
        dirs.extend(p.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    }
    if cfg!(target_os = "macos") {
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(Path::new(&home).join("Library/Audio/Plug-Ins/VST3"));
        }
        dirs.push("/Library/Audio/Plug-Ins/VST3".into());
    }
    dirs
}

fn bundles_in(dir: &Path, depth: u8, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("vst3")) {
            out.push(p);
        } else if depth > 0 && p.is_dir() {
            bundles_in(&p, depth - 1, out); // vendors keep their plug-ins in a folder of their own
        }
    }
}

/// Every `.vst3` bundle in the search folders. Nothing is loaded.
pub fn bundles() -> Vec<PathBuf> {
    let mut out = vec![];
    for d in search_dirs() {
        bundles_in(&d, 2, &mut out);
    }
    out
}

/// The bundle a name refers to: the bundle's file name without the extension, in the search folders
/// (letter case does not matter). A name is never taken as a path: a song must not be able to make
/// the player load code from wherever it likes. With `AGB_NO_PLUGINS` set nothing is found: a way to
/// hear what listeners without the plug-ins get.
pub fn find_bundle(name: &str) -> Option<PathBuf> {
    if std::env::var_os("AGB_NO_PLUGINS").is_some_and(|v| !v.is_empty() && v != "0") {
        return None;
    }
    if name.is_empty() || name.contains(['/', '\\']) {
        return None;
    }
    bundles().into_iter().find(|b| b.file_stem().is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case(name)))
}

// ---------------------------------------------------------------------------------------------
// Loading

#[cfg(target_os = "macos")]
mod sys {
    use std::ffi::{c_char, c_void, CString};
    use std::path::Path;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFURLCreateFromFileSystemRepresentation(alloc: *const c_void, buf: *const u8, len: isize, is_dir: u8) -> *const c_void;
        fn CFBundleCreate(alloc: *const c_void, url: *const c_void) -> *mut c_void;
        fn CFBundleLoadExecutable(bundle: *mut c_void) -> u8;
        fn CFBundleGetFunctionPointerForName(bundle: *mut c_void, name: *const c_void) -> *mut c_void;
        fn CFStringCreateWithCString(alloc: *const c_void, s: *const c_char, encoding: u32) -> *const c_void;
        fn CFRelease(x: *const c_void);
    }
    const UTF8: u32 = 0x0800_0100;

    unsafe fn symbol(bundle: *mut c_void, name: &str) -> *mut c_void {
        let c = CString::new(name).unwrap();
        let s = CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8);
        let f = CFBundleGetFunctionPointerForName(bundle, s);
        CFRelease(s);
        f
    }

    /// Load the bundle, call its entry point and return its `GetPluginFactory`. The bundle stays
    /// loaded for the life of the process.
    pub unsafe fn open(path: &Path) -> Result<*mut c_void, String> {
        let bytes = path.to_string_lossy().into_owned().into_bytes();
        let url = CFURLCreateFromFileSystemRepresentation(std::ptr::null(), bytes.as_ptr(), bytes.len() as isize, 1);
        if url.is_null() {
            return Err("not a usable path".into());
        }
        let bundle = CFBundleCreate(std::ptr::null(), url);
        CFRelease(url);
        if bundle.is_null() {
            return Err("not a bundle".into());
        }
        if CFBundleLoadExecutable(bundle) == 0 {
            return Err("its code could not be loaded (wrong processor type, or blocked by the system)".into());
        }
        let entry = symbol(bundle, "bundleEntry");
        if !entry.is_null() {
            let entry: unsafe extern "C" fn(*mut c_void) -> bool = std::mem::transmute(entry);
            if !entry(bundle) {
                return Err("its entry point refused to start".into());
            }
        }
        let factory = symbol(bundle, "GetPluginFactory");
        if factory.is_null() {
            return Err("it has no GetPluginFactory".into());
        }
        let factory: unsafe extern "system" fn() -> *mut c_void = std::mem::transmute(factory);
        let f = factory();
        if f.is_null() {
            return Err("it returned no factory".into());
        }
        Ok(f)
    }
}

#[cfg(not(target_os = "macos"))]
mod sys {
    use std::ffi::c_void;
    use std::path::Path;
    pub unsafe fn open(_path: &Path) -> Result<*mut c_void, String> {
        Err("plug-ins are loaded on macOS only in this build".into())
    }
}

struct Module {
    factory: ComPtr<IPluginFactory>,
    classes: Vec<ClassInfo>,
}
// The factory is only used under the lock of `modules()`, or by an `Instance` that is itself used by
// one thread at a time.
unsafe impl Send for Module {}
unsafe impl Sync for Module {}

fn modules() -> &'static Mutex<HashMap<PathBuf, Result<Arc<Module>, String>>> {
    static M: OnceLock<Mutex<HashMap<PathBuf, Result<Arc<Module>, String>>>> = OnceLock::new();
    M.get_or_init(Default::default)
}

fn chars8(s: &[std::ffi::c_char]) -> String {
    let b: Vec<u8> = s.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
    String::from_utf8_lossy(&b).into_owned()
}

fn chars16(s: &[TChar]) -> String {
    let b: Vec<u16> = s.iter().take_while(|c| **c != 0).map(|c| *c as u16).collect();
    String::from_utf16_lossy(&b)
}

fn hex(t: &TUID) -> String {
    t.iter().map(|b| format!("{:02X}", *b as u8)).collect()
}

fn load(path: &Path) -> Result<Arc<Module>, String> {
    let mut map = modules().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(m) = map.get(path) {
        return m.clone();
    }
    let r = unsafe { load_module(path) }.map(Arc::new);
    map.insert(path.to_path_buf(), r.clone());
    r
}

unsafe fn load_module(path: &Path) -> Result<Module, String> {
    let raw = sys::open(path)?;
    let factory = ComPtr::<IPluginFactory>::from_raw(raw as *mut IPluginFactory).ok_or("no factory")?;
    let factory2 = factory.cast::<IPluginFactory2>();
    let mut classes = vec![];
    for i in 0..factory.countClasses() {
        let mut info: PClassInfo = std::mem::zeroed();
        if factory.getClassInfo(i, &mut info) != kResultOk || chars8(&info.category) != "Audio Module Class" {
            continue;
        }
        let (mut vendor, mut sub) = (String::new(), String::new());
        if let Some(f2) = &factory2 {
            let mut i2: PClassInfo2 = std::mem::zeroed();
            if f2.getClassInfo2(i, &mut i2) == kResultOk {
                vendor = chars8(&i2.vendor);
                sub = chars8(&i2.subCategories);
            }
        }
        if vendor.is_empty() {
            let mut fi: PFactoryInfo = std::mem::zeroed();
            if factory.getFactoryInfo(&mut fi) == kResultOk {
                vendor = chars8(&fi.vendor);
            }
        }
        classes.push(ClassInfo { name: chars8(&info.name), vendor, uid: hex(&info.cid), sub_categories: sub, cid: info.cid });
    }
    if classes.is_empty() {
        return Err("it contains no audio plug-in".into());
    }
    Ok(Module { factory, classes })
}

/// The audio classes of a plug-in. Loads the bundle.
pub fn classes(name: &str) -> Result<Vec<ClassInfo>, String> {
    let path = find_bundle(name).ok_or_else(|| not_found(name))?;
    classes_at(&path)
}

/// The audio classes of the bundle at `path` (one of [`bundles`]). Loads it.
pub fn classes_at(path: &Path) -> Result<Vec<ClassInfo>, String> {
    load(path).map(|m| m.classes.clone()).map_err(|e| format!("plug-in {}: {e}", path.display()))
}

fn not_found(name: &str) -> String {
    let dirs: Vec<String> = search_dirs().iter().map(|d| d.display().to_string()).collect();
    format!("plug-in {name:?} is not installed (looked for {name}.vst3 in {})", dirs.join(", "))
}

/// Whether the plug-in can be loaded on this machine.
pub fn available(name: &str, uid: Option<&str>) -> bool {
    if let Some(id) = name.strip_prefix(web::PREFIX) {
        return web::available(id);
    }
    classes(name).is_ok_and(|c| uid.map_or(true, |u| c.iter().any(|k| k.uid.eq_ignore_ascii_case(u))))
}

/// The parameters of a plug-in. It is started once to read them; later calls answer from memory.
pub fn params_of(name: &str, uid: Option<&str>) -> Result<Arc<Vec<ParamInfo>>, String> {
    type Known = Mutex<HashMap<(String, Option<String>), Result<Arc<Vec<ParamInfo>>, String>>>;
    static K: OnceLock<Known> = OnceLock::new();
    let key = (name.to_string(), uid.map(str::to_string));
    let mut known = K.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(r) = known.get(&key) {
        return r.clone();
    }
    let instrument = classes(name)?.iter().any(|c| c.is_instrument());
    let r = Instance::open(name, uid, instrument, 48000).map(|i| Arc::new(i.params().to_vec()));
    known.insert(key, r.clone());
    r
}

/// The first of `keys` that names no parameter of the plug-in (a title, or `#<id>`).
pub fn unknown_param<'a>(name: &str, uid: Option<&str>, keys: impl Iterator<Item = &'a String>) -> Result<Option<&'a String>, String> {
    if let Some(id) = name.strip_prefix(web::PREFIX) {
        return Ok(web::unknown_param(id, keys));
    }
    let params = params_of(name, uid)?;
    let known = |k: &str| match k.strip_prefix('#') {
        Some(n) => n.parse::<u32>().is_ok_and(|id| params.iter().any(|p| p.id == id)),
        None => params.iter().any(|p| p.title.eq_ignore_ascii_case(k)),
    };
    Ok(keys.into_iter().find(|k| !known(k)))
}

// ---------------------------------------------------------------------------------------------
// What the host hands to a plug-in

struct Host;

impl Class for Host {
    type Interfaces = (IHostApplication, IComponentHandler);
}

impl IHostApplicationTrait for Host {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        if name.is_null() {
            return kInvalidArgument;
        }
        let dst = &mut *name;
        dst.fill(0);
        for (d, s) in dst.iter_mut().take(127).zip("Agent Groovebox".encode_utf16()) {
            *d = s as TChar;
        }
        kResultOk
    }
    unsafe fn createInstance(&self, _cid: *mut TUID, _iid: *mut TUID, obj: *mut *mut c_void) -> tresult {
        if !obj.is_null() {
            *obj = std::ptr::null_mut();
        }
        kNotImplemented // no messages between the two halves of a plug-in: it is driven without its editor
    }
}

impl IComponentHandlerTrait for Host {
    unsafe fn beginEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn performEdit(&self, _id: ParamID, _value: ParamValue) -> tresult {
        kResultOk
    }
    unsafe fn endEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn restartComponent(&self, _flags: int32) -> tresult {
        kResultOk
    }
}

/// A stream over bytes in memory.
struct MemStream {
    data: RefCell<Vec<u8>>,
    pos: Cell<usize>,
}

impl Class for MemStream {
    type Interfaces = (IBStream,);
}

impl IBStreamTrait for MemStream {
    unsafe fn read(&self, buffer: *mut c_void, num: int32, read: *mut int32) -> tresult {
        let data = self.data.borrow();
        let pos = self.pos.get().min(data.len());
        let n = (num.max(0) as usize).min(data.len() - pos);
        if n > 0 && !buffer.is_null() {
            std::ptr::copy_nonoverlapping(data.as_ptr().add(pos), buffer as *mut u8, n);
        }
        self.pos.set(pos + n);
        if !read.is_null() {
            *read = n as int32;
        }
        kResultOk
    }
    unsafe fn write(&self, buffer: *mut c_void, num: int32, written: *mut int32) -> tresult {
        let n = num.max(0) as usize;
        let mut data = self.data.borrow_mut();
        let pos = self.pos.get();
        if data.len() < pos + n {
            data.resize(pos + n, 0);
        }
        if n > 0 && !buffer.is_null() {
            std::ptr::copy_nonoverlapping(buffer as *const u8, data.as_mut_ptr().add(pos), n);
        }
        self.pos.set(pos + n);
        if !written.is_null() {
            *written = n as int32;
        }
        kResultOk
    }
    unsafe fn seek(&self, pos: int64, mode: int32, result: *mut int64) -> tresult {
        let len = self.data.borrow().len() as i64;
        let to = match mode {
            1 => self.pos.get() as i64 + pos,
            2 => len + pos,
            _ => pos,
        };
        self.pos.set(to.clamp(0, len.max(to)) as usize);
        if !result.is_null() {
            *result = self.pos.get() as int64;
        }
        kResultOk
    }
    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        if !pos.is_null() {
            *pos = self.pos.get() as int64;
        }
        kResultOk
    }
}

fn stream(bytes: Vec<u8>) -> (ComWrapper<MemStream>, ComPtr<IBStream>) {
    let w = ComWrapper::new(MemStream { data: RefCell::new(bytes), pos: Cell::new(0) });
    let p = w.to_com_ptr::<IBStream>().unwrap();
    (w, p)
}

#[derive(Default)]
struct EventList {
    events: RefCell<Vec<Event>>,
}

impl Class for EventList {
    type Interfaces = (IEventList,);
}

impl IEventListTrait for EventList {
    unsafe fn getEventCount(&self) -> int32 {
        self.events.borrow().len() as int32
    }
    unsafe fn getEvent(&self, index: int32, e: *mut Event) -> tresult {
        match self.events.borrow().get(index as usize) {
            Some(ev) if !e.is_null() => {
                *e = *ev;
                kResultOk
            }
            _ => kResultFalse,
        }
    }
    unsafe fn addEvent(&self, _e: *mut Event) -> tresult {
        kResultOk // what a plug-in sends out is not used
    }
}

/// One parameter's value for a block: a single point at its start.
struct Queue {
    id: ParamID,
    value: Cell<f64>,
}

impl Class for Queue {
    type Interfaces = (IParamValueQueue,);
}

impl IParamValueQueueTrait for Queue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id
    }
    unsafe fn getPointCount(&self) -> int32 {
        1
    }
    unsafe fn getPoint(&self, index: int32, offset: *mut int32, value: *mut ParamValue) -> tresult {
        if index != 0 || offset.is_null() || value.is_null() {
            return kResultFalse;
        }
        *offset = 0;
        *value = self.value.get();
        kResultOk
    }
    unsafe fn addPoint(&self, _offset: int32, value: ParamValue, index: *mut int32) -> tresult {
        self.value.set(value);
        if !index.is_null() {
            *index = 0;
        }
        kResultOk
    }
}

#[derive(Default)]
struct Changes {
    queues: RefCell<Vec<ComPtr<IParamValueQueue>>>,
}

impl Changes {
    fn push(&self, id: ParamID, value: f64) {
        let q = ComWrapper::new(Queue { id, value: Cell::new(value) }).to_com_ptr::<IParamValueQueue>().unwrap();
        self.queues.borrow_mut().push(q);
    }
}

impl Class for Changes {
    type Interfaces = (IParameterChanges,);
}

impl IParameterChangesTrait for Changes {
    unsafe fn getParameterCount(&self) -> int32 {
        self.queues.borrow().len() as int32
    }
    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        self.queues.borrow().get(index as usize).map_or(std::ptr::null_mut(), |q| q.as_ptr())
    }
    unsafe fn addParameterData(&self, id: *const ParamID, index: *mut int32) -> *mut IParamValueQueue {
        if id.is_null() {
            return std::ptr::null_mut();
        }
        self.push(*id, 0.0);
        let q = self.queues.borrow();
        if !index.is_null() {
            *index = q.len() as int32 - 1;
        }
        q.last().unwrap().as_ptr()
    }
}

// ---------------------------------------------------------------------------------------------
// A `.vstpreset` file

/// The processor's and the controller's state stored in a `.vstpreset` file.
pub fn parse_vstpreset(b: &[u8]) -> Result<(Vec<u8>, Option<Vec<u8>>), String> {
    let bad = || "not a .vstpreset file".to_string();
    if b.len() < 48 || &b[0..4] != b"VST3" {
        return Err(bad());
    }
    let i64_at = |o: usize| b.get(o..o + 8).map(|x| i64::from_le_bytes(x.try_into().unwrap()) as usize);
    let list = i64_at(40).ok_or_else(bad)?;
    if b.get(list..list + 4) != Some(b"List") {
        return Err(bad());
    }
    let count = b.get(list + 4..list + 8).map(|x| i32::from_le_bytes(x.try_into().unwrap())).ok_or_else(bad)?;
    let (mut comp, mut cont) = (None, None);
    for k in 0..count.max(0) as usize {
        let e = list + 8 + k * 20;
        let id = b.get(e..e + 4).ok_or_else(bad)?;
        let (off, size) = (i64_at(e + 4).ok_or_else(bad)?, i64_at(e + 12).ok_or_else(bad)?);
        let data = b.get(off..off + size).ok_or_else(bad)?.to_vec();
        match id {
            b"Comp" => comp = Some(data),
            b"Cont" => cont = Some(data),
            _ => {}
        }
    }
    Ok((comp.ok_or("the .vstpreset file holds no processor state")?, cont))
}

// ---------------------------------------------------------------------------------------------
// A running plug-in

struct Bus {
    channels: usize,
    /// One buffer per channel, `BLOCK` long.
    data: Vec<Vec<f32>>,
    ptrs: Vec<*mut f32>,
}

impl Bus {
    fn new(channels: usize) -> Bus {
        let mut data = vec![vec![0f32; BLOCK]; channels];
        let ptrs = data.iter_mut().map(|c| c.as_mut_ptr()).collect();
        Bus { channels, data, ptrs }
    }
}

/// A plug-in, set up and ready to process.
pub struct Instance {
    pub class: ClassInfo,
    component: ComPtr<IComponent>,
    processor: ComPtr<IAudioProcessor>,
    controller: Option<ComPtr<IEditController>>,
    /// The controller is an object of its own (not the component itself).
    separate: bool,
    _module: Arc<Module>,
    _host: ComWrapper<Host>,
    inputs: Vec<Bus>,
    outputs: Vec<Bus>,
    has_event_input: bool,
    params: Vec<ParamInfo>,
    pending: Vec<(u32, f64)>,
    sample_rate: f64,
}
// Used by one thread at a time (the renderer keeps instances behind a lock).
unsafe impl Send for Instance {}

impl Instance {
    /// Load `name` and set it up at `sample_rate`. `uid` chooses among the classes of a bundle that
    /// holds several; without it an instrument is preferred when `instrument`, an effect otherwise.
    pub fn open(name: &str, uid: Option<&str>, instrument: bool, sample_rate: u32) -> Result<Instance, String> {
        let path = find_bundle(name).ok_or_else(|| not_found(name))?;
        let module = load(&path).map_err(|e| format!("plug-in {name:?} ({}): {e}", path.display()))?;
        let class = match uid {
            Some(u) => module.classes.iter().find(|c| c.uid.eq_ignore_ascii_case(u)).ok_or_else(|| format!("plug-in {name:?} has no class {u}"))?,
            None => module.classes.iter().find(|c| c.is_instrument() == instrument).unwrap_or(&module.classes[0]),
        }
        .clone();
        unsafe { Self::start(module, class, sample_rate) }.map_err(|e| format!("plug-in {name:?}: {e}"))
    }

    unsafe fn start(module: Arc<Module>, class: ClassInfo, sample_rate: u32) -> Result<Instance, String> {
        let host = ComWrapper::new(Host);
        let host_ptr = host.to_com_ptr::<IHostApplication>().unwrap();
        let ctx = host_ptr.as_ptr() as *mut FUnknown;

        let mut raw: *mut c_void = std::ptr::null_mut();
        let r = module.factory.createInstance(class.cid.as_ptr(), IComponent::IID.as_ptr() as *const _, &mut raw);
        let component = ComPtr::<IComponent>::from_raw(raw as *mut IComponent).filter(|_| r == kResultOk).ok_or("could not be created")?;
        if component.initialize(ctx) != kResultOk {
            return Err("did not initialise".into());
        }
        let processor = component.cast::<IAudioProcessor>().ok_or("has no audio processor")?;

        // The controller: the component itself, or a second object.
        let (controller, separate) = match component.cast::<IEditController>() {
            Some(c) => (Some(c), false),
            None => {
                let mut cid: TUID = [0; 16];
                let mut made = None;
                if component.getControllerClassId(&mut cid) == kResultOk {
                    let mut raw: *mut c_void = std::ptr::null_mut();
                    let r = module.factory.createInstance(cid.as_ptr(), IEditController::IID.as_ptr() as *const _, &mut raw);
                    if let Some(c) = ComPtr::<IEditController>::from_raw(raw as *mut IEditController).filter(|_| r == kResultOk) {
                        if c.initialize(ctx) == kResultOk {
                            made = Some(c);
                        }
                    }
                }
                (made, true)
            }
        };
        if let Some(c) = &controller {
            let handler = host.to_com_ptr::<IComponentHandler>().unwrap();
            c.setComponentHandler(handler.as_ptr());
            if separate {
                if let (Some(a), Some(b)) = (component.cast::<IConnectionPoint>(), c.cast::<IConnectionPoint>()) {
                    a.connect(b.as_ptr());
                    b.connect(a.as_ptr());
                }
            }
        }

        // Buses: stereo where the plug-in accepts it, otherwise what it has.
        let count = |dir| component.getBusCount(MediaTypes_::kAudio as MediaType, dir).max(0);
        let (n_in, n_out) = (count(BusDirections_::kInput as BusDirection), count(BusDirections_::kOutput as BusDirection));
        let mut ins = vec![SpeakerArr::kStereo; n_in as usize];
        let mut outs = vec![SpeakerArr::kStereo; n_out as usize];
        processor.setBusArrangements(ins.as_mut_ptr(), n_in, outs.as_mut_ptr(), n_out);
        let buses = |dir: BusDirection, n: i32| -> Vec<Bus> {
            (0..n)
                .map(|i| {
                    let mut info: BusInfo = std::mem::zeroed();
                    component.getBusInfo(MediaTypes_::kAudio as MediaType, dir, i, &mut info);
                    component.activateBus(MediaTypes_::kAudio as MediaType, dir, i, (i == 0) as TBool);
                    Bus::new(info.channelCount.clamp(0, 64) as usize)
                })
                .collect()
        };
        let inputs = buses(BusDirections_::kInput as BusDirection, n_in);
        let outputs = buses(BusDirections_::kOutput as BusDirection, n_out);
        if outputs.first().map_or(true, |b| b.channels == 0) {
            return Err("has no audio output".into());
        }
        let has_event_input = component.getBusCount(MediaTypes_::kEvent as MediaType, BusDirections_::kInput as BusDirection) > 0;
        if has_event_input {
            component.activateBus(MediaTypes_::kEvent as MediaType, BusDirections_::kInput as BusDirection, 0, 1);
        }

        let mut setup = ProcessSetup {
            processMode: ProcessModes_::kOffline as int32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            maxSamplesPerBlock: BLOCK as int32,
            sampleRate: sample_rate as f64,
        };
        if processor.setupProcessing(&mut setup) != kResultOk {
            return Err(format!("does not run at {sample_rate} Hz"));
        }
        if component.setActive(1) != kResultOk {
            return Err("could not be switched on".into());
        }
        processor.setProcessing(1);

        let mut inst = Instance {
            class,
            component,
            processor,
            controller,
            separate,
            _module: module,
            _host: host,
            inputs,
            outputs,
            has_event_input,
            params: vec![],
            pending: vec![],
            sample_rate: sample_rate as f64,
        };
        inst.sync_controller();
        inst.read_params();
        Ok(inst)
    }

    /// Give the controller the processor's state, so both describe the same sound.
    unsafe fn sync_controller(&mut self) {
        if !self.separate {
            return;
        }
        if let Some(c) = &self.controller {
            let (w, s) = stream(vec![]);
            if self.component.getState(s.as_ptr()) == kResultOk {
                w.pos.set(0);
                c.setComponentState(s.as_ptr());
            }
        }
    }

    unsafe fn read_params(&mut self) {
        self.params.clear();
        let Some(c) = &self.controller else { return };
        for i in 0..c.getParameterCount() {
            let mut info: ParameterInfo = std::mem::zeroed();
            if c.getParameterInfo(i, &mut info) != kResultOk {
                continue;
            }
            let mut text: String128 = [0; 128];
            c.getParamStringByValue(info.id, info.defaultNormalizedValue, &mut text);
            self.params.push(ParamInfo {
                id: info.id,
                title: chars16(&info.title),
                units: chars16(&info.units),
                steps: info.stepCount,
                default: info.defaultNormalizedValue,
                default_text: chars16(&text),
            });
        }
    }

    pub fn params(&self) -> &[ParamInfo] {
        &self.params
    }

    pub fn has_audio_input(&self) -> bool {
        self.inputs.first().is_some_and(|b| b.channels > 0)
    }

    pub fn takes_notes(&self) -> bool {
        self.has_event_input
    }

    /// The parameter a key names: its title (letter case does not matter) or `#<id>`.
    pub fn param_id(&self, key: &str) -> Option<u32> {
        if let Some(n) = key.strip_prefix('#') {
            let id: u32 = n.parse().ok()?;
            return self.params.iter().any(|p| p.id == id).then_some(id);
        }
        self.params.iter().find(|p| p.title.eq_ignore_ascii_case(key)).map(|p| p.id)
    }

    /// Set a parameter (0…1). It takes effect with the next block.
    pub fn set_param(&mut self, id: u32, value: f64) {
        let value = value.clamp(0.0, 1.0);
        if let Some(c) = &self.controller {
            unsafe { c.setParamNormalized(id, value) };
        }
        self.pending.retain(|(i, _)| *i != id);
        self.pending.push((id, value));
    }

    /// The value a parameter has now (0…1), and how the plug-in prints it.
    pub fn param_value(&self, id: u32) -> Option<(f64, String)> {
        let c = self.controller.as_ref()?;
        unsafe {
            let v = c.getParamNormalized(id);
            let mut text: String128 = [0; 128];
            c.getParamStringByValue(id, v, &mut text);
            Some((v, chars16(&text)))
        }
    }

    /// Load the sound stored in a `.vstpreset` file.
    pub fn load_preset(&mut self, bytes: &[u8]) -> Result<(), String> {
        let (comp, cont) = parse_vstpreset(bytes)?;
        unsafe {
            let (_w, s) = stream(comp.clone());
            if self.component.setState(s.as_ptr()) != kResultOk {
                return Err("the plug-in did not accept the preset".into());
            }
            if let Some(c) = &self.controller {
                if self.separate {
                    let (_w, s) = stream(comp);
                    c.setComponentState(s.as_ptr());
                }
                if let Some(cont) = cont {
                    let (_w, s) = stream(cont);
                    c.setState(s.as_ptr());
                }
            }
        }
        self.pending.clear();
        Ok(())
    }

    /// The current sound as a `.vstpreset` file.
    pub fn save_preset(&self) -> Result<Vec<u8>, String> {
        let grab = |f: &dyn Fn(*mut IBStream) -> tresult| -> Option<Vec<u8>> {
            let (w, s) = stream(vec![]);
            (f(s.as_ptr()) == kResultOk).then(|| w.data.borrow().clone())
        };
        let comp = grab(&|s| unsafe { self.component.getState(s) }).ok_or("the plug-in did not give its state")?;
        let cont = self.controller.as_ref().filter(|_| self.separate).and_then(|c| grab(&|s| unsafe { c.getState(s) }));
        let mut out = b"VST3".to_vec();
        out.extend(1i32.to_le_bytes());
        out.extend(self.class.uid.as_bytes());
        out.extend(0i64.to_le_bytes()); // list offset, filled in below
        let mut entries: Vec<(&[u8; 4], usize, usize)> = vec![(b"Comp", out.len(), comp.len())];
        out.extend(&comp);
        if let Some(c) = &cont {
            entries.push((b"Cont", out.len(), c.len()));
            out.extend(c);
        }
        let list = out.len() as i64;
        out[40..48].copy_from_slice(&list.to_le_bytes());
        out.extend(b"List");
        out.extend((entries.len() as i32).to_le_bytes());
        for (id, off, size) in entries {
            out.extend(id);
            out.extend((off as i64).to_le_bytes());
            out.extend((size as i64).to_le_bytes());
        }
        Ok(out)
    }

    /// Silence everything and forget what was playing.
    pub fn reset(&mut self) {
        unsafe {
            self.processor.setProcessing(0);
            self.component.setActive(0);
            self.component.setActive(1);
            self.processor.setProcessing(1);
        }
    }

    /// Process `io` in place: its content is the input of an effect (ignored by an instrument), and
    /// it is replaced by the plug-in's output. `events` are sorted by offset. `position` is the
    /// sample at which `io` begins in the piece.
    pub fn process(&mut self, io: &mut [f32], events: &[NoteEvent], tempo_bpm: f64, position: u64) {
        let mut next = 0usize;
        let mut at = 0usize;
        while at < io.len() {
            let n = (io.len() - at).min(BLOCK);
            let list = ComWrapper::new(EventList::default());
            while next < events.len() && events[next].offset < at + n {
                let e = &events[next];
                let mut ev: Event = unsafe { std::mem::zeroed() };
                ev.sampleOffset = e.offset.saturating_sub(at) as int32;
                if e.on {
                    ev.r#type = Event_::EventTypes_::kNoteOnEvent as uint16;
                    ev.__field0.noteOn = NoteOnEvent { channel: 0, pitch: e.pitch as int16, tuning: 0.0, velocity: e.velocity.clamp(0.004, 1.0), length: 0, noteId: e.id };
                } else {
                    ev.r#type = Event_::EventTypes_::kNoteOffEvent as uint16;
                    ev.__field0.noteOff = NoteOffEvent { channel: 0, pitch: e.pitch as int16, velocity: 0.0, noteId: e.id, tuning: 0.0 };
                }
                list.events.borrow_mut().push(ev);
                next += 1;
            }
            unsafe { self.block(&mut io[at..at + n], &list, tempo_bpm, position + at as u64) };
            at += n;
        }
    }

    unsafe fn block(&mut self, io: &mut [f32], events: &ComWrapper<EventList>, tempo_bpm: f64, position: u64) {
        let n = io.len();
        for bus in self.inputs.iter_mut() {
            for ch in bus.data.iter_mut() {
                ch[..n].copy_from_slice(io);
            }
        }
        for bus in self.outputs.iter_mut() {
            for ch in bus.data.iter_mut() {
                ch[..n].fill(0.0);
            }
        }
        let wrap = |buses: &mut Vec<Bus>| -> Vec<AudioBusBuffers> {
            buses
                .iter_mut()
                .map(|b| AudioBusBuffers {
                    numChannels: b.channels as int32,
                    silenceFlags: 0,
                    __field0: AudioBusBuffers__type0 { channelBuffers32: b.ptrs.as_mut_ptr() },
                })
                .collect()
        };
        let mut ins = wrap(&mut self.inputs);
        let mut outs = wrap(&mut self.outputs);

        let changes = ComWrapper::new(Changes::default());
        for (id, v) in self.pending.drain(..) {
            changes.push(id, v);
        }
        let out_changes = ComWrapper::new(Changes::default());
        let out_events = ComWrapper::new(EventList::default());
        let in_changes = changes.to_com_ptr::<IParameterChanges>().unwrap();
        let out_changes = out_changes.to_com_ptr::<IParameterChanges>().unwrap();
        let in_events = events.to_com_ptr::<IEventList>().unwrap();
        let out_events = out_events.to_com_ptr::<IEventList>().unwrap();

        use ProcessContext_::StatesAndFlags_ as F;
        let mut ctx: ProcessContext = std::mem::zeroed();
        ctx.state = (F::kPlaying | F::kTempoValid | F::kProjectTimeMusicValid | F::kTimeSigValid | F::kContTimeValid) as uint32;
        ctx.sampleRate = self.sample_rate;
        ctx.projectTimeSamples = position as TSamples;
        ctx.continousTimeSamples = position as TSamples;
        ctx.projectTimeMusic = position as f64 / self.sample_rate * tempo_bpm / 60.0;
        ctx.tempo = tempo_bpm;
        ctx.timeSigNumerator = 4;
        ctx.timeSigDenominator = 4;

        let mut data = ProcessData {
            processMode: ProcessModes_::kOffline as int32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            numSamples: n as int32,
            numInputs: ins.len() as int32,
            numOutputs: outs.len() as int32,
            inputs: if ins.is_empty() { std::ptr::null_mut() } else { ins.as_mut_ptr() },
            outputs: outs.as_mut_ptr(),
            inputParameterChanges: in_changes.as_ptr(),
            outputParameterChanges: out_changes.as_ptr(),
            inputEvents: in_events.as_ptr(),
            outputEvents: out_events.as_ptr(),
            processContext: &mut ctx,
        };
        self.processor.process(&mut data);

        let main = &self.outputs[0];
        let k = 1.0 / main.channels as f32;
        for (i, x) in io.iter_mut().enumerate() {
            let s: f32 = main.data.iter().map(|ch| ch[i]).sum::<f32>() * k;
            *x = if s.is_finite() { s } else { 0.0 };
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            self.processor.setProcessing(0);
            self.component.setActive(0);
            if let Some(c) = &self.controller {
                if self.separate {
                    if let (Some(a), Some(b)) = (self.component.cast::<IConnectionPoint>(), c.cast::<IConnectionPoint>()) {
                        a.disconnect(b.as_ptr());
                        b.disconnect(a.as_ptr());
                    }
                    c.terminate();
                }
            }
            self.component.terminate();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Either kind of plug-in

/// A running plug-in: a VST 3 bundle on this machine, or (name `web:<id>`) a web instrument.
pub enum Player {
    Vst(Box<Instance>),
    Web(web::Instance),
}

impl Player {
    pub fn open(name: &str, uid: Option<&str>, instrument: bool, sample_rate: u32) -> Result<Player, String> {
        match name.strip_prefix(web::PREFIX) {
            Some(id) => web::Instance::open(id, instrument, sample_rate).map(Player::Web),
            None => Instance::open(name, uid, instrument, sample_rate).map(|i| Player::Vst(Box::new(i))),
        }
    }

    pub fn has_audio_input(&self) -> bool {
        match self {
            Player::Vst(i) => i.has_audio_input(),
            Player::Web(i) => i.has_audio_input(),
        }
    }

    pub fn param_id(&self, key: &str) -> Option<u32> {
        match self {
            Player::Vst(i) => i.param_id(key),
            Player::Web(i) => i.param_id(key),
        }
    }

    pub fn set_param(&mut self, id: u32, value: f64) {
        match self {
            Player::Vst(i) => i.set_param(id, value),
            Player::Web(i) => i.set_param(id, value),
        }
    }

    /// Web instruments have no preset files.
    pub fn load_preset(&mut self, bytes: &[u8]) -> Result<(), String> {
        match self {
            Player::Vst(i) => i.load_preset(bytes),
            Player::Web(_) => Err("a web instrument takes no .vstpreset".into()),
        }
    }

    pub fn reset(&mut self) {
        match self {
            Player::Vst(i) => i.reset(),
            Player::Web(i) => i.reset(),
        }
    }

    pub fn process(&mut self, io: &mut [f32], events: &[NoteEvent], tempo_bpm: f64, position: u64) {
        match self {
            Player::Vst(i) => i.process(io, events, tempo_bpm, position),
            Player::Web(i) => i.process(io, events),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vstpreset_layout() {
        let mut f = b"VST3".to_vec();
        f.extend(1i32.to_le_bytes());
        f.extend([b'A'; 32]);
        f.extend(0i64.to_le_bytes());
        f.extend(b"hello");
        f.extend(b"ctl");
        let list = f.len() as i64;
        f[40..48].copy_from_slice(&list.to_le_bytes());
        f.extend(b"List");
        f.extend(2i32.to_le_bytes());
        for (id, off, size) in [(b"Comp", 48i64, 5i64), (b"Cont", 53, 3)] {
            f.extend(id);
            f.extend(off.to_le_bytes());
            f.extend(size.to_le_bytes());
        }
        assert_eq!(parse_vstpreset(&f).unwrap(), (b"hello".to_vec(), Some(b"ctl".to_vec())));
        assert!(parse_vstpreset(b"nope").is_err());
    }

    #[test]
    fn a_missing_plugin_is_reported() {
        assert!(!available("no-such-plugin-anywhere", None));
        assert!(find_bundle("/Library/Audio/Plug-Ins/VST3/Dexed.vst3").is_none(), "a name is not a path");
        assert!(Instance::open("no-such-plugin-anywhere", None, true, 48000).err().unwrap().contains("not installed"));
    }
}
