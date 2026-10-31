//! Portable rebasing for project-owned static instance paths.

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf, Prefix};

/// Rebases every static local instance path when a project document moves.
///
/// Dynamic paths and HTTP(S) URLs are expressions/identities rather than
/// document-relative files and are left unchanged.
pub fn rebase(
    project: &mut mapping::Project,
    from_document: &Path,
    to_document: &Path,
) -> std::io::Result<()> {
    let from = absolute_parent(from_document)?;
    let to = absolute_parent(to_document)?;
    rebase_optional(&mut project.source_path, &from, &to, PathSyntax::Glob)?;
    rebase_optional(&mut project.target_path, &from, &to, PathSyntax::Literal)?;
    for source in &mut project.extra_sources {
        if source.dynamic_path.is_none() {
            rebase_required(&mut source.path, &from, &to, PathSyntax::Glob)?;
        }
    }
    for target in &mut project.extra_targets {
        rebase_optional(&mut target.path, &from, &to, PathSyntax::Literal)?;
    }
    Ok(())
}

/// Whether moving a project document between these paths changes the meaning
/// of its relative instance paths, i.e. whether [`rebase`] resolves them
/// against a different directory.
pub fn relocates(from_document: &Path, to_document: &Path) -> std::io::Result<bool> {
    Ok(absolute_parent(from_document)? != absolute_parent(to_document)?)
}

/// Stores an imported mapping's active-file identity relative to a saved
/// pipeline, so moving the two files together preserves runtime path values.
pub fn mapping_identity_relative_to(
    mapping_path: &Path,
    pipeline_path: &Path,
) -> std::io::Result<String> {
    let pipeline_dir = document_parent(pipeline_path)?;
    let mapping_path = if mapping_path.is_absolute() {
        mapping_path.to_path_buf()
    } else {
        std::env::current_dir()?.join(mapping_path)
    };
    require_absolute_spelling(&pipeline_dir)?;
    require_absolute_spelling(&mapping_path)?;
    // Directory links can cross volume roots. Compare the resolved directories,
    // not their original drive-letter or UNC spellings.
    let pipeline_dir = resolve_directory(&pipeline_dir)?;
    let mapping_path = resolve_resource_path(&mapping_path)?;
    let relative = relative_path(&pipeline_dir, &mapping_path).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "cannot make mapping path `{}` relative to `{}`",
                mapping_path.display(),
                pipeline_dir.display()
            ),
        )
    })?;
    path_to_portable(&relative)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PathSyntax {
    Literal,
    Glob,
}

fn rebase_optional(
    path: &mut Option<String>,
    from: &Path,
    to: &Path,
    syntax: PathSyntax,
) -> std::io::Result<()> {
    if let Some(path) = path {
        rebase_required(path, from, to, syntax)?;
    }
    Ok(())
}

fn rebase_required(
    path: &mut String,
    from: &Path,
    to: &Path,
    syntax: PathSyntax,
) -> std::io::Result<()> {
    if let Some(rebased) = rebase_instance_path(path, from, to, syntax)? {
        *path = rebased;
    }
    Ok(())
}

fn relative_local_path(stored: &str) -> Option<PathBuf> {
    if stored.trim().is_empty() || stored.contains("://") {
        return None;
    }
    let portable = stored.replace('\\', "/");
    if looks_like_windows_absolute(&portable) {
        return None;
    }
    let relative = Path::new(&portable);
    if relative.is_absolute() {
        return None;
    }
    Some(relative.to_path_buf())
}

/// Lexical rebasing for virtual, already-confined bundle paths. This must not
/// inspect unrelated host filesystem entries that share a virtual pathname.
pub(super) fn rebase_path(stored: &str, from: &Path, to: &Path) -> std::io::Result<Option<String>> {
    let Some(relative) = relative_local_path(stored) else {
        return Ok(None);
    };
    let resolved = normalize_absolute(&from.join(relative)).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("cannot resolve stored project path `{stored}`"),
        )
    })?;
    rebased_relative(stored, from, to, &resolved).map(Some)
}

