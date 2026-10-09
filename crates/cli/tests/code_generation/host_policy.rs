//! Test-only host reuse and original retention for the three older wrappers.
//! The shared mutex covers this test process; other processes need coordination.
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;

use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};

use super::CommandOutputExt;

static SHARED_HOST_RUN: Mutex<()> = Mutex::new(());
const MAX_SOURCE_FILE: u64 = 64 * 1024 * 1024;
const MAX_SOURCE_TOTAL: u64 = 256 * 1024 * 1024;

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn choose_target(directory: &Path, cwd: &Path, explicit: Option<&OsStr>) -> io::Result<PathBuf> {
    match explicit {
        Some(value) if value.is_empty() => Err(invalid(
            "FERRULE_CODEGEN_HOST_TARGET_DIR must name a dedicated host target",
        )),
        Some(value) => {
            let path = PathBuf::from(value);
            Ok(if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            })
        }
        None => Ok(directory.join("cargo-target")),
    }
}

// Resolve existing ancestors before `..`, without creating a cache.
fn existing_identity(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(invalid("absolute target identity required"));
    }
    let mut identity = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                identity.pop();
            }
            other => {
                identity.push(other.as_os_str());
                match fs::symlink_metadata(&identity) {
                    Ok(_) => {
                        identity = identity.canonicalize()?;
                        if !identity.is_dir() {
                            return Err(invalid("host target ancestor must be a directory"));
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
        }
    }
    Ok(identity)
}

fn dedicated(target: &Path, outer: Option<&Path>, executable: &Path) -> io::Result<()> {
    if outer.is_some_and(|outer| target.starts_with(outer) || outer.starts_with(target)) {
        return Err(invalid(
            "generated host target must not overlap the outer Cargo target",
        ));
    }
    if executable.starts_with(target) {
        return Err(invalid(
            "generated host target must not contain the active test executable",
        ));
    }
    Ok(())
}

fn checked_target(directory: &Path) -> io::Result<(PathBuf, bool)> {
    let cwd = std::env::current_dir()?;
    let explicit = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR");
    let target = choose_target(directory, &cwd, explicit.as_deref())?;
    if explicit.is_some() {
        let target = existing_identity(&target)?;
        let outer = std::env::var_os("CARGO_TARGET_DIR")
            .map(|value| {
                if value.is_empty() {
                    return Err(invalid("outer CARGO_TARGET_DIR is empty"));
                }
                let path = PathBuf::from(value);
                existing_identity(&if path.is_absolute() {
                    path
                } else {
                    cwd.join(path)
                })
            })
            .transpose()?;
        dedicated(
            &target,
            outer.as_deref(),
            &std::env::current_exe()?.canonicalize()?,
        )?;
        let case = existing_identity(&if directory.is_absolute() {
            directory.to_path_buf()
        } else {
            cwd.join(directory)
        })?;
        if target.starts_with(&case) || case.starts_with(&target) {
            return Err(invalid(
                "shared host target must be separate from retained case sources",
            ));
        }
        Ok((target, true))
    } else {
        Ok((target, false))
    }
}

pub(super) fn preflight_environment(directory: &Path) -> io::Result<()> {
    // Called before TempDir creates, removes or writes anything.
    checked_target(directory).map(|_| ())
}

fn keep(value: Option<&OsStr>) -> bool {
    value == Some(OsStr::new("1"))
}

pub(super) fn keep_artifacts() -> bool {
    keep(std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref())
}

fn json_file(path: &Path, value: &Json) -> io::Result<()> {
    let mut output = OpenOptions::new().write(true).create_new(true).open(path)?;
    serde_json::to_writer_pretty(&mut output, value)?;
    output.write_all(b"\n")?;
    output.flush()?;
    output.sync_all()
}

fn metadata(path: &Path) -> io::Result<Json> {
    let s = fs::symlink_metadata(path)?;
    if !s.is_file() || s.file_type().is_symlink() {
        return Err(invalid("original source must be one regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(
            json!({"dev":s.dev(), "ino":s.ino(), "mode":s.mode(), "uid":s.uid(),
            "gid":s.gid(), "nlink":s.nlink(), "bytes":s.len(), "blocks":s.blocks(),
            "mtime":s.mtime(), "mtime_nsec":s.mtime_nsec(), "ctime":s.ctime(), "ctime_nsec":s.ctime_nsec()}),
        )
    }
    #[cfg(not(unix))]
    {
        Ok(
            json!({"bytes":s.len(), "modified":format!("{:?}", s.modified()?),
        "readonly":s.permissions().readonly()}),
        )
    }
}

fn hash_file(path: &Path) -> io::Result<(u64, String)> {
    let mut source = File::open(path)?;
    let mut hash = Sha256::new();
    let mut length = 0;
    let mut buffer = [0_u8; 65_536];
    loop {
        let count = source.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        length += count as u64;
        if length > MAX_SOURCE_FILE {
            return Err(invalid("original source exceeds file bound"));
        }
        hash.update(&buffer[..count]);
    }
    Ok((length, format!("{:x}", hash.finalize())))
}

fn copy_sources(root: &Path, originals: &Path) -> io::Result<Vec<Json>> {
    fn visit(
        root: &Path,
        current: &Path,
        originals: &Path,
        total: &mut u64,
        rows: &mut Vec<Json>,
    ) -> io::Result<()> {
        let mut entries = fs::read_dir(current)?.collect::<io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let name = entry.file_name();
            if [
                "target",
                "cargo-target",
                "bin",
                "obj",
                ".tmp",
                ".dotnet-home",
                ".nuget-packages",
                "host-originals",
            ]
            .iter()
            .any(|skip| name == OsStr::new(skip))
            {
                continue;
            }
            let ty = entry.file_type()?;
            if ty.is_symlink() {
                return Err(invalid("source traversal refuses symlinks"));
            }
            if ty.is_dir() {
                visit(root, &path, originals, total, rows)?;
                continue;
            }
            let before = metadata(&path)?;
            let identity = hash_file(&path)?;
            if metadata(&path)? != before {
                return Err(invalid("source changed before retention"));
            }
            *total += identity.0;
            if *total > MAX_SOURCE_TOTAL {
                return Err(invalid("original source set exceeds total bound"));
            }
            let relative = path.strip_prefix(root).map_err(io::Error::other)?;
            let retained = originals.join(relative);
            fs::create_dir_all(
                retained
                    .parent()
                    .ok_or_else(|| invalid("source parent missing"))?,
            )?;
            let mut source = File::open(&path)?.take(MAX_SOURCE_FILE + 1);
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&retained)?;
            if io::copy(&mut source, &mut output)? > MAX_SOURCE_FILE {
                return Err(invalid("original source grew beyond file bound"));
            }
            output.flush()?;
            output.sync_all()?;
            let retained_identity = hash_file(&retained)?;
            if identity != retained_identity
                || identity != hash_file(&path)?
                || metadata(&path)? != before
            {
                return Err(invalid("complete retained source/body fence differs"));
            }
            rows.push(
                json!({"relative":relative, "original":path, "original_stat":before,
                "bytes":identity.0, "sha256":identity.1, "retained":retained,
                "retained_stat":metadata(&retained)?}),
            );
        }
        Ok(())
    }
    let mut total = 0;
    let mut rows = Vec::new();
    visit(root, root, originals, &mut total, &mut rows)?;
    Ok(rows)
}

