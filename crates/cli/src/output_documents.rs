use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, anyhow, bail};
use ir::{DocumentMember, Instance, SchemaNode};
use mapping::FormatOptions;

use super::{WrittenOutput, write_output};

pub(super) enum OutputDestination {
    Static(PathBuf),
    DynamicBase(PathBuf),
}

pub(super) struct TargetOutput<'a> {
    pub destination: &'a OutputDestination,
    pub name: &'a str,
    pub schema: &'a SchemaNode,
    pub instance: &'a Instance,
    pub options: &'a FormatOptions,
    pub current_datetime: &'a str,
    pub additional: bool,
}

pub(super) struct TargetWriteResult {
    pub records_written: usize,
    pub outputs: Vec<WrittenOutput>,
}

pub(super) fn write_target_outputs(
    targets: &[TargetOutput<'_>],
) -> anyhow::Result<Vec<TargetWriteResult>> {
    let planned = targets
        .iter()
        .map(PlannedTarget::build)
        .collect::<anyhow::Result<Vec<_>>>()?;
    validate_global_paths(&planned)?;
    preflight_planned_targets(&planned)?;

    let mut staged = Vec::with_capacity(planned.len());
    for target in &planned {
        match StagedTarget::render(target) {
            Ok(rendered) => staged.push(rendered),
            Err(error) => {
                cleanup_stages(&staged);
                return Err(if target.additional {
                    error.context(format!("writing extra target `{}`", target.name))
                } else {
                    error
                });
            }
        }
    }

    if let Err(failure) = publish_staged_targets(&staged) {
        return Err(failure.into_error(&stage_paths(&staged)));
    }
    cleanup_stages(&staged);

    Ok(staged.into_iter().map(StagedTarget::into_result).collect())
}

struct PlannedTarget<'a> {
    name: &'a str,
    schema: &'a SchemaNode,
    options: &'a FormatOptions,
    current_datetime: &'a str,
    additional: bool,
    stage_base: PathBuf,
    dynamic: Option<(PathBuf, Vec<PathBuf>)>,
    files: Vec<PlannedFile<'a>>,
}

struct PlannedFile<'a> {
    final_path: PathBuf,
    stage_path: PathBuf,
    instance: &'a Instance,
}

impl<'a> PlannedTarget<'a> {
    fn build(target: &'a TargetOutput<'a>) -> anyhow::Result<Self> {
        match (target.destination, target.instance) {
            (OutputDestination::Static(_), Instance::DocumentSet(_)) => {
                bail!("mapping produced dynamically named documents for a static output path")
            }
            (OutputDestination::DynamicBase(_), value)
                if !matches!(value, Instance::DocumentSet(_)) =>
            {
                bail!("dynamic target mapping did not produce a document set")
            }
            (OutputDestination::Static(path), instance) => {
                let stage_base = output_parent(path)?;
                let stage_path = path.file_name().map(PathBuf::from).with_context(|| {
                    format!("static output path {} does not name a file", path.display())
                })?;
                Ok(Self {
                    name: target.name,
                    schema: target.schema,
                    options: target.options,
                    current_datetime: target.current_datetime,
                    additional: target.additional,
                    stage_base,
                    dynamic: None,
                    files: vec![PlannedFile {
                        final_path: path.clone(),
                        stage_path,
                        instance,
                    }],
                })
            }
            (OutputDestination::DynamicBase(base), Instance::DocumentSet(documents)) => {
                let relative_paths = validate_document_paths(documents)?;
                let files = documents
                    .iter()
                    .zip(&relative_paths)
                    .map(|(document, relative)| PlannedFile {
                        final_path: base.join(relative),
                        stage_path: relative.clone(),
                        instance: document.value(),
                    })
                    .collect();
                Ok(Self {
                    name: target.name,
                    schema: target.schema,
                    options: target.options,
                    current_datetime: target.current_datetime,
                    additional: target.additional,
                    stage_base: base.clone(),
                    dynamic: Some((base.clone(), relative_paths)),
                    files,
                })
            }
            (OutputDestination::DynamicBase(_), _) => unreachable!("guarded above"),
        }
    }
}

fn output_parent(path: &Path) -> anyhow::Result<PathBuf> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    Ok(parent.to_path_buf())
}