fn rebase_instance_path(
    stored: &str,
    from: &Path,
    to: &Path,
    syntax: PathSyntax,
) -> std::io::Result<Option<String>> {
    let Some(relative) = relative_local_path(stored) else {
        return Ok(None);
    };
    let from = resolve_directory(from)?;
    let resolved = match (relative.parent(), relative.file_name()) {
        (Some(parent), Some(name)) => {
            resolve_components(from.clone(), parent.components(), syntax)?.join(name)
        }
        _ => resolve_components(from.clone(), relative.components(), syntax)?,
    };
    let to = resolve_directory(to)?;
    rebased_relative(stored, &from, &to, &resolved).map(Some)
}

fn rebased_relative(
    stored: &str,
    from: &Path,
    to: &Path,
    resolved: &Path,
) -> std::io::Result<String> {
    let relative = relative_path(to, resolved).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "cannot rebase stored project path `{stored}` from `{}` to `{}`",
                from.display(),
                to.display()
            ),
        )
    })?;
    path_to_portable(&relative)
}

fn absolute_parent(document: &Path) -> std::io::Result<PathBuf> {
    resolve_directory(&document_parent(document)?)
}

fn document_parent(document: &Path) -> std::io::Result<PathBuf> {
    let parent = document
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    Ok(if parent.is_absolute() {
        parent.to_path_buf()
    } else {
        std::env::current_dir()?.join(parent)
    })
}

/// Resolve directory links but retain the resource entry itself. In particular,
/// a linked output file must not become its referent: publication may replace
/// that entry, and a nonexistent output or wildcard is still a valid resource.
fn resolve_resource_path(path: &Path) -> std::io::Result<PathBuf> {
    require_absolute_spelling(path)?;
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => Ok(resolve_directory(parent)?.join(name)),
        _ => resolve_directory(path),
    }
}

fn require_absolute_spelling(path: &Path) -> std::io::Result<()> {
    if path.is_absolute() && verbatim_has_ordinary_spelling(path) {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "project path `{}` has no ordinary absolute spelling",
                path.display()
            ),
        ))
    }
}

/// Existing directory ancestors must be resolved before a following `..`.
/// Unknown directories keep lexical behavior, but the suffix after a glob is
/// opaque: removing `..` there could change which paths the pattern selects.
fn resolve_directory(path: &Path) -> std::io::Result<PathBuf> {
    require_absolute_spelling(path)?;
    resolve_components(PathBuf::new(), path.components(), PathSyntax::Literal)
}

fn resolve_components<'a>(
    mut resolved: PathBuf,
    components: impl Iterator<Item = Component<'a>>,
    syntax: PathSyntax,
) -> std::io::Result<PathBuf> {
    let mut pattern = false;
    for component in components {
        match component {
            Component::Prefix(prefix) => resolved.push(ordinary_prefix(prefix.kind())),
            Component::RootDir => resolved.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir if !pattern => {
                resolved.pop();
            }
            Component::ParentDir => resolved.push(".."),
            Component::Normal(name) => {
                resolved.push(name);
                pattern |= syntax == PathSyntax::Glob
                    && name
                        .as_encoded_bytes()
                        .iter()
                        .any(|&byte| matches!(byte, b'*' | b'?'));
                if pattern {
                    continue;
                }
                match std::fs::symlink_metadata(&resolved) {
                    Ok(_) => {
                        let canonical = std::fs::canonicalize(&resolved)?;
                        if !std::fs::metadata(&canonical)?.is_dir() {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::NotADirectory,
                                format!(
                                    "project path ancestor `{}` is not a directory",
                                    resolved.display()
                                ),
                            ));
                        }
                        resolved = normalize_absolute(&canonical).ok_or_else(|| {
                            std::io::Error::new(
                                std::io::ErrorKind::InvalidInput,
                                format!(
                                    "project directory `{}` has no ordinary spelling",
                                    canonical.display()
                                ),
                            )
                        })?;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
        }
    }
    Ok(resolved)
}

fn normalize_absolute(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() || !verbatim_has_ordinary_spelling(path) {
        return None;
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(ordinary_prefix(prefix.kind())),
            Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
        }
    }
    Some(normalized)
}

