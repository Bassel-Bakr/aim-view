//! The detector model on this computer, on the device the configuration says (config.rs: `Device`): ONNX Runtime with
//! DirectML (any Windows GPU), with CUDA (an NVIDIA GPU, with the `cuda` feature), with WebGPU (any GPU with Vulkan,
//! with the `webgpu` feature) or on the CPU; `Auto` tries the GPU first and falls back to the CPU. It takes the _u8in export (python/model/export.py): a batch of 720p RGB frames and
//! the fixed map, as bytes. The model's settings come from its settings file beside it (`model_settings`). In: the
//! review's frames (review.rs). Out: each frame's score and reg maps, which the core's tracking takes.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use aimview::convert::{DST_H, DST_W};
use aimview::model::{DEFAULT_THRESHOLD, ModelSettings, settings_file};
use ort::execution_providers::{
    CPUExecutionProvider, CUDAExecutionProvider, DirectMLExecutionProvider, ExecutionProviderDispatch,
    WebGPUExecutionProvider,
};
use ort::session::Session;
use ort::session::builder::SessionBuilder;
use ort::value::{Tensor, TensorRef};

use crate::config::Device;

/// The detector's maps are this many times smaller than the frame each way.
const MAP_SCALE: usize = 4;
/// A map's width in cells (`gw` in the core's session.rs).
pub const MAP_WIDTH: usize = DST_W / MAP_SCALE;
/// A map's height in cells (`gh` in the core's session.rs).
pub const MAP_HEIGHT: usize = DST_H / MAP_SCALE;
/// The reg maps (the boxes' regression) each frame has.
const REG_MAPS: usize = 4;
/// The bytes of an RGB pixel.
const RGB_CHANNELS: usize = 3;
/// The CPU's threads when the system cannot say how many it has.
const DEFAULT_CPU_THREADS: usize = 4;
/// The most CPU threads the detector takes.
const MAX_CPU_THREADS: usize = 8;
/// The Vulkan loader's setting that hides drivers, by filters on their manifest files' names.
#[cfg(target_os = "linux")]
const VULKAN_DRIVERS_HIDDEN: &str = "VK_LOADER_DRIVERS_DISABLE";
/// The Vulkan drivers WebGPU must not run the detector on: Mesa's software one (lavapipe: the GPU drawn on the CPU,
/// 10 times slower than ONNX Runtime's own CPU code) and WSL's Dozen (Vulkan over Direct3D 12: Dawn refuses it, and it
/// crashes when the process exits).
#[cfg(target_os = "linux")]
const UNUSABLE_VULKAN_DRIVERS: &str = "*lvp*,*dzn*";

/// Hides UNUSABLE_VULKAN_DRIVERS from this process's Vulkan loader, so WebGPU finds a real GPU or none (`Auto` then
/// runs the detector on the CPU); a value the user set stays. Linux only. Call it first in `main`, before any other
/// thread starts: changing the environment while another thread reads it is undefined behavior.
pub fn hide_unusable_vulkan_drivers() {
    #[cfg(target_os = "linux")]
    if std::env::var_os(VULKAN_DRIVERS_HIDDEN).is_none() {
        // SAFETY: called before any other thread exists (see above)
        unsafe { std::env::set_var(VULKAN_DRIVERS_HIDDEN, UNUSABLE_VULKAN_DRIVERS) };
    }
}

#[cfg(target_os = "linux")]
unsafe extern "C" {
    /// glibc's: `function` runs when the process exits, with the exit status, before the handlers registered earlier.
    fn on_exit(function: extern "C" fn(i32, *mut std::ffi::c_void), arg: *mut std::ffi::c_void) -> i32;
}

/// Ends the process at once with its exit status, skipping the exit handlers registered before it.
#[cfg(target_os = "linux")]
extern "C" fn leave_at_once(status: i32, _arg: *mut std::ffi::c_void) {
    // SAFETY: _exit only ends the process; Rust's own output is flushed before the exit handlers run
    unsafe { libc::_exit(status) }
}

