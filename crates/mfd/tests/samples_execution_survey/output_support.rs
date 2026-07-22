use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use ir::{Instance, SchemaKind, SchemaNode};
use mapping::{FormatOptions, Project};

use super::format_io::{
    extension, output_path, portable_path, read_instance, resolve_sample_input_from, write_instance,
};
use super::reference_support::{
    first_instance_difference, host_path_instances_equal, instances_semantically_equal,
};

pub(super) struct WrittenSurveyOutputs {
    pub(super) primary: PathBuf,
    pub(super) extras: Vec<PathBuf>,
}

pub(super) fn compare_execution_outputs(
    expected: &engine::ExecutionOutputs,
    actual: &engine::ExecutionOutputs,
) -> Result<(), String> {
    if !instances_semantically_equal(&expected.primary, &actual.primary) {
        return Err(format!(
            "primary output changed: {}",
            first_instance_difference(&expected.primary, &actual.primary)
        ));
    }
    let expected = named_output_map(&expected.extras)?;
    let actual = named_output_map(&actual.extras)?;
    let expected_names = expected.keys().copied().collect::<BTreeSet<_>>();
    let actual_names = actual.keys().copied().collect::<BTreeSet<_>>();
    if expected_names != actual_names {
        return Err(format!(
            "named output set changed: expected {expected_names:?}, produced {actual_names:?}"
        ));
    }
    for (name, expected) in expected {
        let Some(actual) = actual.get(name) else {
            return Err(format!("named output `{name}` disappeared"));
        };
        if !instances_semantically_equal(expected, actual) {
            return Err(format!(
                "named output `{name}` changed: {}",
                first_instance_difference(expected, actual)
            ));
        }
    }
    Ok(())
}

fn named_output_map(outputs: &[engine::NamedOutput]) -> Result<BTreeMap<&str, &Instance>, String> {
    let mut by_name = BTreeMap::new();
    for output in outputs {
        if by_name
            .insert(output.name.as_str(), &output.instance)
            .is_some()
        {
            return Err(format!(
                "engine produced duplicate named output `{}`",
                output.name
            ));
        }
    }
    Ok(by_name)
}

pub(super) fn write_outputs(
    project: &Project,
    outputs: &engine::ExecutionOutputs,
    sample_dir: &Path,
    samples_root: &Path,
    design_base: &Path,
) -> Result<WrittenSurveyOutputs, String> {
    let primary_path = write_target_output(
        &project.root,
        "primary-output",
        project.target_path.as_deref(),
        &project.target,
        &outputs.primary,
        &project.target_options,
        sample_dir,
        samples_root,
        design_base,
    )?;
    if outputs.extras.len() != project.extra_targets.len() {
        return Err("engine returned an unexpected number of additional targets".to_string());
    }
    let mut extras = Vec::with_capacity(outputs.extras.len());
    for (index, (target, output)) in project
        .extra_targets
        .iter()
        .zip(&outputs.extras)
        .enumerate()
    {
        let path = write_target_output(
            &target.root,
            &format!("extra-output-{index}"),
            target.path.as_deref(),
            &target.schema,
            &output.instance,
            &target.options,
            sample_dir,
            samples_root,
            design_base,
        )
        .map_err(|error| format!("writing extra target `{}` failed: {error}", target.name))?;
        extras.push(path);
    }
    Ok(WrittenSurveyOutputs {
        primary: primary_path,
        extras,
    })
}

#[allow(clippy::too_many_arguments)]
fn write_target_output(
    scope: &mapping::Scope,
    label: &str,
    stored: Option<&str>,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    sample_dir: &Path,
    samples_root: &Path,
    design_base: &Path,
) -> Result<PathBuf, String> {
    if scope.output_path().is_some() {
        let documents = instance.as_document_set().ok_or_else(|| {
            format!("{label} has dynamic paths but did not produce a document set")
        })?;
        return write_document_set(sample_dir, label, documents, schema, options);
    }
    if matches!(instance, Instance::DocumentSet(_)) {
        return Err(format!(
            "{label} produced dynamically named documents without a dynamic target"
        ));
    }
    let path = output_path(sample_dir, stored, options, label)?;
    prepare_database_output(samples_root, design_base, stored, &path, schema)?;
    prepare_xlsx_update_output(
        samples_root,
        design_base,
        sample_dir,
        stored,
        &path,
        options,
    )?;
    write_instance(&path, schema, instance, options)
        .map_err(|error| format!("writing {label} failed: {error}"))?;
    Ok(path)
}

