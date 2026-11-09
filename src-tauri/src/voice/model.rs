//! The English Parakeet TDT 0.6B v2 int8 ONNX export and the ONNX Runtime that runs it.
//!
//! The runtime is Microsoft's official CPU build of ONNX Runtime 1.31, the release ort
//! 2.0.0-rc.15's `api-31` feature was written for. A model folder is checked
//! first (`<dir>/ort-1.31.0/`), then a shared copy next to the model folder, so the runtime
//! is not downloaded twice and an existing install keeps working.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

/// Hugging Face repo that publishes the ONNX export.
const SOURCE_REPO: &str = "istupakov/parakeet-tdt-0.6b-v2-onnx";
/// Commit the download URLs and hashes are pinned to.
pub const REVISION: &str = "0bbb45a3365852604aef28b538a8f066f4ccaa85";
/// Directory name under `<app local data>/models/`.
pub const MODEL_ID: &str = "parakeet-tdt-0.6b-v2";
/// What Settings calls the model.
pub const DISPLAY_NAME: &str = "Parakeet TDT 0.6B v2";
/// Decimal size shown on the model card. Progress uses the exact byte count.
pub(crate) const SIZE_LABEL: &str = "661 MB";
/// NVIDIA's license for Parakeet TDT v2. The ONNX export keeps it.
pub const LICENSE: &str = "CC-BY-4.0";
/// ONNX Runtime release loaded for speech. ort 2.0.0-rc.15 requires API 31, which is 1.31.
pub const RUNTIME_VERSION: &str = "1.31.0";
/// Bytes of the model files plus the runtime archive, the size [`download`] reports
/// when the runtime still has to be fetched.
pub const DOWNLOAD_BYTES: u64 = MODEL_BYTES + RUNTIME.archive_size;

pub(crate) const MODEL_BYTES: u64 = 661_191_684;

struct ModelFile {
    name: &'static str,
    size: u64,
    sha256: &'static str,
}

struct Runtime {
    /// `ort-<version>` under the model directory.
    dir: &'static str,
    /// Library file name inside [`Runtime::dir`].
    name: &'static str,
    size: u64,
    sha256: &'static str,
    url: &'static str,
    archive_name: &'static str,
    archive_size: u64,
    archive_sha256: &'static str,
    /// Path of the library inside the archive, without a leading `./`.
    member: &'static str,
}

/// Official Windows x64 CPU build. The archive is the GitHub release asset; the hash and
/// size below are the extracted `onnxruntime.dll`, which is the only file the engine loads.
#[cfg(all(windows, target_arch = "x86_64"))]
const RUNTIME: Runtime = Runtime {
    dir: "ort-1.31.0",
    name: "onnxruntime.dll",
    size: 17_253_176,
    sha256: "0c42dc4e278f0968fdf9ebf1b1e54a74c657a9942e70803e453c91993451cf4c",
    url: "https://github.com/microsoft/onnxruntime/releases/download/v1.31.0/onnxruntime-win-x64-1.31.0.zip",
    archive_name: "onnxruntime-win-x64-1.31.0.zip",
    archive_size: 82_887_349,
    archive_sha256: "3958d8a44984160692c1b4ffd0de3cb48f8c39f9d34bb31e433db06e30b82195",
    member: "onnxruntime-win-x64-1.31.0/lib/onnxruntime.dll",
};

/// Official macOS arm64 CPU build. Zephyr's release workflow ships `aarch64-apple-darwin` only.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const RUNTIME: Runtime = Runtime {
    dir: "ort-1.31.0",
    name: "libonnxruntime.1.31.0.dylib",
    size: 44_830_288,
    sha256: "2a50a63e7f41f8d4d551d3ee47248df3864f6540109fce8ad520ca3b51af4b70",
    url: "https://github.com/microsoft/onnxruntime/releases/download/v1.31.0/onnxruntime-osx-arm64-1.31.0.tgz",
    archive_name: "onnxruntime-osx-arm64-1.31.0.tgz",
    archive_size: 36_549_009,
    archive_sha256: "1031b36dd3bdaa8d976a6130d9cf2a05e9be320b93ae804f8427b91dc3432afc",
    member: "onnxruntime-osx-arm64-1.31.0/lib/libonnxruntime.1.31.0.dylib",
};

#[cfg(not(any(
    all(windows, target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "aarch64"),
)))]
compile_error!("the speech runtime is packaged for Windows x64 and macOS arm64");

