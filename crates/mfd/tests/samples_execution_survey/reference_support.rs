use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};

use ir::Instance;

const REFERENCE_MANIFEST_ENV: &str = "FERRULE_REFERENCE_SAMPLES_MANIFEST";

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
    if manifest["schema_version"] != 1 || manifest["kind"] != "ferrule.reference_samples_outputs" {
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