/// Before WebGPU first loads: the process's exit skips the static destructors registered so far, where ONNX Runtime
/// 1.22's WebGPU provider releases Dawn's instance twice and crashes the exit (a segfault after every review, even one
/// whose WebGPU did not start). The exit status stays. Once per process; Linux only (glibc's `on_exit`).
fn skip_webgpu_teardown() {
    #[cfg(target_os = "linux")]
    {
        /// Whether the handler is registered.
        static REGISTERED: std::sync::Once = std::sync::Once::new();
        REGISTERED.call_once(|| {
            // SAFETY: on_exit only records the handler, which only ends the process; when it cannot (no room), the
            // exit crashes as before, after every file is written
            let _ = unsafe { on_exit(leave_at_once, std::ptr::null_mut()) };
        });
    }
}

/// The detector model loaded in ONNX Runtime, with the fixed map ready for every call.
pub struct Detector {
    /// ONNX Runtime's session, its sizes fixed to `batch` frames of 720p.
    session: Session,
    /// The fixed map repeated for each frame of a batch (batch x DST_H x DST_W).
    fixed: Tensor<u8>,
    /// The frames each call takes.
    batch: usize,
    /// Where it runs, for the tracks' `detector` ("DirectML", "CUDA" or "CPU").
    pub device: &'static str,
}

/// One call's maps: score (batch x 1 x MAP_HEIGHT x MAP_WIDTH) and reg (batch x REG_MAPS x MAP_HEIGHT x MAP_WIDTH),
/// where ONNX Runtime left them.
pub struct Maps<'a> {
    /// Each cell's score, every frame's map one after another.
    pub score: &'a [f32],
    /// Each cell's box regression, `REG_MAPS` maps a frame, every frame's one after another.
    pub reg: &'a [f32],
}

impl<'a> Maps<'a> {
    /// The score map and the reg maps of the batch's `index`-th frame.
    pub fn of_frame(&self, index: usize) -> (&'a [f32], &'a [f32]) {
        let map = MAP_WIDTH * MAP_HEIGHT;
        let regs = REG_MAPS * map;
        (&self.score[index * map..(index + 1) * map], &self.reg[index * regs..(index + 1) * regs])
    }
}

/// A session's builder with the model's sizes fixed: `batch` frames of 720p a call.
fn sized(builder: SessionBuilder, batch: usize) -> ort::Result<SessionBuilder> {
    builder
        .with_dimension_override("n", batch as i64)?
        .with_dimension_override("h", DST_H as i64)?
        .with_dimension_override("w", DST_W as i64)
}

/// The model on a GPU, through `provider`.
fn on_gpu(model: &Path, batch: usize, provider: ExecutionProviderDispatch) -> ort::Result<Session> {
    // ONNX Runtime's environment first (ort makes it only when a session is committed): the WebGPU provider logs
    // through its logger as it is added, and fails without one
    ort::environment::get_environment()?;
    Session::builder()
        .and_then(|builder| builder.with_execution_providers([provider.error_on_failure()]))
        // a GPU runs one node at a time, with no memory pattern (DirectML needs it so)
        .and_then(|builder| builder.with_parallel_execution(false))
        .and_then(|builder| builder.with_memory_pattern(false))
        .and_then(|builder| sized(builder, batch))
        .and_then(|builder| builder.commit_from_file(model))
}

/// The model on the CPU, on up to MAX_CPU_THREADS threads.
fn on_cpu(model: &Path, batch: usize) -> ort::Result<Session> {
    let threads =
        std::thread::available_parallelism().map_or(DEFAULT_CPU_THREADS, |threads| threads.get().min(MAX_CPU_THREADS));
    Session::builder()
        .and_then(|builder| builder.with_execution_providers([CPUExecutionProvider::default().build()]))
        .and_then(|builder| builder.with_intra_threads(threads))
        .and_then(|builder| sized(builder, batch))
        .and_then(|builder| builder.commit_from_file(model))
}