/// int8 encoder, int8 decoder/joint, and the SentencePiece vocab. Hashes are the Git LFS
/// sha256 for the ONNX files and the sha256 of `vocab.txt` at [`REVISION`] (it is not in LFS).
const FILES: &[ModelFile] = &[
    ModelFile {
        name: "encoder-model.int8.onnx",
        size: 652_184_014,
        sha256: "3e0581fda6ab843888b51e56d7ee78b6d5bc3237ec113af1f732d1d5286aa155",
    },
    ModelFile {
        name: "decoder_joint-model.int8.onnx",
        size: 8_998_286,
        sha256: "a449f49acd68979d418651dd2dcb737cc0f1bf0225e009e29ee326354edbf7d3",
    },
    ModelFile {
        name: "vocab.txt",
        size: 9_384,
        sha256: "ec182b70dd42113aff6c5372c75cac58c952443eb22322f57bbd7f53977d497d",
    },
];

/// `<app_local_data_dir>/models`.
pub(crate) fn models_root(app_local_data_dir: impl AsRef<Path>) -> PathBuf {
    app_local_data_dir.as_ref().join("models")
}

/// `<app_local_data_dir>/models/parakeet-tdt-0.6b-v2`.
pub fn model_dir(app_local_data_dir: impl AsRef<Path>) -> PathBuf {
    models_root(app_local_data_dir).join(MODEL_ID)
}

/// The ONNX Runtime library inside `dir`, in its versioned folder. Benches pass a folder
/// that keeps the runtime here. The app also finds a shared copy; see [`resolve_runtime_library`].
pub(crate) fn runtime_library(dir: &Path) -> PathBuf {
    dir.join(RUNTIME.dir).join(RUNTIME.name)
}

/// Where a new runtime is unpacked. The app model folder shares `<models>/ort-1.31.0`. A
/// bench folder keeps the runtime inside itself.
fn runtime_home(model_dir: &Path) -> PathBuf {
    let name = model_dir.file_name().and_then(|name| name.to_str());
    if name == Some(MODEL_ID)
        && let Some(parent) = model_dir.parent()
    {
        return parent.join(RUNTIME.dir);
    }
    model_dir.join(RUNTIME.dir)
}

/// The CPU runtime `model_dir` can load: its own folder, then `<models>/ort-1.31.0`. The
/// first file with the pinned size wins, so a bench that vendors the library is unchanged
/// and an existing install is not downloaded again.
pub(crate) fn resolve_runtime_library(model_dir: &Path) -> Option<PathBuf> {
    find_sized_runtime(model_dir, RUNTIME.size)
}

pub(crate) fn find_sized_runtime(model_dir: &Path, size: u64) -> Option<PathBuf> {
    runtime_candidates(model_dir)
        .into_iter()
        .find(|path| file_len(path) == Some(size))
}

fn runtime_candidates(model_dir: &Path) -> Vec<PathBuf> {
    let own = runtime_library(model_dir);
    let mut paths = vec![own.clone()];
    let Some(parent) = model_dir.parent() else {
        return paths;
    };
    paths.push(parent.join(RUNTIME.dir).join(RUNTIME.name));
    paths
}

/// Copies a runtime an older install left inside the model folder up to
/// `<models>/ort-1.31.0` when that shared copy is missing. The original stays put, so a
/// running process that already mapped it is left alone, and deleting the model does not
/// take the runtime with it.
pub(crate) fn share_runtime(models_root: &Path) {
    let shared = models_root.join(RUNTIME.dir).join(RUNTIME.name);
    if file_len(&shared) == Some(RUNTIME.size) {
        return;
    }
    let legacy = models_root
        .join(MODEL_ID)
        .join(RUNTIME.dir)
        .join(RUNTIME.name);
    if file_len(&legacy) != Some(RUNTIME.size) {
        return;
    }
    if let Some(parent) = shared.parent()
        && fs::create_dir_all(parent).is_ok()
    {
        let _ = fs::copy(&legacy, &shared);
    }
}

/// The weights and a resolvable CPU runtime are on disk.
pub fn installed(dir: &Path) -> bool {
    files_present(dir, FILES) && resolve_runtime_library(dir).is_some()
}

/// Bytes [`download`] reports as its total: the weights, plus the runtime archive when no
/// runtime is already visible from `dir`.
pub(crate) fn download_total(dir: &Path) -> u64 {
    MODEL_BYTES
        + if resolve_runtime_library(dir).is_none() {
            RUNTIME.archive_size
        } else {
            0
        }
}

