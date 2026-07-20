use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};

use ir::Instance;

const REFERENCE_MANIFEST_ENV: &str = "FERRULE_REFERENCE_OUTPUT_MANIFEST";

#[derive(Debug)]
pub(super) struct GeneratedReferences {
    outputs: BTreeMap<PathBuf, Vec<PathBuf>>,
}

impl GeneratedReferences {
    pub(super) fn for_sample(
        &self,
        mfd_path: &Path,
        samples_root: &Path,
    ) -> Result<Option<&[PathBuf]>, String> {
        let relative = mfd_path.strip_prefix(samples_root).map_err(|_| {
            format!(
                "sample `{}` is outside `{}`",
                mfd_path.display(),
                samples_root.display()
            )
        })?;
        if let Some(reference) = self.outputs.get(relative) {
            return Ok(Some(reference));
        }
        let mut matches = self
            .outputs
            .iter()
            .filter(|(declared, _)| declared.ends_with(relative));
        let reference = matches.next().map(|(_, references)| references.as_slice());
        if matches.next().is_some() {
            return Err(format!(
                "reference manifest contains multiple entries ending in `{}`",
                relative.display()
            ));
        }
        Ok(reference)
    }
}

pub(super) fn requested_generated_references() -> Result<Option<GeneratedReferences>, Box<dyn Error>>
{
    let Some(paths) = std::env::var_os(REFERENCE_MANIFEST_ENV) else {
        return Ok(None);
    };
    let paths = std::env::split_paths(&paths).collect::<Vec<_>>();
    if paths.is_empty() {
        return Err(
            format!("{REFERENCE_MANIFEST_ENV} must name one or more manifest files").into(),
        );
    }
    let mut combined = GeneratedReferences {
        outputs: BTreeMap::new(),
    };
    for path in paths {
        for (sample, output) in load_generated_references(&path)?.outputs {
            if combined.outputs.insert(sample.clone(), output).is_some() {
                return Err(
                    format!("reference manifests repeat sample `{}`", sample.display()).into(),
                );
            }
        }
    }
    Ok(Some(combined))
}

pub(super) fn load_generated_references(path: &Path) -> Result<GeneratedReferences, String> {
    let manifest_path = std::fs::canonicalize(path)
        .map_err(|error| format!("resolving reference manifest failed: {error}"))?;
    let root = manifest_path
        .parent()
        .ok_or_else(|| "reference manifest has no parent directory".to_string())?;
    let encoded = std::fs::read(&manifest_path)
        .map_err(|error| format!("reading reference manifest failed: {error}"))?;
    let manifest: serde_json::Value = serde_json::from_slice(&encoded)
        .map_err(|error| format!("parsing reference manifest failed: {error}"))?;
    if manifest["schema_version"] != 1 || manifest["kind"] != "ferrule.reference_outputs" {
        return Err("reference manifest has an unsupported schema or kind".into());
    }
    let records = manifest["samples"]
        .as_array()
        .ok_or_else(|| "reference manifest has no samples array".to_string())?;
    let mut outputs = BTreeMap::new();
    for record in records {
        if record["status"] != "passed" {
            continue;
        }
        let generated = record["outputs"]
            .as_array()
            .ok_or_else(|| "passed reference record has no outputs array".to_string())?;
        if generated.is_empty() {
            continue;
        }
        let sample = manifest_relative_path(&record["file"], "sample file")?;
        let directory = manifest_relative_path(&record["directory"], "output directory")?;
        let mut references = Vec::with_capacity(generated.len());
        for output in generated {
            let output = manifest_relative_path(output, "generated output")?;
            let reference =
                std::fs::canonicalize(root.join(&directory).join(output)).map_err(|error| {
                    format!(
                        "resolving generated reference for `{}` failed: {error}",
                        sample.display()
                    )
                })?;
            if !reference.starts_with(root) || !reference.is_file() {
                return Err(format!(
                    "generated reference for `{}` escapes the manifest directory",
                    sample.display()
                ));
            }
            references.push(reference);
        }
        if outputs.insert(sample.clone(), references).is_some() {
            return Err(format!(
                "reference manifest repeats sample `{}`",
                sample.display()
            ));
        }
    }
    Ok(GeneratedReferences { outputs })
}

fn manifest_relative_path(value: &serde_json::Value, label: &str) -> Result<PathBuf, String> {
    let value = value
        .as_str()
        .ok_or_else(|| format!("reference manifest {label} is not a string"))?;
    let path = PathBuf::from(value.replace('\\', "/"));
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(format!(
            "reference manifest {label} `{value}` is not a contained relative path"
        ));
    }
    Ok(path)
}

pub(super) fn first_instance_difference(expected: &Instance, actual: &Instance) -> String {
    instance_difference("$", expected, actual, 0)
        .unwrap_or_else(|| "values differ but no structural difference was located".into())
}