impl Detector {
    /// The model at `model`, taking `batch` frames a call, with the fixed map (DST_W x DST_H) for each of them, on
    /// `device`. `Auto` tries DirectML on Windows, else CUDA and WebGPU (those built in), then the CPU; an error when
    /// the device asked for (or the CPU, last) cannot load it.
    pub fn new(model: &Path, batch: usize, fixed: &[u8], device: Device) -> Result<Detector, String> {
        let directml = || on_gpu(model, batch, DirectMLExecutionProvider::default().build());
        let cuda = || on_gpu(model, batch, CUDAExecutionProvider::default().build());
        let webgpu = || {
            skip_webgpu_teardown();
            on_gpu(model, batch, WebGPUExecutionProvider::default().build())
        };
        let cpu = || on_cpu(model, batch);
        let failed =
            |on: &str, error: ort::Error| format!("the detector could not load {} on {on}: {error}", model.display());
        let (session, device) = match device {
            Device::DirectMl => (directml().map_err(|error| failed("DirectML", error))?, "DirectML"),
            Device::Cuda => (cuda().map_err(|error| failed("CUDA", error))?, "CUDA"),
            Device::WebGpu => (webgpu().map_err(|error| failed("WebGPU", error))?, "WebGPU"),
            Device::Cpu => (cpu().map_err(|error| failed("the CPU", error))?, "CPU"),
            Device::Auto => {
                let on_webgpu = || webgpu().map(|session| (session, "WebGPU"));
                let gpu = if cfg!(windows) {
                    directml().map(|session| (session, "DirectML"))
                } else if cfg!(feature = "cuda") {
                    cuda().map(|session| (session, "CUDA")).or_else(|_| on_webgpu())
                } else {
                    on_webgpu()
                };
                match gpu {
                    Ok(found) => found,
                    Err(_) => (cpu().map_err(|error| failed("the CPU", error))?, "CPU"),
                }
            }
        };
        let all = fixed.repeat(batch);
        let fixed = Tensor::from_array(([batch, DST_H, DST_W], all)).map_err(|error| error.to_string())?;
        Ok(Detector { session, fixed, batch, device })
    }

    /// One call on a whole batch of RGB frames (batch x DST_H x DST_W x 3 bytes): its maps handed to `read` where ONNX
    /// Runtime left them (copying them out cost 4.6 MB a call of 4 frames).
    pub fn run<T>(&mut self, rgb: &[u8], read: impl FnOnce(Maps<'_>) -> T) -> Result<T, String> {
        let rgb = TensorRef::from_array_view(([self.batch, DST_H, DST_W, RGB_CHANNELS], rgb))
            .map_err(|error| error.to_string())?;
        let out = self
            .session
            .run(ort::inputs!["rgb" => rgb, "fixed" => &self.fixed])
            .map_err(|error| format!("the detector failed: {error}"))?;
        let score = out["score"].try_extract_tensor::<f32>().map_err(|error| error.to_string())?.1;
        let reg = out["reg"].try_extract_tensor::<f32>().map_err(|error| error.to_string())?.1;
        Ok(read(Maps { score, reg }))
    }
}

/// The settings file beside a model's export (detector_<name>.json: src/model.rs). A model with none gets today's
/// values, said on stderr once a model; a file that cannot be read stops the review.
pub fn model_settings(model: &Path) -> Result<ModelSettings, String> {
    let name = model.file_name().and_then(|name| name.to_str()).and_then(settings_file);
    let path = name.map_or_else(|| model.with_extension("json"), |name| model.with_file_name(name));
    match std::fs::read_to_string(&path) {
        Ok(text) => ModelSettings::from_json(&text)
            .map_err(|error| format!("the model's settings file {}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            /// The missing settings files already said, so each is said once a process.
            static SAID: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
            if let Ok(mut said) = SAID.lock()
                && !said.contains(&path)
            {
                eprintln!(
                    "{} is missing: the detector takes today's values (threshold {DEFAULT_THRESHOLD}, no score map)",
                    path.display()
                );
                said.push(path);
            }
            Ok(ModelSettings::default())
        }
        Err(error) => Err(format!("the model's settings file {} could not be read: {error}", path.display())),
    }
}
