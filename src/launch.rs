//! Getting a window up on any PC. Graphics drivers are the shakiest part of
//! Windows: an old Intel OpenGL driver, for one, crashes the whole process the
//! moment it's loaded, before the app can report anything. So on Windows the
//! .exe first runs as a small supervisor that starts the real app as a child
//! process, one graphics setup at a time (Direct3D 12 on the GPU, then
//! Direct3D 12 in software, then Vulkan, then OpenGL), until one of them gets a
//! window on screen. The one that worked is remembered for next time.

use eframe::egui_wgpu::{self, WgpuSetup, wgpu};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set on the child process: the graphics setup to use.
pub const GPU_ENV: &str = "DROP_IN_BADMINTON_GPU";

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Gpu {
    /// Direct3D 12 on a real graphics card.
    Dx12,
    /// Direct3D 12 drawn by the CPU (WARP), which every Windows 10/11 PC has.
    Warp,
    Vulkan,
    OpenGl,
}

impl Gpu {
    const ALL: [Gpu; 4] = [Gpu::Dx12, Gpu::Warp, Gpu::Vulkan, Gpu::OpenGl];

    fn name(self) -> &'static str {
        match self {
            Gpu::Dx12 => "dx12",
            Gpu::Warp => "warp",
            Gpu::Vulkan => "vulkan",
            Gpu::OpenGl => "opengl",
        }
    }

    fn parse(s: &str) -> Option<Gpu> {
        Gpu::ALL.into_iter().find(|g| g.name() == s.trim())
    }
}

/// The graphics setup this process was told to use, if any.
pub fn chosen() -> Option<Gpu> {
    std::env::var(GPU_ENV).ok().as_deref().and_then(Gpu::parse)
}

static LAUNCHED: AtomicBool = AtomicBool::new(false);

fn marker() -> PathBuf {
    crate::model::data_dir().join("graphics.txt")
}

/// The window has drawn a few frames: this graphics setup works. Remembered so
/// the next start goes straight to it, and so the supervisor can tell a crash
/// at start-up (try something else) from one later on.
pub fn mark_launched() {
    if LAUNCHED.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Some(gpu) = chosen() {
        let _ = std::fs::create_dir_all(crate::model::data_dir());
        let _ = std::fs::write(marker(), gpu.name());
    }
}

/// Whether a fatal error should be shown to the user. A child that hasn't got a
/// window up yet stays quiet: the supervisor will try the next setup.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn should_report() -> bool {
    chosen().is_none() || LAUNCHED.load(Ordering::SeqCst)
}

/// The renderer settings for this process.
pub fn wgpu_options() -> egui_wgpu::WgpuConfiguration {
    let mut config = egui_wgpu::WgpuConfiguration::default();
    let Some(gpu) = chosen() else { return config };
    // Tests: die the way a broken driver does, without a word, to check the fallback.
    if std::env::var("DROP_IN_BADMINTON_FAKE_CRASH").is_ok_and(|s| s == gpu.name()) {
        std::process::abort();
    }
    if let WgpuSetup::CreateNew(setup) = &mut config.wgpu_setup {
        // Only start the one backend: merely probing the others loads their drivers.
        setup.instance_descriptor.backends = match gpu {
            Gpu::Dx12 | Gpu::Warp => wgpu::Backends::DX12,
            Gpu::Vulkan => wgpu::Backends::VULKAN,
            Gpu::OpenGl => wgpu::Backends::GL,
        };
        setup.native_adapter_selector = Some(Arc::new(move |adapters, surface| {
            let rank = |a: &&wgpu::Adapter| match a.get_info().device_type {
                wgpu::DeviceType::DiscreteGpu => 0,
                wgpu::DeviceType::IntegratedGpu => 1,
                wgpu::DeviceType::VirtualGpu => 2,
                wgpu::DeviceType::Other => 3,
                wgpu::DeviceType::Cpu => 4,
            };
            adapters
                .iter()
                .filter(|a| surface.is_none_or(|s| a.is_surface_supported(s)))
                .filter(|a| {
                    let software = a.get_info().device_type == wgpu::DeviceType::Cpu;
                    match gpu {
                        Gpu::Dx12 => !software,
                        Gpu::Warp => software,
                        _ => true,
                    }
                })
                .min_by_key(rank)
                .cloned()
                .ok_or_else(|| format!("no {} graphics adapter", gpu.name()))
        }));
    }
    config
}

/// Windows: run the app as a child process, trying each graphics setup until one
/// works. Returns false when this process should run the app itself instead
/// (other platforms, the child, or `WGPU_BACKEND` set by hand).
pub fn supervise() -> bool {
    if !cfg!(windows) || std::env::var_os(GPU_ENV).is_some() || std::env::var_os("WGPU_BACKEND").is_some() {
        return false;
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    let last_good = std::fs::read_to_string(marker()).ok().as_deref().and_then(Gpu::parse);
    let mut order: Vec<Gpu> = last_good.into_iter().collect();
    order.extend(Gpu::ALL.into_iter().filter(|g| Some(*g) != last_good));

    let mut tried = Vec::new();
    for gpu in order {
        let started = std::time::SystemTime::now();
        let status = match std::process::Command::new(&exe).env(GPU_ENV, gpu.name()).status() {
            Ok(status) => status,
            Err(err) => {
                tried.push(format!("{}: couldn't start ({err})", gpu.name()));
                continue;
            }
        };
        if status.success() {
            return true;
        }
        let code = status.code().map(|c| format!("{:#010x}", c as u32)).unwrap_or_else(|| "?".into());
        let got_going = std::fs::metadata(marker()).and_then(|m| m.modified()).is_ok_and(|t| t >= started)
            && std::fs::read_to_string(marker()).ok().as_deref().and_then(Gpu::parse) == Some(gpu);
        if got_going {
            // It was up and running, so this isn't the graphics. Panics have
            // already been reported by the child (exit code 101).
            if status.code() != Some(101) {
                crate::fail(&format!("closed unexpectedly (exit code {code}, graphics: {})", gpu.name()));
            }
            return true;
        }
        tried.push(format!("{}: exit code {code}", gpu.name()));
    }
    crate::fail(&format!("couldn't open a window with any graphics setup.\n{}", tried.join("\n")));
    true
}