/// Removing a verbatim prefix also enables ordinary Windows normalization.
/// Refuse names whose identity would change, rather than store a relative path
/// to a different file. Ordinary paths keep their existing interpretation.
fn verbatim_has_ordinary_spelling(path: &Path) -> bool {
    let mut components = path.components();
    match components.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::VerbatimDisk(_) => {}
            Prefix::VerbatimUNC(server, share) => {
                if !ordinary_component_spelling(server) || !ordinary_component_spelling(share) {
                    return false;
                }
            }
            _ => return true,
        },
        _ => return true,
    }
    components.all(|component| match component {
        Component::RootDir => true,
        Component::Normal(name) => ordinary_component_spelling(name) && !dos_device_name(name),
        Component::CurDir | Component::ParentDir | Component::Prefix(_) => false,
    })
}

fn ordinary_component_spelling(name: &OsStr) -> bool {
    // The encoded representation preserves native non-Unicode names; ASCII
    // punctuation can be checked without a lossy conversion or allocation.
    let bytes = name.as_encoded_bytes();
    !bytes.is_empty()
        && !bytes.ends_with(b".")
        && !bytes.ends_with(b" ")
        && !bytes.iter().any(|&byte| {
            byte < 32
                || matches!(
                    byte,
                    b'<' | b'>' | b':' | b'"' | b'/' | b'\\' | b'|' | b'?' | b'*'
                )
        })
}

pub(super) fn dos_device_name(name: &OsStr) -> bool {
    let stem = name
        .as_encoded_bytes()
        .split(|&byte| byte == b'.')
        .next()
        .unwrap_or_default();
    let stem = stem.trim_ascii_end();
    if [
        b"CON".as_slice(),
        b"PRN",
        b"AUX",
        b"NUL",
        b"CONIN$",
        b"CONOUT$",
    ]
    .iter()
    .any(|device| stem.eq_ignore_ascii_case(device))
    {
        return true;
    }
    let numbered = stem.get(..3).is_some_and(|prefix| {
        prefix.eq_ignore_ascii_case(b"COM") || prefix.eq_ignore_ascii_case(b"LPT")
    });
    numbered && matches!(&stem[3..], [b'1'..=b'9'] | [0xc2, 0xb9 | 0xb2 | 0xb3])
}

/// Spells a verbatim disk or UNC prefix in its ordinary form.
///
/// Canonical Windows paths are verbatim (`\\?\C:\...`) while user-supplied
/// paths usually are not. After checking component equivalence, relative paths
/// can be computed between equal prefixes for the same volume.
fn ordinary_prefix(prefix: Prefix<'_>) -> OsString {
    match prefix {
        Prefix::VerbatimDisk(drive) | Prefix::Disk(drive) => {
            format!("{}:", char::from(drive.to_ascii_uppercase())).into()
        }
        Prefix::VerbatimUNC(server, share) | Prefix::UNC(server, share) => {
            let mut ordinary = OsString::from(r"\\");
            ordinary.push(server);
            ordinary.push(r"\");
            ordinary.push(share);
            ordinary
        }
        Prefix::Verbatim(name) => {
            let mut verbatim = OsString::from(r"\\?\");
            verbatim.push(name);
            verbatim
        }
        Prefix::DeviceNS(name) => {
            let mut device = OsString::from(r"\\.\");
            device.push(name);
            device
        }
    }
}

fn relative_path(from: &Path, target: &Path) -> Option<PathBuf> {
    let from = from.components().collect::<Vec<_>>();
    let target = target.components().collect::<Vec<_>>();
    let common = from
        .iter()
        .zip(&target)
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0 {
        return None;
    }
    let mut result = PathBuf::new();
    for component in &from[common..] {
        if matches!(component, Component::Normal(_)) {
            result.push("..");
        }
    }
    for component in &target[common..] {
        result.push(component.as_os_str());
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    Some(result)
}

fn path_to_portable(path: &Path) -> std::io::Result<String> {
    path.components()
        .map(|component| {
            component.as_os_str().to_str().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "rebased project path `{}` is not valid Unicode",
                        path.display()
                    ),
                )
            })
        })
        .collect::<std::io::Result<Vec<_>>>()
        .map(|components| components.join("/"))
}

