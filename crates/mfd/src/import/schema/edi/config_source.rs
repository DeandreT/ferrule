//! Bounded resolution of filesystem and adjacent ZIP-packaged EDI configurations.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 512;
const MAX_EXTRACTED_BYTES: u64 = 32 * 1024 * 1024;

pub(super) struct ResolvedConfig {
    path: PathBuf,
    _archive: Option<ExtractedArchive>,
}

impl ResolvedConfig {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

struct ExtractedArchive {
    root: PathBuf,
}

impl Drop for ExtractedArchive {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub(super) fn resolve(mfd_path: &Path, declared: &str) -> Result<ResolvedConfig, String> {
    let portable = declared
        .strip_prefix("altova://edi_config/")
        .unwrap_or(declared)
        .replace('\\', "/");
    let relative = bounded_relative_path(&portable)
        .ok_or_else(|| format!("configuration path `{declared}` is not a bounded relative path"))?;
    let roots = config_roots(mfd_path)?;
    let direct = resolve_matches(&roots, &relative);
    match direct.as_slice() {
        [path] => {
            return Ok(ResolvedConfig {
                path: path.clone(),
                _archive: None,
            });
        }
        [] => {}
        _ => {
            return Err(format!(
                "configuration `{declared}` resolves to multiple nearby installations"
            ));
        }
    }

    let packages = archive_candidates(&roots, &relative);
    let mut extracted = Vec::new();
    let mut errors = Vec::new();
    for (archive, entries) in packages {
        match extract_matching_archive(&archive, &entries) {
            Ok(Some((archive, path))) => extracted.push((archive, path)),
            Ok(None) => {}
            Err(error) => errors.push(format!("{}: {error}", archive.display())),
        }
    }
    match extracted.len() {
        1 => {
            let (archive, path) = extracted.pop().ok_or_else(|| {
                "internal packaged EDI configuration resolution error".to_string()
            })?;
            Ok(ResolvedConfig {
                path,
                _archive: Some(archive),
            })
        }
        0 if errors.is_empty() => Err(format!("configuration `{declared}` was not found")),
        0 => Err(format!(
            "configuration `{declared}` was found in an invalid package ({})",
            errors.join("; ")
        )),
        _ => Err(format!(
            "configuration `{declared}` resolves to multiple nearby packages"
        )),
    }
}

fn config_roots(mfd_path: &Path) -> Result<Vec<PathBuf>, String> {
    let unresolved_base = mfd_path.parent().unwrap_or_else(|| Path::new("."));
    let base = std::fs::canonicalize(unresolved_base)
        .map_err(|error| format!("could not resolve mapping directory ({error})"))?;
    let mut roots = vec![base.to_path_buf()];
    if let Some(root) = std::env::var_os("FERRULE_EDI_CONFIG_DIR") {
        roots.push(PathBuf::from(root));
    }
    for ancestor in base.ancestors().take(12) {
        roots.push(ancestor.to_path_buf());
        roots.push(ancestor.join("MapForceEDI"));
        if let Ok(entries) = std::fs::read_dir(ancestor) {
            roots.extend(
                entries
                    .take(128)
                    .filter_map(Result::ok)
                    .filter_map(|entry| {
                        entry
                            .file_type()
                            .ok()
                            .filter(|file_type| file_type.is_dir())
                            .map(|_| entry)
                    })
                    .map(|entry| entry.path().join("MapForceEDI")),
            );
        }
    }
    roots.sort();
    roots.dedup();
    Ok(roots)
}

fn resolve_matches(roots: &[PathBuf], relative: &Path) -> Vec<PathBuf> {
    let mut matches = roots
        .iter()
        .filter_map(|root| resolve_case_insensitive(root, relative))
        .filter_map(|path| std::fs::canonicalize(path).ok())
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();
    matches
}

fn archive_candidates(roots: &[PathBuf], relative: &Path) -> BTreeMap<PathBuf, BTreeSet<PathBuf>> {
    let components = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_os_string()),
            Component::CurDir => None,
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut candidates = BTreeMap::<PathBuf, BTreeSet<PathBuf>>::new();
    for split in 1..components.len() {
        let mut archive_relative = components[..split.saturating_sub(1)]
            .iter()
            .collect::<PathBuf>();
        let mut archive_name = components[split - 1].clone();
        archive_name.push(".zip");
        archive_relative.push(archive_name);
        let suffix = components[split..].iter().collect::<PathBuf>();
        for archive in resolve_matches(roots, &archive_relative) {
            let entries = candidates.entry(archive).or_default();
            entries.insert(relative.to_path_buf());
            entries.insert(suffix.clone());
        }
    }
    candidates
}

fn extract_matching_archive(
    archive_path: &Path,
    candidates: &BTreeSet<PathBuf>,
) -> Result<Option<(ExtractedArchive, PathBuf)>, String> {
    let metadata = std::fs::metadata(archive_path)
        .map_err(|error| format!("could not inspect package ({error})"))?;
    if metadata.len() > MAX_ARCHIVE_BYTES {
        return Err(format!(
            "package exceeds the {MAX_ARCHIVE_BYTES}-byte compressed-size limit"
        ));
    }
    let file = std::fs::File::open(archive_path)
        .map_err(|error| format!("could not open package ({error})"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| format!("invalid ZIP package ({error})"))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(format!(
            "package exceeds the {MAX_ARCHIVE_ENTRIES}-entry limit"
        ));
    }

    let candidate_keys = candidates
        .iter()
        .filter_map(|path| portable_key(path))
        .collect::<BTreeSet<_>>();
    let mut paths = Vec::with_capacity(archive.len());
    let mut keys = BTreeSet::new();
    let mut total = 0u64;
    let mut matched = None;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("could not inspect ZIP entry {index} ({error})"))?;
        if entry.encrypted() {
            return Err("encrypted ZIP entries are unsupported".to_string());
        }
        if entry.is_symlink() {
            return Err("symbolic links are forbidden in EDI packages".to_string());
        }
        let path = bounded_relative_path(entry.name())
            .ok_or_else(|| format!("ZIP entry `{}` has an unsafe path", entry.name()))?;
        let key = portable_key(&path)
            .ok_or_else(|| format!("ZIP entry `{}` has a non-portable path", entry.name()))?;
        if !keys.insert(key.clone()) {
            return Err(format!("ZIP entry `{}` is duplicated", entry.name()));
        }
        if !entry.is_dir() {
            total = total
                .checked_add(entry.size())
                .ok_or_else(|| "package expanded-size overflow".to_string())?;
            if total > MAX_EXTRACTED_BYTES {
                return Err(format!(
                    "package exceeds the {MAX_EXTRACTED_BYTES}-byte expanded-size limit"
                ));
            }
        }
        if candidate_keys.contains(&key) && matched.replace(path.clone()).is_some() {
            return Err("package contains multiple matching configurations".to_string());
        }
        paths.push(path);
    }
    let Some(matched) = matched else {
        return Ok(None);
    };

    let root = create_temp_directory()?;
    let extracted = ExtractedArchive { root };
    let mut written = 0u64;
    for (index, relative) in paths.into_iter().enumerate() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("could not read ZIP entry {index} ({error})"))?;
        let output = extracted.root.join(&relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&output)
                .map_err(|error| format!("could not create package directory ({error})"))?;
            continue;
        }
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("could not create package directory ({error})"))?;
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)
            .map_err(|error| format!("could not create extracted package file ({error})"))?;
        let remaining = MAX_EXTRACTED_BYTES.saturating_sub(written);
        let copied = std::io::copy(&mut entry.by_ref().take(remaining + 1), &mut file)
            .map_err(|error| format!("could not extract package file ({error})"))?;
        written = written
            .checked_add(copied)
            .ok_or_else(|| "package expanded-size overflow".to_string())?;
        if written > MAX_EXTRACTED_BYTES {
            return Err(format!(
                "package exceeds the {MAX_EXTRACTED_BYTES}-byte expanded-size limit"
            ));
        }
        file.flush()
            .map_err(|error| format!("could not finish extracted package file ({error})"))?;
    }
    let config = extracted.root.join(matched);
    if !config.is_file() {
        return Err("matched configuration was not extracted as a file".to_string());
    }
    Ok(Some((extracted, config)))
}