fn validate_global_paths(targets: &[PlannedTarget<'_>]) -> anyhow::Result<()> {
    let mut paths = Vec::new();
    for target in targets {
        for file in &target.files {
            paths.push((
                normalized_absolute(&file.final_path)?,
                target.name,
                &file.final_path,
            ));
        }
    }
    for (index, (path, name, display_path)) in paths.iter().enumerate() {
        for (other, other_name, other_display_path) in paths.iter().skip(index + 1) {
            if path == other {
                bail!(
                    "output targets `{name}` and `{other_name}` resolve to the same path `{}`",
                    display_path.display()
                );
            }
            if path.starts_with(other) || other.starts_with(path) {
                bail!(
                    "output target paths `{}` and `{}` overlap as file and directory",
                    display_path.display(),
                    other_display_path.display()
                );
            }
        }
    }
    Ok(())
}

fn normalized_absolute(path: &Path) -> anyhow::Result<PathBuf> {
    let absolute = std::path::absolute(path)
        .with_context(|| format!("resolving output path {}", path.display()))?;
    let absolute = lexical_normalize(&absolute);
    let mut existing = absolute.as_path();
    let mut suffix = Vec::new();
    loop {
        match std::fs::symlink_metadata(existing) {
            Ok(_) => {
                let mut resolved = std::fs::canonicalize(existing).with_context(|| {
                    format!("resolving existing output ancestor {}", existing.display())
                })?;
                for segment in suffix.iter().rev() {
                    resolved.push(segment);
                }
                return Ok(lexical_normalize(&resolved));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let segment = existing.file_name().with_context(|| {
                    format!(
                        "output path {} has no existing filesystem ancestor",
                        absolute.display()
                    )
                })?;
                suffix.push(segment.to_os_string());
                existing = existing.parent().with_context(|| {
                    format!(
                        "output path {} has no existing filesystem ancestor",
                        absolute.display()
                    )
                })?;
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("checking output path {}", existing.display()));
            }
        }
    }
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn preflight_planned_targets(targets: &[PlannedTarget<'_>]) -> anyhow::Result<()> {
    for target in targets {
        prepare_stage_base(target)?;
        for file in &target.files {
            match std::fs::symlink_metadata(&file.final_path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    bail!(
                        "output destination {} cannot be a symlink",
                        file.final_path.display()
                    )
                }
                Ok(metadata) if metadata.is_dir() => {
                    bail!(
                        "output destination {} is a directory",
                        file.final_path.display()
                    )
                }
                Ok(metadata) if metadata.is_file() => {}
                Ok(_) => {
                    bail!(
                        "output destination {} is not a regular file",
                        file.final_path.display()
                    )
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("checking output destination {}", file.final_path.display())
                    });
                }
            }
        }
    }
    Ok(())
}

struct StagedTarget<'a> {
    name: &'a str,
    stage: PathBuf,
    dynamic: Option<(PathBuf, Vec<PathBuf>)>,
    files: Vec<StagedFile>,
}

struct StagedFile {
    final_path: PathBuf,
    staged_path: PathBuf,
    records_written: usize,
}

impl<'a> StagedTarget<'a> {
    fn render(target: &'a PlannedTarget<'a>) -> anyhow::Result<Self> {
        prepare_stage_base(target)?;
        let stage = create_stage_directory(&target.stage_base)?;
        let render_result = target
            .files
            .iter()
            .map(|file| {
                let staged_path = stage.join("new").join(&file.stage_path);
                let parent = staged_path.parent().with_context(|| {
                    format!("staged output {} has no parent", staged_path.display())
                })?;
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("creating staged output directory {}", parent.display())
                })?;
                copy_existing_output(&file.final_path, &staged_path)?;
                let records_written = write_output(
                    &staged_path,
                    target.schema,
                    file.instance,
                    target.options,
                    target.current_datetime,
                )?;
                Ok(StagedFile {
                    final_path: file.final_path.clone(),
                    staged_path,
                    records_written,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>();
        match render_result {
            Ok(files) => Ok(Self {
                name: target.name,
                stage,
                dynamic: target.dynamic.clone(),
                files,
            }),
            Err(error) => {
                let _ = std::fs::remove_dir_all(&stage);
                Err(error)
            }
        }
    }

    fn into_result(self) -> TargetWriteResult {
        let records_written = self.files.iter().map(|file| file.records_written).sum();
        let outputs = self
            .files
            .into_iter()
            .map(|file| WrittenOutput {
                name: self.name.to_string(),
                records_written: file.records_written,
                path: file.final_path,
            })
            .collect();
        TargetWriteResult {
            records_written,
            outputs,
        }
    }
}

fn prepare_stage_base(target: &PlannedTarget<'_>) -> anyhow::Result<()> {
    std::fs::create_dir_all(&target.stage_base)
        .with_context(|| format!("creating output directory {}", target.stage_base.display()))?;
    let metadata = std::fs::symlink_metadata(&target.stage_base)
        .with_context(|| format!("reading output directory {}", target.stage_base.display()))?;
    if metadata.file_type().is_symlink() && target.dynamic.is_some() {
        bail!(
            "dynamic output base {} cannot be a symlink",
            target.stage_base.display()
        );
    }
    if !metadata.is_dir() {
        bail!(
            "output base {} is not a directory",
            target.stage_base.display()
        );
    }
    if let Some((base, relative_paths)) = &target.dynamic {
        reject_symlinked_output_components(base, relative_paths)?;
    }
    Ok(())
}

fn copy_existing_output(final_path: &Path, staged_path: &Path) -> anyhow::Result<()> {
    match std::fs::symlink_metadata(final_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "output destination {} cannot be a symlink",
                final_path.display()
            )
        }
        Ok(metadata) if metadata.is_dir() => {
            bail!("output destination {} is a directory", final_path.display())
        }
        Ok(metadata) if metadata.is_file() => {
            std::fs::copy(final_path, staged_path).with_context(|| {
                format!(
                    "copying existing output {} into staging",
                    final_path.display()
                )
            })?;
            Ok(())
        }
        Ok(_) => bail!(
            "output destination {} is not a regular file",
            final_path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("checking existing output {}", final_path.display()))
        }
    }
}

