//! Root-capability reads. Unix uses openat/NOFOLLOW; Windows pins every
//! directory handle against writes/deletion and opens reparse points themselves
//! before examining metadata. No resource body is read before authorization.

use std::fs::File;
use std::io;
use std::path::{Component, Path, PathBuf};

pub(super) fn absolute_root(path: &Path) -> io::Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut result = PathBuf::new();
    if path.components().count() > 64 {
        return Err(denied());
    }
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => return Err(denied()),
            other => result.push(other.as_os_str()),
        }
    }
    Ok(result)
}

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "rooted resource traversal refused",
    )
}

#[cfg(unix)]
pub(super) struct Root {
    path: PathBuf,
    directory: rustix::fd::OwnedFd,
}

#[cfg(unix)]
impl Root {
    pub(super) fn open(path: &Path) -> io::Result<Self> {
        use rustix::fs::{Mode, OFlags, open, openat};
        let path = absolute_root(path)?;
        let mut directory = open(
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        for part in path.components() {
            match part {
                Component::RootDir => {}
                Component::Normal(name) => {
                    directory = openat(
                        &directory,
                        name,
                        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    )?;
                }
                _ => return Err(denied()),
            }
        }
        Ok(Self { path, directory })
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    pub(super) fn open_file(&self, relative: &Path) -> io::Result<File> {
        use rustix::fs::{Mode, OFlags, openat};
        let parts = relative.components().collect::<Vec<_>>();
        let Some((last, parents)) = parts.split_last() else {
            return Err(denied());
        };
        let mut directories = Vec::new();
        for part in parents {
            let Component::Normal(name) = part else {
                return Err(denied());
            };
            let parent = directories.last().unwrap_or(&self.directory);
            exact_name(parent, name)?;
            let opened = openat(
                parent,
                *name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?;
            directories.push(opened);
        }
        let Component::Normal(name) = last else {
            return Err(denied());
        };
        let parent = directories.last().unwrap_or(&self.directory);
        exact_name(parent, name)?;
        let file: File = openat(
            parent,
            *name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?
        .into();
        if !file.metadata()?.is_file() {
            return Err(denied());
        }
        Ok(file)
    }
}

#[cfg(unix)]
fn exact_name(directory: &rustix::fd::OwnedFd, name: &std::ffi::OsStr) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    // Validate an exact canonical spelling, never choose another entry. The
    // enumeration is descriptor-rooted and bounded, including on case-folding
    // filesystems where an alias must not bypass a dependency pin.
    for (count, entry) in rustix::fs::Dir::read_from(directory)?.enumerate() {
        if count >= 4096 {
            return Err(denied());
        }
        if entry?.file_name().to_bytes() == name.as_bytes() {
            return Ok(());
        }
    }
    Err(denied())
}

#[cfg(windows)]
pub(super) struct Root {
    path: PathBuf,
    _directories: Vec<File>,
}

#[cfg(windows)]
fn windows_open(path: &Path, directory: bool) -> io::Result<File> {
    use std::fs::OpenOptions;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    // Deny WRITE and DELETE sharing while the handle is held. This prevents
    // renaming a checked ancestor or installing a reparse point underneath it.
    let file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .custom_flags(0x0010_0000 | 0x0020_0000 | if directory { 0x0200_0000 } else { 0 })
        .open(path)?;
    let metadata = file.metadata()?;
    if metadata.file_attributes() & (0x400 | 0x1000 | 0x400000) != 0
        || metadata.is_dir() != directory
        || (!directory && !metadata.is_file())
    {
        return Err(denied());
    }
    Ok(file)
}

#[cfg(windows)]
fn handle_identity(file: &File) -> io::Result<PathBuf> {
    use std::ffi::{OsString, c_void};
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[link_name = "GetFinalPathNameByHandleW"]
        fn final_path_name(handle: *mut c_void, buffer: *mut u16, size: u32, flags: u32) -> u32;
    }
    let mut buffer = vec![0u16; 32768];
    // SAFETY: `file` owns the live native handle and the writable UTF-16 buffer
    // has exactly this bounded capacity. The query reads metadata, not a body.
    // VOLUME_NAME_GUID avoids mutable DOS-drive and short-name identities.
    let length = unsafe {
        final_path_name(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            1,
        )
    };
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize >= buffer.len() {
        return Err(denied());
    }
    Ok(PathBuf::from(OsString::from_wide(
        &buffer[..length as usize],
    )))
}

#[cfg(windows)]
fn volume_prefix(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    name.len() == 44
        && name.starts_with("Volume{")
        && name.ends_with('}')
        && name[7..43]
            .bytes()
            .all(|b| b.is_ascii_hexdigit() || b == b'-')
}

#[cfg(windows)]
impl Root {
    pub(super) fn open(path: &Path) -> io::Result<Self> {
        use std::path::Prefix;
        let path = absolute_root(path)?;
        match path.components().next() {
            Some(Component::Prefix(p))
                if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
                    || matches!(p.kind(), Prefix::Verbatim(name) if volume_prefix(name)) => {}
            _ => return Err(denied()),
        }
        let mut current = PathBuf::new();
        let mut directories = Vec::new();
        for part in path.components() {
            match part {
                Component::Prefix(_) => current.push(part.as_os_str()),
                Component::RootDir => {
                    current.push(part.as_os_str());
                    let directory = windows_open(&current, true)?;
                    current = handle_identity(&directory)?;
                    directories.push(directory);
                }
                Component::Normal(name) if super::safe_component(name) => {
                    current.push(name);
                    let directory = windows_open(&current, true)?;
                    let identity = handle_identity(&directory)?;
                    if identity.parent() != current.parent() {
                        return Err(denied());
                    }
                    current = identity;
                    directories.push(directory);
                }
                _ => return Err(denied()),
            }
        }
        let path = current;
        Ok(Self {
            path,
            _directories: directories,
        })
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    pub(super) fn open_file(&self, relative: &Path) -> io::Result<File> {
        let parts = relative.components().collect::<Vec<_>>();
        let Some((last, parents)) = parts.split_last() else {
            return Err(denied());
        };
        let mut current = self.path.clone();
        let mut directories = Vec::new();
        for part in parents {
            let Component::Normal(name) = part else {
                return Err(denied());
            };
            current.push(name);
            directories.push(windows_open(&current, true)?);
        }
        let Component::Normal(name) = last else {
            return Err(denied());
        };
        current.push(name);
        let file = windows_open(&current, false)?;
        let canonical = handle_identity(&file)?;
        if canonical != current {
            return Err(denied());
        }
        Ok(file)
    }
}

#[cfg(not(any(unix, windows)))]
pub(super) struct Root {
    path: PathBuf,
}
#[cfg(not(any(unix, windows)))]
impl Root {
    pub(super) fn open(_: &Path) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "bound resource reads require a rooted native backend",
        ))
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    pub(super) fn open_file(&self, _: &Path) -> io::Result<File> {
        Err(denied())
    }
}