fn write_document_set(
    base: &Path,
    label: &str,
    documents: &[ir::DocumentMember],
    schema: &SchemaNode,
    options: &FormatOptions,
) -> Result<PathBuf, String> {
    let relative_paths = validate_document_paths(documents)?;
    let stage = base.join(format!(".{label}-documents"));
    std::fs::create_dir(&stage)
        .map_err(|error| format!("creating dynamic output stage failed: {error}"))?;
    let render_result =
        documents
            .iter()
            .zip(&relative_paths)
            .try_for_each(|(document, relative)| {
                let staged = stage.join(relative);
                if let Some(parent) = staged.parent() {
                    std::fs::create_dir_all(parent).map_err(|error| {
                        format!("creating staged output directory failed: {error}")
                    })?;
                }
                write_instance(&staged, schema, document.value(), options)
                    .map_err(|error| format!("rendering dynamic output failed: {error}"))
            });
    if let Err(error) = render_result {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(error);
    }
    for relative in &relative_paths {
        let destination = base.join(relative);
        if destination.exists() {
            let _ = std::fs::remove_dir_all(&stage);
            return Err(format!(
                "dynamic output destination already exists: {}",
                destination.display()
            ));
        }
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("creating dynamic output directory failed: {error}"))?;
        }
        std::fs::rename(stage.join(relative), &destination)
            .map_err(|error| format!("publishing dynamic output failed: {error}"))?;
    }
    let _ = std::fs::remove_dir_all(&stage);
    Ok(relative_paths
        .first()
        .map_or_else(|| base.to_path_buf(), |relative| base.join(relative)))
}

pub(super) fn validate_document_paths(
    documents: &[ir::DocumentMember],
) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::with_capacity(documents.len());
    let mut unique = BTreeSet::new();
    for document in documents {
        let portable = document.path().replace('\\', "/");
        let path = Path::new(&portable);
        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                std::path::Component::Normal(segment) if !segment.is_empty() => {
                    normalized.push(segment)
                }
                _ => return Err(format!("unsafe dynamic output path `{}`", document.path())),
            }
        }
        if normalized.as_os_str().is_empty() {
            return Err("dynamic output path cannot be empty".into());
        }
        if !unique.insert(normalized.clone()) {
            return Err(format!(
                "duplicate dynamic output path `{}`",
                normalized.display()
            ));
        }
        paths.push(normalized);
    }
    for (index, path) in paths.iter().enumerate() {
        for other in paths.iter().skip(index + 1) {
            if path.starts_with(other) || other.starts_with(path) {
                return Err(format!(
                    "dynamic output paths `{}` and `{}` overlap",
                    path.display(),
                    other.display()
                ));
            }
        }
    }
    Ok(paths)
}

pub(super) fn prepare_database_output(
    samples_root: &Path,
    design_base: &Path,
    stored: Option<&str>,
    output: &Path,
    schema: &SchemaNode,
) -> Result<(), String> {
    let Ok(extension) = extension(output) else {
        return Ok(());
    };
    if !matches!(extension.as_str(), "db" | "sqlite" | "sqlite3") {
        return Ok(());
    }
    let relational = matches!(
        &schema.kind,
        SchemaKind::Group { children, .. }
            if children
                .iter()
                .any(|child| matches!(child.kind, SchemaKind::Group { .. }))
    );
    let Some(stored) = stored else {
        return if relational {
            Err("relational SQLite output has no stored database template".to_string())
        } else {
            Ok(())
        };
    };
    match resolve_sample_input_from(samples_root, design_base, stored) {
        Ok(template) => std::fs::copy(&template, output)
            .map(|_| ())
            .map_err(|error| {
                format!(
                    "copying SQLite output template `{}` failed: {error}",
                    template.display()
                )
            }),
        Err(reason) if relational => Err(format!(
            "relational SQLite output requires its stored database template: {reason}"
        )),
        Err(_) => Ok(()),
    }
}

