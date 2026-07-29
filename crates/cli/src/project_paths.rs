//! Portable rebasing for project-owned static instance paths.

use std::path::{Component, Path, PathBuf};

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
    rebase_optional(&mut project.source_path, &from, &to)?;
    rebase_optional(&mut project.target_path, &from, &to)?;
    for source in &mut project.extra_sources {
        if source.dynamic_path.is_none() {
            rebase_required(&mut source.path, &from, &to)?;
        }
    }
    for target in &mut project.extra_targets {
        rebase_optional(&mut target.path, &from, &to)?;
    }
    Ok(())
}

fn rebase_optional(path: &mut Option<String>, from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(path) = path {
        rebase_required(path, from, to)?;
    }
    Ok(())
}

fn rebase_required(path: &mut String, from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(rebased) = rebase_path(path, from, to)? {
        *path = rebased;
    }
    Ok(())
}

fn rebase_path(stored: &str, from: &Path, to: &Path) -> std::io::Result<Option<String>> {
    if stored.trim().is_empty() || stored.contains("://") {
        return Ok(None);
    }
    let portable = stored.replace('\\', "/");
    if looks_like_windows_absolute(&portable) {
        return Ok(None);
    }
    let relative = Path::new(&portable);
    if relative.is_absolute() {
        return Ok(None);
    }
    let resolved = normalize_absolute(&from.join(relative)).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("cannot resolve stored project path `{stored}`"),
        )
    })?;
    let relative = relative_path(to, &resolved).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "cannot rebase stored project path `{stored}` from `{}` to `{}`",
                from.display(),
                to.display()
            ),
        )
    })?;
    path_to_portable(&relative).map(Some)
}

fn absolute_parent(document: &Path) -> std::io::Result<PathBuf> {
    let parent = document
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let absolute = if parent.is_absolute() {
        parent.to_path_buf()
    } else {
        std::env::current_dir()?.join(parent)
    };
    normalize_absolute(&absolute).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("project directory `{}` is not absolute", absolute.display()),
        )
    })
}

fn normalize_absolute(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
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

    #[test]
    fn rebases_relative_paths_and_preserves_wildcards() {
        let from = Path::new("/package/maps");
        let to = Path::new("/package/projects/nested");

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
    fn leaves_urls_and_absolute_paths_unchanged() {
        let from = Path::new("/package/maps");
        let to = Path::new("/package/projects");
        for path in [
            "https://example.test/orders.json",
            "/var/data/orders.json",
            r"C:\Data\orders.json",
        ] {
            assert_eq!(rebase_path(path, from, to).unwrap(), None);
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
                    dynamic_path: Some(DynamicSourcePath {
                        node: 0,
                        iteration: Vec::new(),
                    }),
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