fn stage_paths(targets: &[StagedTarget<'_>]) -> Vec<PathBuf> {
    targets.iter().map(|target| target.stage.clone()).collect()
}

fn cleanup_stages(targets: &[StagedTarget<'_>]) {
    for target in targets {
        let _ = std::fs::remove_dir_all(&target.stage);
    }
}

pub(super) fn validate_document_paths(
    documents: &[DocumentMember],
) -> anyhow::Result<Vec<PathBuf>> {
    let mut paths = Vec::with_capacity(documents.len());
    let mut unique = BTreeSet::new();
    for document in documents {
        let portable = document.path().replace('\\', "/");
        if portable.is_empty() {
            bail!("dynamic output path cannot be empty");
        }
        if portable.starts_with('/') {
            bail!("dynamic output path must be relative");
        }
        if has_windows_drive_prefix(&portable) {
            bail!("dynamic output path must not use a Windows drive prefix");
        }
        let mut normalized = PathBuf::new();
        for segment in portable.split('/') {
            match segment {
                "" => bail!("dynamic output path cannot contain an empty segment"),
                "." => bail!("dynamic output path cannot contain `.` segments"),
                ".." => bail!("dynamic output path cannot contain `..` segments"),
                _ => normalized.push(segment),
            }
        }
        if normalized.as_os_str().is_empty() {
            bail!("dynamic output path cannot be empty");
        }
        if normalized
            .components()
            .next()
            .and_then(|component| component.as_os_str().to_str())
            .is_some_and(|segment| segment.starts_with(".ferrule-stage-"))
        {
            bail!("dynamic output path uses a reserved staging name");
        }
        if !unique.insert(normalized.clone()) {
            bail!("duplicate dynamic output path `{}`", normalized.display());
        }
        paths.push(normalized);
    }
    for (index, path) in paths.iter().enumerate() {
        for other in paths.iter().skip(index + 1) {
            if path.starts_with(other) || other.starts_with(path) {
                bail!(
                    "dynamic output paths `{}` and `{}` overlap as file and directory",
                    path.display(),
                    other.display()
                );
            }
        }
    }
    Ok(paths)
}

fn has_windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn reject_symlinked_output_components(base: &Path, paths: &[PathBuf]) -> anyhow::Result<()> {
    for relative in paths {
        let mut current = base.to_path_buf();
        for component in relative.components() {
            current.push(component.as_os_str());
            match std::fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    bail!("dynamic output path crosses symlink {}", current.display())
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("checking dynamic output path {}", current.display())
                    });
                }
            }
        }
    }
    Ok(())
}

static STAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn create_stage_directory(base: &Path) -> anyhow::Result<PathBuf> {
    for _ in 0..64 {
        let id = STAGE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let stage = base.join(format!(".ferrule-stage-{}-{id}", std::process::id()));
        match std::fs::create_dir(&stage) {
            Ok(()) => return Ok(stage),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("creating output staging directory {}", stage.display())
                });
            }
        }
    }
    bail!("could not allocate a unique dynamic output staging directory")
}