pub(super) fn prepare_xlsx_update_output(
    samples_root: &Path,
    design_base: &Path,
    writable_root: &Path,
    stored: Option<&str>,
    output: &Path,
    options: &FormatOptions,
) -> Result<(), String> {
    if !options.xlsx_update_existing {
        return Ok(());
    }
    let canonical_samples = std::fs::canonicalize(samples_root)
        .map_err(|error| format!("resolving sample root failed: {error}"))?;
    let canonical_writable = std::fs::canonicalize(writable_root)
        .map_err(|error| format!("resolving writable output root failed: {error}"))?;
    if canonical_writable.starts_with(&canonical_samples) {
        return Err("XLSX update output root must be outside the read-only sample tree".into());
    }
    let stored = stored
        .ok_or_else(|| "update-in-place XLSX output has no stored workbook template".to_string())?;
    let template = resolve_sample_input_from(samples_root, design_base, stored)?;
    if extension(&template)? != "xlsx" {
        return Err(format!(
            "update-in-place XLSX template `{}` is not an .xlsx workbook",
            template.display()
        ));
    }
    atomic_copy_into_writable_root(&template, writable_root, output)
}

fn atomic_copy_into_writable_root(
    source: &Path,
    writable_root: &Path,
    output: &Path,
) -> Result<(), String> {
    let writable_root = std::fs::canonicalize(writable_root)
        .map_err(|error| format!("resolving writable output root failed: {error}"))?;
    let output_parent = output
        .parent()
        .ok_or_else(|| format!("output `{}` has no parent directory", output.display()))?;
    let output_parent = std::fs::canonicalize(output_parent)
        .map_err(|error| format!("resolving output parent failed: {error}"))?;
    if !output_parent.starts_with(&writable_root) {
        return Err(format!(
            "output `{}` escapes the writable execution-survey directory",
            output.display()
        ));
    }
    let file_name = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("output `{}` has no usable filename", output.display()))?;
    for attempt in 0..1_024 {
        let temporary = output_parent.join(format!(".{file_name}.ferrule-copy-{attempt}"));
        let mut destination = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "creating temporary workbook copy `{}` failed: {error}",
                    temporary.display()
                ));
            }
        };
        let copy_result = (|| {
            let mut source_file = std::fs::File::open(source).map_err(|error| {
                format!(
                    "opening workbook template `{}` failed: {error}",
                    source.display()
                )
            })?;
            io::copy(&mut source_file, &mut destination).map_err(|error| {
                format!(
                    "copying workbook template `{}` failed: {error}",
                    source.display()
                )
            })?;
            destination.sync_all().map_err(|error| {
                format!(
                    "syncing workbook copy `{}` failed: {error}",
                    temporary.display()
                )
            })?;
            drop(destination);
            std::fs::rename(&temporary, output).map_err(|error| {
                format!(
                    "installing workbook copy at `{}` failed: {error}",
                    output.display()
                )
            })
        })();
        if copy_result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        return copy_result;
    }
    Err("could not allocate a temporary workbook-copy filename".to_string())
}

struct ComparableTarget<'a> {
    label: String,
    names: BTreeSet<String>,
    schema: &'a SchemaNode,
    options: &'a FormatOptions,
    written: &'a Path,
}

