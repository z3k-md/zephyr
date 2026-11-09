use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    // The speech runtime is loaded from the model folder, not linked. Its Windows
    // build imports the VC++ 2015–2022 CRT, which a clean PC does not have, so those
    // DLLs are staged beside the exe. Zephyr itself keeps its static CRT and does not
    // import them; Windows finds them when the runtime is loaded.
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows") {
        stage_crt();
    }
    tauri_build::build();
}

/// DLLs `onnxruntime.dll` 1.31.0 (CPU, official win-x64 zip) imports, plus nothing
/// further: each of these only imports the others and the Universal CRT that
/// Windows 10 and 11 already ship.
const CRT_DLLS: &[&str] = &[
    "msvcp140.dll",
    "msvcp140_1.dll",
    "vcruntime140.dll",
    "vcruntime140_1.dll",
];

fn stage_crt() {
    let crt_dir = latest_crt_dir();
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let dest_dir = manifest.join("resources");
    std::fs::create_dir_all(&dest_dir).expect("create src-tauri/resources");
    for name in CRT_DLLS {
        let src = crt_dir.join(name);
        println!("cargo:rerun-if-changed={}", src.display());
        let bytes = std::fs::read(&src).unwrap_or_else(|err| {
            panic!(
                "couldn't read {} ({err}). Voice stages this VC++ redistributable DLL next to the exe.",
                src.display()
            )
        });
        write_if_changed(&dest_dir.join(name), &bytes);
    }
}

fn write_if_changed(dest: &Path, bytes: &[u8]) {
    if std::fs::read(dest).ok().as_deref() == Some(bytes) {
        return;
    }
    std::fs::write(dest, bytes)
        .unwrap_or_else(|err| panic!("couldn't stage {}: {err}", dest.display()));
}

fn latest_crt_dir() -> PathBuf {
    let mut dirs = Vec::new();
    for root in vs_install_roots() {
        let redist = root.join("VC").join("Redist").join("MSVC");
        let Ok(versions) = std::fs::read_dir(&redist) else {
            continue;
        };
        for version in versions.flatten() {
            let dir = version.path().join("x64").join("Microsoft.VC143.CRT");
            if dir.join("vcruntime140.dll").is_file() {
                dirs.push(dir);
            }
        }
    }
    dirs.sort();
    dirs.pop().unwrap_or_else(|| {
        panic!(
            "Microsoft.VC143.CRT was not found. Install the MSVC v143 x64 toolset so the Windows build can stage the VC++ runtime next to Zephyr."
        )
    })
}

fn vs_install_roots() -> Vec<PathBuf> {
    let mut roots = vswhere_install_paths();
    roots.extend([
        PathBuf::from(r"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools"),
        PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\2022\BuildTools"),
        PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\2022\Community"),
        PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\2022\Professional"),
        PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\2022\Enterprise"),
    ]);
    roots
}

fn vswhere_install_paths() -> Vec<PathBuf> {
    let vswhere =
        PathBuf::from(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe");
    if !vswhere.is_file() {
        return Vec::new();
    }
    let Ok(output) = Command::new(vswhere)
        .args([
            "-all",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect()
}