pub(super) fn instances_semantically_equal(expected: &Instance, actual: &Instance) -> bool {
    instances_equal_by(
        expected,
        actual,
        |expected, actual| expected == actual,
        false,
    )
}

pub(super) fn host_path_instances_equal(expected: &Instance, actual: &Instance) -> bool {
    instances_equal_by(
        expected,
        actual,
        |expected, actual| match (expected, actual) {
            (ir::Value::String(expected), ir::Value::String(actual)) => {
                normalize_host_path_text(expected) == normalize_host_path_text(actual)
            }
            _ => expected == actual,
        },
        true,
    )
}

fn instances_equal_by(
    expected: &Instance,
    actual: &Instance,
    scalar_equal: fn(&ir::Value, &ir::Value) -> bool,
    normalize_document_paths: bool,
) -> bool {
    match (expected, actual) {
        (Instance::Scalar(expected), Instance::Scalar(actual)) => scalar_equal(expected, actual),
        (Instance::Group(expected), Instance::Group(actual)) => {
            groups_equal_by(expected, actual, scalar_equal, normalize_document_paths)
        }
        (Instance::Repeated(expected), Instance::Repeated(actual))
        | (Instance::MappedSequence(expected), Instance::MappedSequence(actual)) => {
            expected.len() == actual.len()
                && expected.iter().zip(actual).all(|(expected, actual)| {
                    instances_equal_by(expected, actual, scalar_equal, normalize_document_paths)
                })
        }
        (Instance::DocumentSet(expected), Instance::DocumentSet(actual)) => {
            expected.len() == actual.len()
                && expected.iter().zip(actual).all(|(expected, actual)| {
                    let same_path = if normalize_document_paths {
                        normalize_host_path_text(expected.path())
                            == normalize_host_path_text(actual.path())
                    } else {
                        expected.path() == actual.path()
                    };
                    same_path
                        && instances_equal_by(
                            expected.value(),
                            actual.value(),
                            scalar_equal,
                            normalize_document_paths,
                        )
                })
        }
        _ => false,
    }
}

fn groups_equal_by(
    expected: &[(String, Instance)],
    actual: &[(String, Instance)],
    scalar_equal: fn(&ir::Value, &ir::Value) -> bool,
    normalize_document_paths: bool,
) -> bool {
    if expected.len() != actual.len() {
        return false;
    }
    if !group_names_are_unique(expected) || !group_names_are_unique(actual) {
        return expected.iter().zip(actual).all(
            |((expected_name, expected), (actual_name, actual))| {
                expected_name == actual_name
                    && instances_equal_by(expected, actual, scalar_equal, normalize_document_paths)
            },
        );
    }
    expected.iter().all(|(expected_name, expected)| {
        actual
            .iter()
            .find(|(actual_name, _)| actual_name == expected_name)
            .is_some_and(|(_, actual)| {
                instances_equal_by(expected, actual, scalar_equal, normalize_document_paths)
            })
    })
}

fn group_names_are_unique(fields: &[(String, Instance)]) -> bool {
    fields
        .iter()
        .enumerate()
        .all(|(index, (name, _))| fields[..index].iter().all(|(previous, _)| previous != name))
}

fn normalize_host_path_text(value: &str) -> String {
    value.replace('\\', "/").replace("Z:/", "/")
}