pub(super) fn compare_generated_references(
    project: &Project,
    written: &WrittenSurveyOutputs,
    references: &[PathBuf],
) -> Result<(), String> {
    let host_path_output = project
        .graph
        .nodes
        .values()
        .any(|node| matches!(node, mapping::Node::SourceDocumentPath));
    let mut targets = Vec::with_capacity(1 + project.extra_targets.len());
    targets.push(ComparableTarget {
        label: "primary target".into(),
        names: target_reference_names(&project.target.name, project.target_path.as_deref()),
        schema: &project.target,
        options: &project.target_options,
        written: &written.primary,
    });
    for (target, written) in project.extra_targets.iter().zip(&written.extras) {
        let mut names = target_reference_names(&target.name, target.path.as_deref());
        names.insert(normalized_reference_name(&target.schema.name));
        targets.push(ComparableTarget {
            label: format!("extra target `{}`", target.name),
            names,
            schema: &target.schema,
            options: &target.options,
            written,
        });
    }
    if targets.len() != references.len() {
        return Err(format!(
            "reference manifest contains {} outputs, but ferrule produced {} targets",
            references.len(),
            targets.len()
        ));
    }

    let reference_names = references
        .iter()
        .map(|path| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(normalized_reference_name)
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    let mut unmatched = (0..references.len()).collect::<BTreeSet<_>>();
    let mut assignments = vec![None; targets.len()];
    for (target_index, target) in targets.iter().enumerate() {
        let matches = unmatched
            .iter()
            .copied()
            .filter(|index| target.names.contains(&reference_names[*index]))
            .collect::<Vec<_>>();
        if matches.len() == 1 {
            assignments[target_index] = Some(matches[0]);
            unmatched.remove(&matches[0]);
        }
    }
    let unresolved = assignments
        .iter()
        .enumerate()
        .filter_map(|(index, assignment)| assignment.is_none().then_some(index))
        .collect::<Vec<_>>();
    if unresolved.len() == 1 && unmatched.len() == 1 {
        assignments[unresolved[0]] = unmatched.pop_first();
    }
    assign_unique_schema_matches(&targets, references, &mut assignments, &mut unmatched);
    if assignments.iter().any(Option::is_none) {
        return Err("generated reference outputs cannot be matched uniquely to targets".into());
    }

    for (target, reference_index) in targets.iter().zip(assignments) {
        let Some(reference_index) = reference_index else {
            return Err("generated reference assignment disappeared".into());
        };
        let reference = &references[reference_index];
        let expected =
            read_instance(reference, target.schema, target.options).map_err(|error| {
                format!(
                    "reading reference for {} `{}` failed: {error}",
                    target.label,
                    reference.display()
                )
            })?;
        let actual =
            read_instance(target.written, target.schema, target.options).map_err(|error| {
                format!(
                    "reading ferrule output for {} `{}` failed: {error}",
                    target.label,
                    target.written.display()
                )
            })?;
        if !(instances_semantically_equal(&expected, &actual)
            || host_path_output && host_path_instances_equal(&expected, &actual))
        {
            return Err(format!(
                "{} differs from reference `{}`: {}",
                target.label,
                reference.display(),
                first_instance_difference(&expected, &actual)
            ));
        }
    }
    Ok(())
}

fn assign_unique_schema_matches(
    targets: &[ComparableTarget<'_>],
    references: &[PathBuf],
    assignments: &mut [Option<usize>],
    unmatched: &mut BTreeSet<usize>,
) {
    loop {
        let mut progress = false;
        for (target_index, target) in targets.iter().enumerate() {
            if assignments[target_index].is_some() {
                continue;
            }
            let candidates = unmatched
                .iter()
                .copied()
                .filter(|reference_index| {
                    read_instance(&references[*reference_index], target.schema, target.options)
                        .is_ok()
                })
                .collect::<Vec<_>>();
            if let [reference_index] = candidates.as_slice() {
                assignments[target_index] = Some(*reference_index);
                unmatched.remove(reference_index);
                progress = true;
            }
        }
        if !progress {
            break;
        }
    }
}

fn target_reference_names(name: &str, stored: Option<&str>) -> BTreeSet<String> {
    let mut names = BTreeSet::from([normalized_reference_name(name)]);
    if let Some(stem) = stored
        .and_then(|value| portable_path(value).file_stem().map(|stem| stem.to_owned()))
        .and_then(|stem| stem.to_str().map(str::to_string))
    {
        names.insert(normalized_reference_name(&stem));
    }
    names
}

fn normalized_reference_name(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use engine::{ExecutionOutputs, NamedOutput};
    use ir::{Instance, ScalarType, SchemaNode, Value};
    use mapping::{FormatOptions, Graph, NamedTarget, Project, Scope};

    use super::{WrittenSurveyOutputs, compare_execution_outputs, compare_generated_references};

    #[test]
    fn compares_named_execution_outputs_by_identity_and_reports_drift() {
        let scalar = |value: &str| Instance::Scalar(Value::String(value.into()));
        let expected = ExecutionOutputs {
            primary: scalar("primary"),
            extras: vec![
                NamedOutput {
                    name: "alpha".into(),
                    instance: scalar("a"),
                },
                NamedOutput {
                    name: "beta".into(),
                    instance: scalar("b"),
                },
            ],
        };
        let reordered = ExecutionOutputs {
            primary: scalar("primary"),
            extras: vec![
                NamedOutput {
                    name: "beta".into(),
                    instance: scalar("b"),
                },
                NamedOutput {
                    name: "alpha".into(),
                    instance: scalar("a"),
                },
            ],
        };
        assert!(compare_execution_outputs(&expected, &reordered).is_ok());

        let drifted = ExecutionOutputs {
            primary: scalar("primary"),
            extras: vec![
                NamedOutput {
                    name: "alpha".into(),
                    instance: scalar("changed"),
                },
                NamedOutput {
                    name: "beta".into(),
                    instance: scalar("b"),
                },
            ],
        };
        let error = compare_execution_outputs(&expected, &drifted).unwrap_err();
        assert!(error.contains("named output `alpha` changed"), "{error}");
    }

    #[test]
    fn matches_unnamed_multi_outputs_by_their_document_schema() {
        let workspace = super::super::SurveyWorkspace::new().unwrap();
        let primary_schema = SchemaNode::group(
            "Purchases",
            vec![SchemaNode::scalar("Code", ScalarType::String)],
        );
        let extra_schema = SchemaNode::group(
            "Requests",
            vec![SchemaNode::scalar("Number", ScalarType::Int)],
        );
        let primary_instance = Instance::Group(vec![(
            "Code".into(),
            Instance::Scalar(Value::String("A".into())),
        )]);
        let extra_instance =
            Instance::Group(vec![("Number".into(), Instance::Scalar(Value::Int(7)))]);
        let actual_primary = workspace.0.join("actual-primary.xml");
        let actual_extra = workspace.0.join("actual-extra.xml");
        let reference_primary = workspace.0.join("ipos.xml");
        let reference_extra = workspace.0.join("rfqs.xml");
        format_xml::write(&actual_primary, &primary_schema, &primary_instance).unwrap();
        format_xml::write(&actual_extra, &extra_schema, &extra_instance).unwrap();
        format_xml::write(&reference_primary, &primary_schema, &primary_instance).unwrap();
        format_xml::write(&reference_extra, &extra_schema, &extra_instance).unwrap();

        let project = Project {
            source: SchemaNode::group("Source", Vec::new()),
            target: primary_schema,
            source_path: None,
            target_path: None,
            source_options: FormatOptions::default(),
            target_options: FormatOptions::default(),
            extra_sources: Vec::new(),
            extra_targets: vec![NamedTarget {
                name: "RFQ Nanonull Inc".into(),
                path: None,
                schema: extra_schema,
                options: FormatOptions::default(),
                root: Scope::default(),
            }],
            failure_rules: Vec::new(),
            graph: Graph::default(),
            root: Scope::default(),
        };
        let written = WrittenSurveyOutputs {
            primary: actual_primary,
            extras: vec![actual_extra],
        };

        compare_generated_references(&project, &written, &[reference_primary, reference_extra])
            .unwrap();
    }
}