/// Fetches any missing model file into `dir`, and the CPU runtime when none is already
/// visible from `dir`.
///
/// Each file is written to `*.part`, hashed, then renamed. The runtime archive is hashed,
/// unpacked, and removed. For the app model folder the library lands in the shared
/// `ort-1.31.0` directory; a bench folder keeps it inside `dir`. `progress` is called with
/// the cumulative bytes received and [`download_total`]. `cancel` is checked between chunks;
/// a set flag deletes the part file and returns an error. Files that already match their
/// size and hash are left in place, so a cancelled download can be continued.
///
/// Call this from a worker thread. The blocking HTTP client panics if it is started on a
/// thread that already owns a Tokio runtime.
pub fn download(
    dir: &Path,
    mut progress: impl FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("download cancelled".to_string());
    }
    let total = download_total(dir);
    if installed(dir) {
        progress(total, total);
        return Ok(());
    }
    fs::create_dir_all(dir).map_err(|err| format!("couldn't create {}: {err}", dir.display()))?;
    progress(0, total);

    // The blocking client's default timeout is 30s for the whole body, which a
    // multi-gigabyte model will blow past. None leaves a stalled connection to the
    // cancel flag; connect still has its own limit.
    let client = reqwest::blocking::Client::builder()
        .user_agent("Zephyr")
        .connect_timeout(Duration::from_secs(20))
        .timeout(None)
        .build()
        .map_err(|err| format!("couldn't start the download client: {err}"))?;

    let mut received = 0u64;
    for file in FILES {
        if cancel.load(Ordering::Relaxed) {
            return Err("download cancelled".to_string());
        }
        let dest = dir.join(file.name);
        if file_is_valid(&dest, file)? {
            received += file.size;
            progress(received, total);
            continue;
        }
        download_file(
            &client,
            dir,
            file,
            &mut received,
            total,
            &mut progress,
            cancel,
        )?;
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("download cancelled".to_string());
    }
    if resolve_runtime_library(dir).is_none() {
        download_runtime(&client, dir, &mut received, total, &mut progress, cancel)?;
    }
    Ok(())
}

/// Removes a model directory this module created. Refuses any other path, and does not
/// remove the shared runtime next to the model folders.
pub fn delete(dir: &Path) -> Result<(), String> {
    let name = dir.file_name().and_then(|name| name.to_str());
    if name != Some(MODEL_ID) {
        return Err(format!(
            "refusing to delete {} (not a speech model directory)",
            dir.display()
        ));
    }
    if !dir.exists() {
        return Ok(());
    }
    fs::remove_dir_all(dir).map_err(|err| format!("couldn't delete {}: {err}", dir.display()))
}

fn files_present(dir: &Path, files: &[ModelFile]) -> bool {
    files
        .iter()
        .all(|file| file_len(&dir.join(file.name)) == Some(file.size))
}

fn file_len(path: &Path) -> Option<u64> {
    let meta = fs::metadata(path).ok()?;
    meta.is_file().then_some(meta.len())
}

fn file_is_valid(path: &Path, file: &ModelFile) -> Result<bool, String> {
    if file_len(path) != Some(file.size) {
        return Ok(false);
    }
    Ok(sha256_file(path)? == file.sha256)
}

fn library_file() -> ModelFile {
    ModelFile {
        name: RUNTIME.name,
        size: RUNTIME.size,
        sha256: RUNTIME.sha256,
    }
}

