//! The app's build: Tauri's (tauri_build), and the VC++ runtime the installer ships beside the app
//! (installer-hooks.nsh).

use std::path::{Path, PathBuf};
use std::process::Command;

/// The VC++ runtime ONNX Runtime needs: its library is built for the runtime in DLLs, and msvcp140 loads the other
/// three. Windows has them only once some program has installed Microsoft's VC++ Redistributable.
const VC_RUNTIME: [&str; 4] = ["msvcp140.dll", "msvcp140_1.dll", "vcruntime140.dll", "vcruntime140_1.dll"];

fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        stage_vc_runtime();
    }
    tauri_build::build()
}

/// Copies the VC++ runtime from the newest Visual Studio on this computer (a newer runtime runs programs built with an
/// older one) into vc-runtime/ beside the app's exe (target/<profile>/), where the installer takes it from.
fn stage_vc_runtime() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    // OUT_DIR is target/<profile>/build/aimview-desktop-<hash>/out
    let dest = out.ancestors().nth(3).unwrap().join("vc-runtime");
    println!("cargo:rerun-if-changed={}", dest.display());
    let Some(from) = newest_runtime() else {
        println!("cargo:warning=no VC++ runtime in any Visual Studio here: the installer cannot be built");
        return;
    };
    std::fs::create_dir_all(&dest).unwrap();
    for name in VC_RUNTIME {
        let src = from.join(name);
        println!("cargo:rerun-if-changed={}", src.display());
        if let Err(e) = std::fs::copy(&src, dest.join(name)) {
            println!("cargo:warning=could not copy {}: {e}", src.display());
        }
    }
}

/// The folder of the newest VC++ runtime for the target (VC\Redist\MSVC\<version>\<arch>\Microsoft.VC<n>.CRT), from
/// every Visual Studio that vswhere lists.
fn newest_runtime() -> Option<PathBuf> {
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").ok()?.as_str() {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "x86",
        _ => return None,
    };
    let installer = Path::new(&std::env::var("ProgramFiles(x86)").ok()?).join(r"Microsoft Visual Studio\Installer");
    let listed = Command::new(installer.join("vswhere.exe"))
        .args(["-all", "-products", "*", "-property", "installationPath"])
        .output()
        .ok()?;
    let mut best: Option<(Vec<u32>, PathBuf)> = None;
    for install in String::from_utf8_lossy(&listed.stdout).lines().map(str::trim).filter(|l| !l.is_empty()) {
        let vc = Path::new(install).join("VC");
        let Ok(version) = std::fs::read_to_string(vc.join(r"Auxiliary\Build\Microsoft.VCRedistVersion.default.txt")) else {
            continue;
        };
        let version = version.trim();
        let Ok(entries) = std::fs::read_dir(vc.join(r"Redist\MSVC").join(version).join(arch)) else {
            continue;
        };
        let crt = entries.flatten().map(|e| e.path()).find(|p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy();
            p.is_dir() && name.starts_with("Microsoft.VC") && name.ends_with(".CRT")
        });
        let number: Vec<u32> = version.split('.').filter_map(|n| n.parse().ok()).collect();
        if let Some(crt) = crt
            && best.as_ref().is_none_or(|(b, _)| number > *b)
        {
            best = Some((number, crt));
        }
    }
    best.map(|(_, crt)| crt)
}
