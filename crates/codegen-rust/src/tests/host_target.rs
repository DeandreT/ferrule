//! Test-only opt-in reuse; the caller must provide a dedicated host target.
//! The mutex covers Cargo compilation and execution for this test process.
//! Other processes using the cache still require an exclusive external lease.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output};
use std::sync::{Mutex, MutexGuard};

static SHARED_HOST_RUN: Mutex<()> = Mutex::new(());

pub(super) trait GeneratedHostCommand {
    fn generated_host_output(&mut self, directory: &Path) -> io::Result<Output>;
    fn generated_host_status(&mut self, directory: &Path) -> io::Result<ExitStatus>;
}

impl GeneratedHostCommand for Command {
    fn generated_host_output(&mut self, directory: &Path) -> io::Result<Output> {
        let _shared_run = configure(self, directory)?;
        self.output()
    }

    fn generated_host_status(&mut self, directory: &Path) -> io::Result<ExitStatus> {
        let _shared_run = configure(self, directory)?;
        self.status()
    }
}

fn configure(
    command: &mut Command,
    directory: &Path,
) -> io::Result<Option<MutexGuard<'static, ()>>> {
    let cwd = std::env::current_dir()?;
    let explicit = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR");
    let target = choose_target(directory, &cwd, explicit.as_deref())?;
    let shared_run = if explicit.is_some() {
        reject_outer_target(&target, &cwd)?;
        Some(
            SHARED_HOST_RUN
                .lock()
                .map_err(|_| io::Error::other("generated host run lock poisoned"))?,
        )
    } else {
        None
    };
    command
        .env("CARGO_TARGET_DIR", target)
        .env("CARGO_BUILD_JOBS", "1")
        .env("CARGO_INCREMENTAL", "0");
    Ok(shared_run)
}

fn choose_target(directory: &Path, cwd: &Path, explicit: Option<&OsStr>) -> io::Result<PathBuf> {
    match explicit {
        Some(value) if value.is_empty() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
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
        None => Ok(directory.join("target")),
    }
}

// Resolve existing ancestors, including symlinks, without creating a cache.
fn existing_identity(path: &Path) -> io::Result<PathBuf> {
    let mut existing = path;
    let mut suffix = Vec::new();
    while !existing.exists() {
        let name = existing.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "host target has no existing ancestor",
            )
        })?;
        suffix.push(name);
        existing = existing.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "host target has no parent")
        })?;
    }
    let mut resolved = existing.canonicalize()?;
    for name in suffix.into_iter().rev() {
        resolved.push(name);
    }
    Ok(resolved)
}

fn reject_outer_target(target: &Path, cwd: &Path) -> io::Result<()> {
    let target = existing_identity(target)?;
    if let Some(outer) = std::env::var_os("CARGO_TARGET_DIR") {
        let outer = PathBuf::from(outer);
        let outer = if outer.is_absolute() {
            outer
        } else {
            cwd.join(outer)
        };
        if target == existing_identity(&outer)? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "generated host target must differ from the outer Cargo target",
            ));
        }
    }
    // Also catch an outer target supplied by Cargo configuration rather than env.
    if std::env::current_exe()?
        .canonicalize()?
        .starts_with(&target)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "generated host target must not contain the active test executable",
        ));
    }
    Ok(())
}

#[test]
fn isolated_default_does_not_depend_on_outer_cargo_target() {
    let directory = Path::new("/test/isolated-host");
    assert_eq!(
        choose_target(directory, Path::new("/test/parent"), None).unwrap(),
        directory.join("target")
    );
}

#[test]
fn relative_explicit_cache_is_resolved_before_changing_to_host_directory() {
    let target = choose_target(
        Path::new("/test/host"),
        Path::new("/test/parent"),
        Some(OsStr::new("reused-host-cache")),
    )
    .unwrap();
    assert_eq!(target, Path::new("/test/parent/reused-host-cache"));
}

#[test]
fn empty_explicit_cache_is_a_configuration_error() {
    assert_eq!(
        choose_target(
            Path::new("/test/host"),
            Path::new("/test/parent"),
            Some(OsStr::new(""))
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::InvalidInput
    );
}