fn publish_staged_targets(targets: &[StagedTarget<'_>]) -> Result<(), PublishFailure> {
    preflight_publication(targets).map_err(PublishFailure::new)?;

    for target in targets {
        let backup_dir = target.stage.join("backup");
        std::fs::create_dir(&backup_dir)
            .with_context(|| format!("creating output backup directory {}", backup_dir.display()))
            .map_err(PublishFailure::new)?;
    }

    let mut backups = Vec::new();
    for target in targets {
        for (index, file) in target.files.iter().enumerate() {
            recheck_dynamic_path(target, &file.final_path)
                .map_err(|error| PublishFailure::with_rollback(error, restore_backups(&backups)))?;
            let backup = target.stage.join("backup").join(index.to_string());
            let moved = match std::fs::symlink_metadata(&file.final_path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    let error = anyhow!(
                        "output destination {} became a symlink before publication",
                        file.final_path.display()
                    );
                    return Err(PublishFailure::with_rollback(
                        error,
                        restore_backups(&backups),
                    ));
                }
                Ok(metadata) if metadata.is_dir() => {
                    let error = anyhow!(
                        "output destination {} became a directory before publication",
                        file.final_path.display()
                    );
                    return Err(PublishFailure::with_rollback(
                        error,
                        restore_backups(&backups),
                    ));
                }
                Ok(metadata) if metadata.is_file() => {
                    if let Err(error) = std::fs::rename(&file.final_path, &backup) {
                        let error = anyhow::Error::new(error).context(format!(
                            "backing up existing output {}",
                            file.final_path.display()
                        ));
                        return Err(PublishFailure::with_rollback(
                            error,
                            restore_backups(&backups),
                        ));
                    }
                    true
                }
                Ok(_) => {
                    let error = anyhow!(
                        "output destination {} is not a regular file",
                        file.final_path.display()
                    );
                    return Err(PublishFailure::with_rollback(
                        error,
                        restore_backups(&backups),
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => {
                    let error = anyhow::Error::new(error)
                        .context(format!("checking output {}", file.final_path.display()));
                    return Err(PublishFailure::with_rollback(
                        error,
                        restore_backups(&backups),
                    ));
                }
            };
            backups.push((file.final_path.clone(), backup, moved));
        }
    }

    let mut published = Vec::new();
    for target in targets {
        for file in &target.files {
            let readiness = recheck_dynamic_path(target, &file.final_path)
                .and_then(|()| require_destination_absent(&file.final_path));
            if let Err(error) = readiness {
                return Err(PublishFailure::with_rollback(
                    error,
                    rollback_publication(&published, &backups),
                ));
            }
            if let Err(error) = std::fs::rename(&file.staged_path, &file.final_path) {
                let error = anyhow::Error::new(error)
                    .context(format!("publishing output {}", file.final_path.display()));
                return Err(PublishFailure::with_rollback(
                    error,
                    rollback_publication(&published, &backups),
                ));
            }
            published.push(file.final_path.clone());
        }
    }
    Ok(())
}

fn preflight_publication(targets: &[StagedTarget<'_>]) -> anyhow::Result<()> {
    for target in targets {
        if let Some((base, relative_paths)) = &target.dynamic {
            reject_symlinked_output_components(base, relative_paths)?;
        }
        for file in &target.files {
            let parent = file.final_path.parent().with_context(|| {
                format!(
                    "output destination {} has no parent",
                    file.final_path.display()
                )
            })?;
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating output directory {}", parent.display()))?;
            match std::fs::symlink_metadata(&file.final_path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    bail!(
                        "output destination {} cannot be a symlink",
                        file.final_path.display()
                    )
                }
                Ok(metadata) if metadata.is_dir() => {
                    bail!(
                        "output destination {} is a directory",
                        file.final_path.display()
                    )
                }
                Ok(metadata) if metadata.is_file() => {}
                Ok(_) => {
                    bail!(
                        "output destination {} is not a regular file",
                        file.final_path.display()
                    )
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("checking output destination {}", file.final_path.display())
                    });
                }
            }
        }
        if let Some((base, relative_paths)) = &target.dynamic {
            reject_symlinked_output_components(base, relative_paths)?;
        }
    }
    Ok(())
}

fn recheck_dynamic_path(target: &StagedTarget<'_>, final_path: &Path) -> anyhow::Result<()> {
    let Some((base, _)) = &target.dynamic else {
        return Ok(());
    };
    let relative = final_path.strip_prefix(base).with_context(|| {
        format!(
            "dynamic output {} escaped base {}",
            final_path.display(),
            base.display()
        )
    })?;
    reject_symlinked_output_components(base, &[relative.to_path_buf()])
}

fn require_destination_absent(path: &Path) -> anyhow::Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => bail!(
            "output destination {} changed during publication",
            path.display()
        ),
        Err(error) => {
            Err(error).with_context(|| format!("rechecking output destination {}", path.display()))
        }
    }
}

