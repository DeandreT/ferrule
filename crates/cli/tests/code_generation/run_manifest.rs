//! Compact inventory of a retained portable-cohort run.
//!
//! When `FERRULE_CODEGEN_KEEP_ARTIFACTS=1` keeps a run directory, the
//! execution bindings record every command they launched, keep a bounded tail
//! of its output under `logs/`, and write `RUN-MANIFEST.json`: each command
//! with its exit status, the size and SHA-256 of every retained file, and a
//! summary of the build caches and .NET tool state. Those caches are restorable
//! rather than run evidence, so a retained run drops them after summarizing.
use super::*;
use serde::Serialize;
use sha2::{Digest as _, Sha256};
use std::fs;

pub(super) const MANIFEST: &str = "RUN-MANIFEST.json";
const MANIFEST_VERSION: u32 = 1;
/// Bytes of each command's stdout and stderr kept in a retained run.
const LOG_TAIL: usize = 32 * 1024;

#[derive(Serialize)]
struct RunManifest<'a> {
    version: u32,
    run: &'a str,
    commands: &'a [CommandRecord],
    files: Vec<FileRecord>,
    dropped_caches: Vec<DirectorySummary>,
}

#[derive(Serialize)]
struct CommandRecord {
    label: String,
    program: String,
    arguments: Vec<String>,
    exit_code: Option<i32>,
    success: bool,
    stdout_bytes: usize,
    stderr_bytes: usize,
}

#[derive(Serialize)]
struct FileRecord {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Serialize)]
struct DirectorySummary {
    path: String,
    files: usize,
    bytes: u64,
}

/// The commands one run launched, in completion order.
pub(super) struct Recorder {
    root: PathBuf,
    keep: bool,
    commands: Mutex<Vec<CommandRecord>>,
}

fn tail(bytes: &[u8]) -> &[u8] {
    &bytes[bytes.len().saturating_sub(LOG_TAIL)..]
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

/// Every regular file below `directory`, in path order.
fn regular_files(directory: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<io::Result<Vec<_>>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let kind = entry.file_type()?;
        if kind.is_dir() {
            regular_files(&entry.path(), files)?;
        } else if kind.is_file() {
            files.push(entry.path());
        }
    }
    Ok(())
}

impl Recorder {
    pub(super) fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            keep: host_policy::keep_artifacts(),
            commands: Mutex::new(Vec::new()),
        }
    }

    /// Record one finished command under a unique `label`.
    pub(super) fn record(&self, label: &str, command: &Command, output: &Output) -> io::Result<()> {
        if self.keep {
            let logs = self.root.join("logs");
            fs::create_dir_all(&logs)?;
            fs::write(
                logs.join(format!("{label}.stdout.txt")),
                tail(&output.stdout),
            )?;
            fs::write(
                logs.join(format!("{label}.stderr.txt")),
                tail(&output.stderr),
            )?;
        }
        let record = CommandRecord {
            label: label.into(),
            program: command.get_program().to_string_lossy().into_owned(),
            arguments: command
                .get_args()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect(),
            exit_code: output.status.code(),
            success: output.status.success(),
            stdout_bytes: output.stdout.len(),
            stderr_bytes: output.stderr.len(),
        };
        self.commands
            .lock()
            .map_err(|_| io::Error::other("command recorder poisoned"))?
            .push(record);
        Ok(())
    }

    /// For a retained run, summarize and drop the caches named in `caches`
    /// (relative to the run directory), then write its manifest.
    pub(super) fn finish(self, run: &str, caches: &[PathBuf]) -> TestResult<()> {
        if !self.keep {
            return Ok(());
        }
        let mut dropped_caches = Vec::new();
        for cache in caches {
            let path = self.root.join(cache);
            if !path.is_dir() {
                continue;
            }
            let mut files = Vec::new();
            regular_files(&path, &mut files)?;
            let bytes = files
                .iter()
                .map(|file| fs::metadata(file).map(|metadata| metadata.len()))
                .sum::<io::Result<u64>>()?;
            fs::remove_dir_all(&path)?;
            dropped_caches.push(DirectorySummary {
                path: relative(&self.root, &path),
                files: files.len(),
                bytes,
            });
        }
        let mut paths = Vec::new();
        regular_files(&self.root, &mut paths)?;
        let files = paths
            .iter()
            .map(|path| {
                let bytes = fs::read(path)?;
                Ok(FileRecord {
                    path: relative(&self.root, path),
                    bytes: bytes.len() as u64,
                    sha256: format!("{:x}", Sha256::digest(&bytes)),
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        let commands = self
            .commands
            .into_inner()
            .map_err(|_| "command recorder poisoned")?;
        let manifest = RunManifest {
            version: MANIFEST_VERSION,
            run,
            commands: &commands,
            files,
            dropped_caches,
        };
        fs::write(
            self.root.join(MANIFEST),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        Ok(())
    }
}

#[test]
fn log_tails_are_bounded() {
    let long = vec![b'x'; LOG_TAIL + 10];
    assert_eq!(tail(&long).len(), LOG_TAIL);
    assert_eq!(tail(b"short"), b"short");
}

#[test]
fn retained_runs_inventory_files_and_drop_caches() -> TestResult<()> {
    let temp = TempDir::new("run_manifest")?;
    let root = temp.0.canonicalize()?;
    fs::create_dir_all(root.join("cache/nested"))?;
    fs::write(root.join("cache/nested/blob"), b"cached")?;
    fs::write(root.join("evidence.json"), b"{}")?;
    let recorder = Recorder {
        root: root.clone(),
        keep: true,
        commands: Mutex::new(Vec::new()),
    };
    let mut command = Command::new("true");
    command.arg("--flag");
    let output = command.output()?;
    recorder.record("probe", &command, &output)?;
    recorder.finish("probe-run", &[PathBuf::from("cache")])?;
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(root.join(MANIFEST))?)?;
    assert_eq!(manifest["run"], "probe-run");
    assert_eq!(manifest["commands"][0]["arguments"][0], "--flag");
    assert_eq!(manifest["commands"][0]["exit_code"], 0);
    assert_eq!(
        manifest["dropped_caches"][0],
        serde_json::json!({"path": "cache", "files": 1, "bytes": 6})
    );
    let files: Vec<_> = manifest["files"]
        .as_array()
        .ok_or("files")?
        .iter()
        .map(|file| file["path"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(
        files,
        [
            "evidence.json",
            "logs/probe.stderr.txt",
            "logs/probe.stdout.txt"
        ]
    );
    assert!(!root.join("cache").exists());
    Ok(())
}