fn instance_difference(
    path: &str,
    expected: &Instance,
    actual: &Instance,
    depth: usize,
) -> Option<String> {
    if expected == actual {
        return None;
    }
    if depth >= 256 {
        return Some(format!("{path}: comparison depth exceeded"));
    }
    match (expected, actual) {
        (Instance::Scalar(expected), Instance::Scalar(actual)) => Some(format!(
            "{path}: expected {expected:?}, produced {actual:?}"
        )),
        (Instance::Group(expected), Instance::Group(actual)) => {
            if group_names_are_unique(expected) && group_names_are_unique(actual) {
                for (expected_name, expected) in expected {
                    let Some((_, actual)) = actual
                        .iter()
                        .find(|(actual_name, _)| actual_name == expected_name)
                    else {
                        return Some(format!(
                            "{path}: expected field `{expected_name}` is missing"
                        ));
                    };
                    let child_path = format!("{path}.{expected_name}");
                    if let Some(difference) =
                        instance_difference(&child_path, expected, actual, depth + 1)
                    {
                        return Some(difference);
                    }
                }
                if let Some((actual_name, _)) = actual.iter().find(|(actual_name, _)| {
                    !expected
                        .iter()
                        .any(|(expected_name, _)| expected_name == actual_name)
                }) {
                    return Some(format!("{path}: produced unexpected field `{actual_name}`"));
                }
                return None;
            }
            for (index, ((expected_name, expected), (actual_name, actual))) in
                expected.iter().zip(actual).enumerate()
            {
                if expected_name != actual_name {
                    return Some(format!(
                        "{path}[{index}]: expected field `{expected_name}`, produced `{actual_name}`"
                    ));
                }
                let child_path = format!("{path}.{expected_name}");
                if let Some(difference) =
                    instance_difference(&child_path, expected, actual, depth + 1)
                {
                    return Some(difference);
                }
            }
            Some(format!(
                "{path}: expected {} fields, produced {}",
                expected.len(),
                actual.len()
            ))
        }
        (Instance::Repeated(expected), Instance::Repeated(actual))
        | (Instance::MappedSequence(expected), Instance::MappedSequence(actual)) => {
            for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
                let item_path = format!("{path}[{index}]");
                if let Some(difference) =
                    instance_difference(&item_path, expected, actual, depth + 1)
                {
                    return Some(difference);
                }
            }
            Some(format!(
                "{path}: expected {} items, produced {}",
                expected.len(),
                actual.len()
            ))
        }
        (Instance::DocumentSet(expected), Instance::DocumentSet(actual)) => {
            for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
                if expected.path() != actual.path() {
                    return Some(format!(
                        "{path}[{index}]: expected document `{}`, produced `{}`",
                        expected.path(),
                        actual.path()
                    ));
                }
                let item_path = format!("{path}[{index}]");
                if let Some(difference) =
                    instance_difference(&item_path, expected.value(), actual.value(), depth + 1)
                {
                    return Some(difference);
                }
            }
            Some(format!(
                "{path}: expected {} documents, produced {}",
                expected.len(),
                actual.len()
            ))
        }
        _ => Some(format!(
            "{path}: expected {}, produced {}",
            instance_kind(expected),
            instance_kind(actual)
        )),
    }
}

fn instance_kind(instance: &Instance) -> &'static str {
    match instance {
        Instance::Scalar(_) => "scalar",
        Instance::Group(_) => "group",
        Instance::Repeated(_) => "repeated sequence",
        Instance::DocumentSet(_) => "document set",
        Instance::MappedSequence(_) => "mapped sequence",
    }
}

#[cfg(test)]
mod tests {
    use ir::{Instance, Value};

    use super::{
        first_instance_difference, host_path_instances_equal, instances_semantically_equal,
    };

    #[test]
    fn equates_wine_and_native_host_paths_only_when_the_surrounding_values_match() {
        let expected = Instance::Scalar(Value::String(
            "read from file: Z:\\work\\inputs\\one.xml".into(),
        ));
        let actual = Instance::Scalar(Value::String("read from file: /work/inputs/one.xml".into()));
        let other = Instance::Scalar(Value::String("read from file: /work/inputs/two.xml".into()));

        assert!(host_path_instances_equal(&expected, &actual));
        assert!(!host_path_instances_equal(&expected, &other));
    }

    #[test]
    fn group_field_order_is_semantic_free_but_sequence_order_is_not() {
        let scalar = |value| Instance::Scalar(Value::Int(value));
        let expected = Instance::Group(vec![
            (
                "nested".into(),
                Instance::Group(vec![
                    ("first".into(), scalar(1)),
                    ("second".into(), scalar(2)),
                ]),
            ),
            (
                "items".into(),
                Instance::Repeated(vec![scalar(3), scalar(4)]),
            ),
        ]);
        let reordered = Instance::Group(vec![
            (
                "items".into(),
                Instance::Repeated(vec![scalar(3), scalar(4)]),
            ),
            (
                "nested".into(),
                Instance::Group(vec![
                    ("second".into(), scalar(2)),
                    ("first".into(), scalar(1)),
                ]),
            ),
        ]);
        assert!(instances_semantically_equal(&expected, &reordered));

        let reordered_items = Instance::Group(vec![
            (
                "items".into(),
                Instance::Repeated(vec![scalar(4), scalar(3)]),
            ),
            (
                "nested".into(),
                Instance::Group(vec![
                    ("second".into(), scalar(2)),
                    ("first".into(), scalar(1)),
                ]),
            ),
        ]);
        assert!(!instances_semantically_equal(&expected, &reordered_items));
        assert!(
            first_instance_difference(&expected, &reordered_items)
                .contains("$.items[0]: expected Int(3), produced Int(4)")
        );
    }

    #[test]
    fn duplicate_group_names_retain_positional_comparison() {
        let expected = Instance::Group(vec![
            ("value".into(), Instance::Scalar(Value::Int(1))),
            ("value".into(), Instance::Scalar(Value::Int(2))),
        ]);
        let reordered = Instance::Group(vec![
            ("value".into(), Instance::Scalar(Value::Int(2))),
            ("value".into(), Instance::Scalar(Value::Int(1))),
        ]);

        assert!(!instances_semantically_equal(&expected, &reordered));
    }
}