fn looks_like_windows_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        || path.starts_with("//")
}

#[cfg(test)]
mod tests {
    use ir::SchemaNode;
    use mapping::{
        DynamicSourcePath, FormatOptions, Graph, NamedSource, NamedTarget, Project, Scope,
    };

    use super::*;

    #[cfg(unix)]
    struct Scratch(PathBuf);

    #[cfg(unix)]
    impl Scratch {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "ferrule-save-as-links-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    #[cfg(unix)]
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    fn filesystem_path(stored: &str, from: &Path, to: &Path) -> std::io::Result<Option<String>> {
        rebase_instance_path(stored, from, to, PathSyntax::Glob)
    }

    #[cfg(unix)]
    #[test]
    fn save_as_through_directory_symlink_keeps_resource_identity() {
        let scratch = Scratch::new();
        let original = scratch.0.join("maps[archive]");
        let physical = scratch.0.join("physical/deep");
        let wrong = scratch.0.join("physical/maps");
        for directory in [&original, &physical, &wrong] {
            std::fs::create_dir_all(directory).unwrap();
        }
        for resource in ["source.xml", "target.xml", "catalog.xml", "audit.xml"] {
            std::fs::write(original.join(resource), b"original").unwrap();
            std::fs::write(wrong.join(resource), b"wrong").unwrap();
        }
        for name in [
            "linked",
            "linked[archive]",
            "linked*archive",
            "linked?archive",
        ] {
            let linked = scratch.0.join(name);
            std::os::unix::fs::symlink(&physical, &linked).unwrap();
            for resource in ["source.xml", "target.xml", "catalog.xml", "audit.xml"] {
                let rebased = filesystem_path(
                    resource,
                    &original,
                    &absolute_parent(&linked.join("project.json")).unwrap(),
                )
                .unwrap()
                .unwrap();
                assert_eq!(std::fs::read(linked.join(rebased)).unwrap(), b"original");
            }
            assert!(
                !relocates(&physical.join("project.json"), &linked.join("renamed.json")).unwrap()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn stored_symlink_dotdot_is_resolved_before_rebasing() {
        let scratch = Scratch::new();
        let original = scratch.0.join("maps");
        let physical = scratch.0.join("physical/deep");
        let destination = scratch.0.join("saved");
        for directory in [&original, &physical, &destination] {
            std::fs::create_dir_all(directory).unwrap();
        }
        std::os::unix::fs::symlink(&physical, original.join("route")).unwrap();
        std::fs::write(scratch.0.join("physical/source.xml"), b"physical parent").unwrap();
        std::fs::write(original.join("source.xml"), b"lexical parent").unwrap();
        let rebased = filesystem_path("route/../source.xml", &original, &destination)
            .unwrap()
            .unwrap();
        assert_eq!(
            std::fs::read(destination.join(rebased)).unwrap(),
            b"physical parent"
        );
    }

    #[cfg(unix)]
    #[test]
    fn linked_directories_preserve_missing_outputs_and_glob_suffixes() {
        let scratch = Scratch::new();
        let original = scratch.0.join("maps");
        let physical = scratch.0.join("physical/deep");
        let destination = scratch.0.join("saved");
        for directory in [&original, &physical, &destination] {
            std::fs::create_dir_all(directory).unwrap();
        }
        std::os::unix::fs::symlink(&physical, original.join("route")).unwrap();
        for (stored, expected) in [
            (
                "route/../missing/output.json",
                "../physical/missing/output.json",
            ),
            ("route/../*.xml", "../physical/*.xml"),
            ("route/*/../*.xml", "../physical/deep/*/../*.xml"),
        ] {
            assert_eq!(
                filesystem_path(stored, &original, &destination)
                    .unwrap()
                    .as_deref(),
                Some(expected)
            );
        }
        assert!(!scratch.0.join("physical/missing").exists());
    }

    #[cfg(unix)]
    #[test]
    fn rebasing_does_not_follow_resource_file_links() {
        let scratch = Scratch::new();
        let original = scratch.0.join("maps");
        let destination = scratch.0.join("saved");
        for directory in [&original, &destination] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let target = scratch.0.join("real-output.xml");
        std::fs::write(&target, b"target").unwrap();
        std::os::unix::fs::symlink(&target, original.join("output.xml")).unwrap();
        let rebased =
            rebase_instance_path("output.xml", &original, &destination, PathSyntax::Literal)
                .unwrap()
                .unwrap();
        assert_eq!(rebased, "../maps/output.xml");
        assert!(
            std::fs::symlink_metadata(destination.join(&rebased))
                .unwrap()
                .is_symlink()
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"target");
    }

    #[cfg(unix)]
    #[test]
    fn rebasing_rejects_unresolvable_existing_directory_entries() {
        let scratch = Scratch::new();
        let original = scratch.0.join("maps");
        let destination = scratch.0.join("saved");
        for directory in [&original, &destination] {
            std::fs::create_dir_all(directory).unwrap();
        }
        std::os::unix::fs::symlink(scratch.0.join("missing"), original.join("broken")).unwrap();
        std::fs::write(original.join("file"), b"not a directory").unwrap();
        for stored in ["broken/../source.xml", "file/../source.xml"] {
            assert!(
                filesystem_path(stored, &original, &destination).is_err(),
                "{stored}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn literal_output_directories_and_virtual_paths_have_distinct_rules() {
        let scratch = Scratch::new();
        let original = scratch.0.join("maps");
        let destination = scratch.0.join("saved");
        let physical = scratch.0.join("physical/deep");
        for directory in [&original, &destination, &physical] {
            std::fs::create_dir_all(directory).unwrap();
        }
        std::os::unix::fs::symlink(&physical, original.join("output[archive]")).unwrap();
        let path = "output[archive]/../target.xml";
        assert_eq!(
            rebase_instance_path(path, &original, &destination, PathSyntax::Literal)
                .unwrap()
                .as_deref(),
            Some("../physical/target.xml")
        );
        assert_eq!(
            filesystem_path(path, &original, &destination)
                .unwrap()
                .as_deref(),
            Some("../physical/target.xml"),
            "brackets are literal in Ferrule's source wildcard syntax"
        );
        assert_eq!(
            rebase_path(path, &original, &destination)
                .unwrap()
                .as_deref(),
            Some("../maps/target.xml")
        );
    }

    #[cfg(unix)]
    #[test]
    fn mapping_identity_uses_the_actual_pipeline_directory() {
        let scratch = Scratch::new();
        let original = scratch.0.join("maps");
        let physical = scratch.0.join("physical/deep");
        for directory in [&original, &physical] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let mapping = original.join("design.mfd");
        std::fs::write(&mapping, b"mapping").unwrap();
        let linked = scratch.0.join("linked[archive]");
        std::os::unix::fs::symlink(&physical, &linked).unwrap();
        let relative =
            mapping_identity_relative_to(&mapping, &linked.join("pipeline.json")).unwrap();
        assert_eq!(std::fs::read(linked.join(relative)).unwrap(), b"mapping");
    }

    /// Spells a rooted test path as an absolute path on the current platform.
    fn absolute(path: &str) -> String {
        if cfg!(windows) {
            format!("C:{path}")
        } else {
            path.to_string()
        }
    }

    #[test]
    fn rebases_relative_paths_and_preserves_wildcards() {
        let from = absolute("/package/maps");
        let to = absolute("/package/projects/nested");
        let (from, to) = (Path::new(&from), Path::new(&to));

        assert_eq!(
            rebase_path("../data/orders.json", from, to)
                .unwrap()
                .as_deref(),
            Some("../../data/orders.json")
        );
        assert_eq!(
            rebase_path(r"..\data\*.xml", from, to).unwrap().as_deref(),
            Some("../../data/*.xml")
        );
    }

    #[test]
    fn relocation_compares_normalized_document_directories() {
        let maps = absolute("/package/maps/project.json");
        let renamed = absolute("/package/maps/./nested/../renamed.json");
        let moved = absolute("/package/projects/project.json");
        let (maps, renamed, moved) = (Path::new(&maps), Path::new(&renamed), Path::new(&moved));

        assert!(!relocates(maps, renamed).unwrap());
        assert!(relocates(maps, moved).unwrap());
    }

    #[test]
    fn leaves_urls_and_absolute_paths_unchanged() {
        let from = absolute("/package/maps");
        let to = absolute("/package/projects");
        for path in [
            "https://example.test/orders.json",
            &absolute("/var/data/orders.json"),
            r"C:\Data\orders.json",
        ] {
            assert_eq!(
                rebase_path(path, Path::new(&from), Path::new(&to)).unwrap(),
                None
            );
        }
    }

    #[test]
    fn rebases_every_static_project_boundary_but_not_dynamic_sources() {
        let schema = SchemaNode::group("root", Vec::new());
        let mut project = Project {
            source: schema.clone(),
            target: schema.clone(),
            source_path: Some("../data/input.json".into()),
            target_path: Some("output.json".into()),
            source_options: FormatOptions::default(),
            target_options: FormatOptions::default(),
            extra_sources: vec![
                NamedSource {
                    name: "catalog".into(),
                    path: "catalog.json".into(),
                    schema: schema.clone(),
                    options: FormatOptions::default(),
                    dynamic_path: None,
                },
                NamedSource {
                    name: "computed".into(),
                    path: "fallback.json".into(),
                    schema: schema.clone(),
                    options: FormatOptions::default(),
                    dynamic_path: Some(DynamicSourcePath::legacy(0, Vec::new())),
                },
            ],
            extra_targets: vec![NamedTarget {
                name: "audit".into(),
                path: Some("audit.json".into()),
                schema,
                options: FormatOptions::default(),
                root: Scope::default(),
            }],
            failure_rules: Vec::new(),
            user_functions: Default::default(),
            graph: Graph::default(),
            root: Scope::default(),
        };

        rebase(
            &mut project,
            Path::new("/package/maps/design.json"),
            Path::new("/package/projects/project.json"),
        )
        .unwrap();

        assert_eq!(project.source_path.as_deref(), Some("../data/input.json"));
        assert_eq!(project.target_path.as_deref(), Some("../maps/output.json"));
        assert_eq!(project.extra_sources[0].path, "../maps/catalog.json");
        assert_eq!(project.extra_sources[1].path, "fallback.json");
        assert_eq!(
            project.extra_targets[0].path.as_deref(),
            Some("../maps/audit.json")
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_unicode_rebased_components() {
        use std::os::unix::ffi::OsStringExt as _;

        let path = PathBuf::from(std::ffi::OsString::from_vec(vec![b'f', 0xff]));
        let error = path_to_portable(&path).expect_err("non-Unicode path must not be corrupted");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[cfg(windows)]
    #[test]
    fn rebases_between_verbatim_and_ordinary_prefixes() {
        assert_eq!(
            rebase_path(
                "data/orders.json",
                Path::new(r"\\?\C:\package\maps"),
                Path::new(r"C:\package\projects"),
            )
            .unwrap()
            .as_deref(),
            Some("../maps/data/orders.json")
        );
        assert_eq!(
            rebase_path(
                "orders.json",
                Path::new(r"\\?\UNC\server\share\maps"),
                Path::new(r"\\server\share\projects"),
            )
            .unwrap()
            .as_deref(),
            Some("../maps/orders.json")
        );
        assert_eq!(
            mapping_identity_relative_to(
                Path::new(r"\\?\c:\package\maps\design.mfd"),
                Path::new(r"C:\package\pipelines\pipeline.json"),
            )
            .unwrap(),
            "../maps/design.mfd"
        );
    }

    #[cfg(windows)]
    #[test]
    fn refuses_verbatim_file_names_without_an_ordinary_identity() {
        let pipeline = Path::new(r"C:\package\pipelines\pipeline.json");
        for name in [
            "hidden.",
            "hidden ",
            ".",
            "..",
            "CON",
            "prn.txt",
            "AUX",
            "NUL.dat",
            "COM1",
            "LPT9.log",
            "COM¹",
            "lpt².txt",
            "COM³",
            "CONIN$",
            "conout$.txt",
            "NUL .txt",
            "folder/child",
            "stream:part",
            "bad?name",
        ] {
            for root in [r"\\?\C:\package", r"\\?\UNC\server\share\package"] {
                let mapping = PathBuf::from(format!(r"{root}\{name}\design.mfd"));
                let error = mapping_identity_relative_to(&mapping, pipeline)
                    .expect_err("verbatim-only names must not acquire an ordinary identity");
                assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
                assert!(normalize_absolute(&mapping).is_none());
            }
        }
        let mapping = Path::new(r"\\?\C:\package\maps\design.mfd.");
        assert!(mapping_identity_relative_to(mapping, pipeline).is_err());
        let pipeline = Path::new(r"\\?\C:\package\hidden.\pipeline.json");
        assert!(
            mapping_identity_relative_to(Path::new(r"C:\package\design.mfd"), pipeline).is_err()
        );
    }

    #[cfg(windows)]
    #[test]
    fn refuses_verbatim_unc_prefix_names_that_change_spelling() {
        for path in [
            r"\\?\UNC\server.\share\maps\design.mfd",
            r"\\?\UNC\server\share \maps\design.mfd",
            r"\\?\UNC\server\..\maps\design.mfd",
            r"\\?\UNC\server/name\share\maps\design.mfd",
        ] {
            assert!(normalize_absolute(Path::new(path)).is_none());
        }
    }

    #[cfg(windows)]
    #[test]
    fn keeps_safe_reverse_prefixes_wildcards_and_volume_boundaries() {
        for (from, to) in [
            (r"C:\package\maps", r"\\?\c:\package\projects"),
            (
                r"\\server\share\package\maps",
                r"\\?\UNC\server\share\package\projects",
            ),
        ] {
            let from = normalize_absolute(Path::new(from)).unwrap();
            let to = normalize_absolute(Path::new(to)).unwrap();
            assert_eq!(
                rebase_path(r"data\*.xml", &from, &to).unwrap().as_deref(),
                Some("../maps/data/*.xml")
            );
        }
        for path in [
            r"\\?\C:\package\.config\COM10\design.mfd",
            r"\\?\C:\package\COM0\LPT10\console.txt",
        ] {
            assert!(normalize_absolute(Path::new(path)).is_some());
        }
        for (mapping, pipeline) in [
            (r"\\?\C:\package\design.mfd", r"D:\package\pipeline.json"),
            (
                r"\\?\UNC\server\share\design.mfd",
                r"\\server\other\pipeline.json",
            ),
            (
                r"\\?\UNC\server\share\design.mfd",
                r"\\other\share\pipeline.json",
            ),
        ] {
            // This is a lexical prefix test, not an attempt to contact a
            // fabricated UNC server. Real directory aliases are resolved by
            // mapping_identity_relative_to before comparing their roots.
            let error = rebase_path(
                "design.mfd",
                Path::new(mapping).parent().unwrap(),
                Path::new(pipeline).parent().unwrap(),
            )
            .expect_err("different volumes must remain unrepresentable");
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        }
        // The new guard applies only when removing a verbatim prefix.
        assert!(normalize_absolute(Path::new(r"C:\package\hidden.\NUL.txt")).is_some());
    }

    #[cfg(windows)]
    #[test]
    fn preserves_native_non_unicode_unc_prefixes_without_lossy_conversion() {
        use std::os::windows::ffi::OsStringExt as _;

        let mut verbatim = OsString::from(r"\\?\UNC\server");
        verbatim.push(OsString::from_wide(&[0xd800]));
        verbatim.push(r"\share\maps");
        let mut ordinary = OsString::from(r"\\server");
        ordinary.push(OsString::from_wide(&[0xd800]));
        ordinary.push(r"\share\maps");
        assert_eq!(
            normalize_absolute(Path::new(&verbatim)).unwrap(),
            PathBuf::from(ordinary)
        );
    }

    #[cfg(windows)]
    #[test]
    fn rejects_cross_volume_relative_rebase() {
        let error = rebase_path(
            "data/orders.json",
            Path::new(r"C:\package\maps"),
            Path::new(r"D:\package\projects"),
        )
        .expect_err("cross-volume relative path cannot be represented");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }
}