fn create_temp_directory() -> Result<PathBuf, String> {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    for _ in 0..32 {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("ferrule-mfd-edi-{}-{id}", std::process::id()));
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("could not create extraction directory ({error})")),
        }
    }
    Err("could not allocate a unique extraction directory".to_string())
}

fn bounded_relative_path(text: &str) -> Option<PathBuf> {
    let portable = text.replace('\\', "/");
    let path = Path::new(&portable);
    if portable.is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    let normalized = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value),
            Component::CurDir => None,
            _ => None,
        })
        .collect::<PathBuf>();
    (!normalized.as_os_str().is_empty()).then_some(normalized)
}

fn portable_key(path: &Path) -> Option<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            Component::CurDir => None,
            _ => None,
        })
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join("/")
        .into()
}

fn resolve_case_insensitive(base: &Path, relative: &Path) -> Option<PathBuf> {
    let mut current = base.to_path_buf();
    for component in relative.components() {
        let Component::Normal(expected) = component else {
            continue;
        };
        let direct = current.join(expected);
        if direct.exists() {
            current = direct;
            continue;
        }
        let expected = expected.to_str()?;
        let mut matches = std::fs::read_dir(&current)
            .ok()?
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case(expected))
            })
            .map(|entry| entry.path());
        let found = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        current = found;
    }
    current.is_file().then_some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_paths_reject_escape_and_normalize_windows_separators() {
        assert_eq!(
            bounded_relative_path(r"Custom.X12\Envelope.Config"),
            Some(PathBuf::from("Custom.X12/Envelope.Config"))
        );
        assert!(bounded_relative_path("../Envelope.Config").is_none());
        assert!(bounded_relative_path("/Envelope.Config").is_none());
    }
}