fn configure_cargo(command: &mut Command, target: &Path) {
    command
        .env("CARGO_TARGET_DIR", target)
        .env("CARGO_BUILD_JOBS", "1")
        .env("CARGO_INCREMENTAL", "0");
}

pub(super) fn recorded_output(
    command: &mut Command,
    directory: &Path,
    name: &str,
) -> io::Result<Output> {
    let (target, shared) = checked_target(directory)?;
    let cargo = command.get_program() == OsStr::new("cargo");
    let _shared_run = if cargo && shared {
        Some(
            SHARED_HOST_RUN
                .lock()
                .map_err(|_| io::Error::other("shared host run lock poisoned"))?,
        )
    } else {
        None
    };
    if cargo {
        configure_cargo(command, &target);
    }
    let originals = directory.join("host-originals");
    fs::create_dir_all(&originals)?;
    let invocation = originals.join(name);
    fs::create_dir(&invocation)?;
    let sources = copy_sources(directory, &invocation.join("prebuild-sources"))?;
    json_file(
        &invocation.join("command.original.json"),
        &json!({
        "command_debug":format!("{command:?}"), "program":command.get_program(),
        "arguments":command.get_args().collect::<Vec<_>>(), "cwd":command.get_current_dir(),
        "configured_host_target":if cargo {Some(&target)} else {None},
        "shared_cache_in_this_process":cargo && shared, "source_originals":sources,
        "inherited_rustflags":std::env::var_os("RUSTFLAGS"),
        "inherited_encoded_rustflags":std::env::var_os("CARGO_ENCODED_RUSTFLAGS")}),
    )?;
    let output = match command.isolated_output() {
        Ok(output) => output,
        Err(error) => {
            json_file(
                &invocation.join("spawn-error.original.json"),
                &json!({"kind":format!("{:?}",error.kind()), "debug":format!("{error:?}"), "display":error.to_string()}),
            )?;
            return Err(error);
        }
    };
    for (name, bytes) in [
        ("stdout.original.bin", &output.stdout),
        ("stderr.original.bin", &output.stderr),
    ] {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(invocation.join(name))?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
    }
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        output.status.signal()
    };
    #[cfg(not(unix))]
    let signal: Option<i32> = None;
    json_file(
        &invocation.join("status.original.json"),
        &json!({"success":output.status.success(),
        "code":output.status.code(), "signal":signal, "debug":format!("{:?}",output.status)}),
    )?;
    Ok(output)
}