fn download_file(
    client: &reqwest::blocking::Client,
    dir: &Path,
    file: &ModelFile,
    received: &mut u64,
    total: u64,
    progress: &mut impl FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<(), String> {
    let part = dir.join(format!("{}.part", file.name));
    let url = format!(
        "https://huggingface.co/{SOURCE_REPO}/resolve/{REVISION}/{}",
        file.name
    );
    let result = (|| {
        write_part(
            client,
            &PartRequest {
                url: &url,
                label: file.name,
                size: file.size,
            },
            &part,
            received,
            total,
            progress,
            cancel,
        )?;
        let actual = sha256_file(&part)?;
        if actual != file.sha256 {
            return Err(format!(
                "{} hash is {actual}, expected {}",
                file.name, file.sha256
            ));
        }
        let dest = dir.join(file.name);
        if dest.exists() {
            fs::remove_file(&dest)
                .map_err(|err| format!("couldn't replace {}: {err}", dest.display()))?;
        }
        fs::rename(&part, &dest)
            .map_err(|err| format!("couldn't move {} into place: {err}", dest.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}

fn download_runtime(
    client: &reqwest::blocking::Client,
    dir: &Path,
    received: &mut u64,
    total: u64,
    progress: &mut impl FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<(), String> {
    let library = runtime_home(dir).join(RUNTIME.name);
    if file_is_valid(&library, &library_file())? {
        *received += RUNTIME.archive_size;
        progress(*received, total);
        return Ok(());
    }
    let part = dir.join(format!("{}.part", RUNTIME.archive_name));
    let result = (|| {
        write_part(
            client,
            &PartRequest {
                url: RUNTIME.url,
                label: RUNTIME.archive_name,
                size: RUNTIME.archive_size,
            },
            &part,
            received,
            total,
            progress,
            cancel,
        )?;
        let actual = sha256_file(&part)?;
        if actual != RUNTIME.archive_sha256 {
            return Err(format!(
                "{} hash is {actual}, expected {}",
                RUNTIME.archive_name, RUNTIME.archive_sha256
            ));
        }
        install_library(&part, dir)?;
        Ok(())
    })();
    let _ = fs::remove_file(&part);
    result
}

fn install_library(archive: &Path, dir: &Path) -> Result<(), String> {
    let dest_dir = runtime_home(dir);
    fs::create_dir_all(&dest_dir)
        .map_err(|err| format!("couldn't create {}: {err}", dest_dir.display()))?;
    let part = dest_dir.join(format!("{}.part", RUNTIME.name));
    let result = (|| {
        extract_library(archive, &part)?;
        let len = file_len(&part);
        let actual = sha256_file(&part)?;
        if len != Some(RUNTIME.size) || actual != RUNTIME.sha256 {
            let len = len
                .map(|n| n.to_string())
                .unwrap_or_else(|| "missing".to_string());
            return Err(format!(
                "{} is {len} bytes with hash {actual}, expected {} bytes and {}",
                RUNTIME.name, RUNTIME.size, RUNTIME.sha256
            ));
        }
        let dest = dest_dir.join(RUNTIME.name);
        if dest.exists() {
            fs::remove_file(&dest)
                .map_err(|err| format!("couldn't replace {}: {err}", dest.display()))?;
        }
        fs::rename(&part, &dest)
            .map_err(|err| format!("couldn't move {} into place: {err}", dest.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}

#[cfg(windows)]
fn extract_library(archive: &Path, dest: &Path) -> Result<(), String> {
    let file =
        File::open(archive).map_err(|err| format!("couldn't read {}: {err}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|err| format!("couldn't read the speech runtime archive: {err}"))?;
    let mut entry = zip
        .by_name(RUNTIME.member)
        .map_err(|err| format!("speech runtime archive has no {}: {err}", RUNTIME.member))?;
    let mut out =
        File::create(dest).map_err(|err| format!("couldn't write {}: {err}", dest.display()))?;
    std::io::copy(&mut entry, &mut out)
        .map_err(|err| format!("couldn't unpack the speech runtime: {err}"))?;
    out.flush()
        .map_err(|err| format!("couldn't write {}: {err}", dest.display()))?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn extract_library(archive: &Path, dest: &Path) -> Result<(), String> {
    let file =
        File::open(archive).map_err(|err| format!("couldn't read {}: {err}", archive.display()))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    let entries = tar
        .entries()
        .map_err(|err| format!("couldn't read the speech runtime archive: {err}"))?;
    for entry in entries {
        let mut entry =
            entry.map_err(|err| format!("couldn't read the speech runtime archive: {err}"))?;
        let path = entry
            .path()
            .map_err(|err| format!("couldn't read the speech runtime archive: {err}"))?;
        if !archive_member_is(&path, RUNTIME.member) {
            continue;
        }
        if !entry.header().entry_type().is_file() {
            return Err(format!(
                "speech runtime {} is not a file in the archive",
                RUNTIME.member
            ));
        }
        let mut out = File::create(dest)
            .map_err(|err| format!("couldn't write {}: {err}", dest.display()))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|err| format!("couldn't unpack the speech runtime: {err}"))?;
        out.flush()
            .map_err(|err| format!("couldn't write {}: {err}", dest.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dest, fs::Permissions::from_mode(0o755))
                .map_err(|err| format!("couldn't set permissions on {}: {err}", dest.display()))?;
        }
        return Ok(());
    }
    Err(format!("speech runtime archive has no {}", RUNTIME.member))
}

#[cfg(target_os = "macos")]
fn archive_member_is(path: &Path, member: &str) -> bool {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.strip_prefix("./").unwrap_or(&text);
    text == member
}

#[derive(Copy, Clone)]
struct PartRequest<'a> {
    url: &'a str,
    label: &'a str,
    size: u64,
}

fn write_part(
    client: &reqwest::blocking::Client,
    request: &PartRequest<'_>,
    part: &Path,
    received: &mut u64,
    total: u64,
    progress: &mut impl FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<(), String> {
    let PartRequest { url, label, size } = *request;
    let mut response = client
        .get(url)
        .send()
        .map_err(|err| format!("couldn't download {label}: {err}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("couldn't download {label}: HTTP {status}"));
    }

    let mut out =
        File::create(part).map_err(|err| format!("couldn't write {}: {err}", part.display()))?;
    let mut buf = vec![0u8; 256 * 1024];
    let mut got = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("download cancelled".to_string());
        }
        let n = response
            .read(&mut buf)
            .map_err(|err| format!("couldn't download {label}: {err}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])
            .map_err(|err| format!("couldn't write {}: {err}", part.display()))?;
        got += n as u64;
        *received += n as u64;
        progress(*received, total);
    }
    out.flush()
        .map_err(|err| format!("couldn't write {}: {err}", part.display()))?;
    if got != size {
        return Err(format!("{label} is {got} bytes, expected {size}"));
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|err| format!("couldn't read {}: {err}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|err| format!("couldn't read {}: {err}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_size_is_the_sum_of_the_files() {
        let models: u64 = FILES.iter().map(|file| file.size).sum();
        assert_eq!(models, MODEL_BYTES);
        assert_eq!(models + RUNTIME.archive_size, DOWNLOAD_BYTES);
    }

    #[test]
    fn runtime_resolves_from_the_model_dir_before_the_shared_copy() {
        let root = std::env::temp_dir().join(format!("zephyr-voice-rt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let model = root.join(MODEL_ID);
        let own = runtime_library(&model);
        let shared = root.join(RUNTIME.dir).join(RUNTIME.name);
        fs::create_dir_all(own.parent().unwrap()).unwrap();
        fs::create_dir_all(shared.parent().unwrap()).unwrap();
        fs::write(&shared, b"shared").unwrap();
        fs::write(&own, b"local!").unwrap();
        assert_eq!(find_sized_runtime(&model, 6), Some(own.clone()));
        fs::remove_file(&own).unwrap();
        assert_eq!(find_sized_runtime(&model, 6), Some(shared));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn installed_requires_every_file_at_its_size_and_a_runtime() {
        let root = std::env::temp_dir().join(format!("zephyr-voice-inst-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = root.join(MODEL_ID);
        fs::create_dir_all(&dir).unwrap();
        assert!(!installed(&dir));
        for file in FILES {
            // The real weights are gigabytes. This only checks the size gate, so writing the
            // pinned length is not practical; a short file must not count as installed.
            fs::write(dir.join(file.name), b"x").unwrap();
        }
        assert!(!installed(&dir));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn runtime_is_pinned_to_its_versioned_folder() {
        assert_eq!(RUNTIME.dir, format!("ort-{RUNTIME_VERSION}"));
        assert!(RUNTIME.url.contains(RUNTIME_VERSION));
        assert_eq!(RUNTIME.sha256.len(), 64);
        assert_eq!(RUNTIME.archive_sha256.len(), 64);
        let library = runtime_library(Path::new("models"));
        assert_eq!(
            library,
            Path::new("models").join(RUNTIME.dir).join(RUNTIME.name)
        );
    }

    #[test]
    fn model_dir_is_under_models() {
        let dir = model_dir(Path::new("app-data"));
        assert_eq!(dir, Path::new("app-data").join("models").join(MODEL_ID));
    }

    #[test]
    fn a_missing_directory_is_not_installed() {
        assert!(!installed(Path::new("this-model-dir-does-not-exist")));
    }

    #[test]
    fn present_means_every_file_has_its_size() {
        let dir = std::env::temp_dir().join(format!("zephyr-voice-size-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let files = [ModelFile {
            name: "a.bin",
            size: 3,
            sha256: "unused",
        }];
        fs::write(dir.join("a.bin"), b"abc").unwrap();
        assert!(files_present(&dir, &files));
        fs::write(dir.join("a.bin"), b"ab").unwrap();
        assert!(!files_present(&dir, &files));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sha256_of_abc() {
        let dir = std::env::temp_dir().join(format!("zephyr-voice-hash-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("abc.txt");
        fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_raised_cancel_flag_stops_before_the_network() {
        let cancel = AtomicBool::new(true);
        let err = download(Path::new("unused"), |_, _| panic!("progress"), &cancel).unwrap_err();
        assert!(err.to_lowercase().contains("cancel"), "{err}");
    }

    #[test]
    fn delete_removes_only_a_model_directory() {
        let root = std::env::temp_dir().join(format!("zephyr-voice-del-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = model_dir(&root);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("vocab.txt"), b"x").unwrap();
        delete(&dir).unwrap();
        assert!(!dir.exists());
        delete(&dir).unwrap();
        assert!(delete(&root).unwrap_err().contains("refusing"));
        let _ = fs::remove_dir_all(&root);
    }
}