fn rollback_publication(
    published: &[PathBuf],
    backups: &[(PathBuf, PathBuf, bool)],
) -> Vec<anyhow::Error> {
    let mut errors = Vec::new();
    for path in published.iter().rev() {
        if let Err(error) = std::fs::remove_file(path) {
            errors.push(anyhow!(
                "removing partially published output {} during rollback: {error}",
                path.display()
            ));
        }
    }
    errors.extend(restore_backups(backups));
    errors
}

fn restore_backups(backups: &[(PathBuf, PathBuf, bool)]) -> Vec<anyhow::Error> {
    let mut errors = Vec::new();
    for (final_path, backup, moved) in backups.iter().rev() {
        if *moved && let Err(error) = std::fs::rename(backup, final_path) {
            errors.push(anyhow!(
                "restoring backup {} to {}: {error}",
                backup.display(),
                final_path.display()
            ));
        }
    }
    errors
}

struct PublishFailure {
    error: anyhow::Error,
    rollback_errors: Vec<anyhow::Error>,
}

impl PublishFailure {
    fn new(error: anyhow::Error) -> Self {
        Self {
            error,
            rollback_errors: Vec::new(),
        }
    }

    fn with_rollback(error: anyhow::Error, rollback_errors: Vec<anyhow::Error>) -> Self {
        Self {
            error,
            rollback_errors,
        }
    }

    fn into_error(self, stages: &[PathBuf]) -> anyhow::Error {
        if self.rollback_errors.is_empty() {
            cleanup_stage_paths(stages);
            return self.error;
        }
        let recovery = self
            .rollback_errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        let stages = stages
            .iter()
            .map(|stage| stage.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        self.error.context(format!(
            "output rollback was incomplete; recovery files were retained at {stages}: {recovery}"
        ))
    }
}

fn cleanup_stage_paths(stages: &[PathBuf]) {
    for stage in stages {
        let _ = std::fs::remove_dir_all(stage);
    }
}

#[cfg(test)]
mod tests {
    use super::{PublishFailure, restore_backups, rollback_publication};

    fn test_root(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "ferrule-cli-{label}-{}-{}",
            std::process::id(),
            super::STAGE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ))
    }

    #[test]
    fn incomplete_rollback_preserves_recovery_stage_and_reports_paths() -> anyhow::Result<()> {
        let root = test_root("rollback");
        let stage = root.join("stage");
        let backup = stage.join("backup/0");
        let destination = root.join("destination");
        let backup_parent = backup
            .parent()
            .ok_or_else(|| anyhow::anyhow!("backup has no parent"))?;
        std::fs::create_dir_all(backup_parent)?;
        std::fs::write(&backup, "original")?;
        std::fs::create_dir(&destination)?;

        let rollback_errors = restore_backups(&[(destination.clone(), backup.clone(), true)]);
        assert_eq!(rollback_errors.len(), 1);
        let error =
            PublishFailure::with_rollback(anyhow::anyhow!("publication failed"), rollback_errors)
                .into_error(std::slice::from_ref(&stage));
        let message = format!("{error:#}");

        assert!(stage.exists());
        assert!(backup.exists());
        assert!(message.contains("recovery files were retained"));
        assert!(message.contains(&stage.display().to_string()));
        assert!(message.contains(&backup.display().to_string()));
        assert!(message.contains(&destination.display().to_string()));

        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn batch_rollback_restores_primary_and_late_target_backups() -> anyhow::Result<()> {
        let root = test_root("batch-rollback");
        let primary = root.join("primary.xml");
        let extra = root.join("extra.xml");
        let primary_backup = root.join("stages/primary/backup/0");
        let extra_backup = root.join("stages/extra/backup/0");
        for path in [&primary, &extra, &primary_backup, &extra_backup] {
            let parent = path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("test path has no parent"))?;
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&primary, "published primary")?;
        std::fs::write(&extra, "published extra")?;
        std::fs::write(&primary_backup, "original primary")?;
        std::fs::write(&extra_backup, "original extra")?;

        let errors = rollback_publication(
            &[primary.clone(), extra.clone()],
            &[
                (primary.clone(), primary_backup, true),
                (extra.clone(), extra_backup, true),
            ],
        );

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(std::fs::read_to_string(&primary)?, "original primary");
        assert_eq!(std::fs::read_to_string(&extra)?, "original extra");
        std::fs::remove_dir_all(root)?;
        Ok(())
    }
}