#[test]
fn default_isolated_target_keeps_the_older_wrapper_path() {
    assert_eq!(
        choose_target(Path::new("/case"), Path::new("/parent"), None).unwrap(),
        Path::new("/case/cargo-target")
    );
}

#[test]
fn relative_explicit_target_is_bound_before_host_cwd_changes() {
    assert_eq!(
        choose_target(
            Path::new("/case"),
            Path::new("/parent"),
            Some(OsStr::new("shared-host"))
        )
        .unwrap(),
        Path::new("/parent/shared-host")
    );
}

#[test]
fn empty_explicit_target_is_a_configuration_refusal() {
    assert_eq!(
        choose_target(
            Path::new("/case"),
            Path::new("/parent"),
            Some(OsStr::new(""))
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn resolved_outer_overlap_and_active_executable_are_refused() {
    let outer = Path::new("/build/target");
    let executable = Path::new("/build/target/debug/deps/test");
    for target in [outer, Path::new("/build/target/host"), Path::new("/build")] {
        assert_eq!(
            dedicated(target, Some(outer), executable)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
    assert_eq!(
        dedicated(outer, None, executable).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    dedicated(Path::new("/build/host-target"), Some(outer), executable).unwrap();
}

#[test]
fn cache_policy_preserves_host_arguments_and_warning_flags() {
    let mut command = Command::new("cargo");
    command
        .args(["run", "--quiet"])
        .env("RUSTFLAGS", "-D warnings");
    configure_cargo(&mut command, Path::new("/dedicated-host"));
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        vec![OsStr::new("run"), OsStr::new("--quiet")]
    );
    let env = command
        .get_envs()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        env[OsStr::new("RUSTFLAGS")],
        Some(OsStr::new("-D warnings"))
    );
    assert_eq!(env[OsStr::new("CARGO_BUILD_JOBS")], Some(OsStr::new("1")));
    assert_eq!(env[OsStr::new("CARGO_INCREMENTAL")], Some(OsStr::new("0")));
    assert_eq!(
        env[OsStr::new("CARGO_TARGET_DIR")],
        Some(OsStr::new("/dedicated-host"))
    );
}

#[test]
fn retention_is_explicit_one_and_cleanup_remains_the_default() {
    assert!(!keep(None));
    assert!(!keep(Some(OsStr::new(""))));
    assert!(!keep(Some(OsStr::new("0"))));
    assert!(keep(Some(OsStr::new("1"))));
}

#[cfg(unix)]
#[test]
fn symlink_then_parent_cannot_hide_an_outer_target_alias() -> io::Result<()> {
    let directory = super::TempDir::new("host_policy_alias")?;
    let outer = directory.0.join("outer");
    let elsewhere = directory.0.join("elsewhere");
    fs::create_dir_all(outer.join("child"))?;
    fs::create_dir(&elsewhere)?;
    std::os::unix::fs::symlink(outer.join("child"), elsewhere.join("alias"))?;
    let target = existing_identity(&elsewhere.join("alias/.."))?;
    assert_eq!(target, outer.canonicalize()?);
    assert_eq!(
        dedicated(
            &target,
            Some(&outer.canonicalize()?),
            Path::new("/unrelated/test")
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::InvalidInput
    );
    Ok(())
}

#[test]
fn complete_source_retention_excludes_build_cache_bodies() -> io::Result<()> {
    let directory = super::TempDir::new("host_policy_sources")?;
    fs::create_dir_all(directory.0.join("rust/src"))?;
    fs::write(directory.0.join("project.json"), b"{\"identity\":true}\n")?;
    fs::write(directory.0.join("rust/src/main.rs"), b"fn main() {}\n")?;
    for excluded in [
        "target",
        "cargo-target",
        "bin",
        "obj",
        ".tmp",
        ".dotnet-home",
        ".nuget-packages",
    ] {
        fs::create_dir(directory.0.join(excluded))?;
        fs::write(directory.0.join(excluded).join("cache.bin"), b"excluded")?;
    }
    let originals = directory.0.join("host-originals/source-control");
    fs::create_dir_all(&originals)?;
    let rows = copy_sources(&directory.0, &originals)?;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        fs::read(originals.join("project.json"))?,
        b"{\"identity\":true}\n"
    );
    assert_eq!(
        fs::read(originals.join("rust/src/main.rs"))?,
        b"fn main() {}\n"
    );
    assert!(
        rows.iter()
            .all(|row| row["sha256"].as_str().is_some_and(|hash| hash.len() == 64))
    );
    assert!(!originals.join("target").exists());
    Ok(())
}

#[test]
fn failed_spawn_retains_prebuild_sources_and_complete_error() -> io::Result<()> {
    let directory = super::TempDir::new("host_policy_spawn_error")?;
    fs::write(
        directory.0.join("project.json"),
        b"{\"failure-control\":true}\n",
    )?;
    let mut command = Command::new(directory.0.join("absent-program"));
    let error = recorded_output(&mut command, &directory.0, "missing-host").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    let invocation = directory.0.join("host-originals/missing-host");
    assert_eq!(
        fs::read(invocation.join("prebuild-sources/project.json"))?,
        b"{\"failure-control\":true}\n"
    );
    let retained: Json =
        serde_json::from_slice(&fs::read(invocation.join("spawn-error.original.json"))?)?;
    assert_eq!(retained["kind"], "NotFound");
    assert_eq!(retained["debug"], format!("{error:?}"));
    assert_eq!(retained["display"], error.to_string());
    assert!(invocation.join("command.original.json").is_file());
    Ok(())
}
