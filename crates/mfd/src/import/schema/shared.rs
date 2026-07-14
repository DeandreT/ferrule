use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ir::SchemaNode;

pub(in crate::import) fn resolve_xml_schema_reference(
    mfd_path: &Path,
    relative: &str,
) -> Result<PathBuf, String> {
    let portable = relative.replace('\\', "/");
    let base = mfd_path.parent().unwrap_or_else(|| Path::new("."));
    let direct = base.join(portable);
    if direct.is_file() {
        return Ok(direct);
    }

    let file_name = direct
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("schema path `{relative}` has no file name"))?;
    let directory = direct.parent().unwrap_or(base);
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("could not resolve schema `{relative}` ({error})"))?;
    let mut matched = None;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("could not resolve schema `{relative}` ({error})"))?;
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(file_name))
        {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|error| format!("could not inspect schema `{relative}` ({error})"))?;
        if !file_type.is_file() {
            continue;
        }
        if matched.is_some() {
            return Err(format!(
                "schema `{relative}` has multiple case-insensitive sibling matches"
            ));
        }
        matched = Some(entry.path());
    }
    matched.ok_or_else(|| format!("schema `{relative}` was not found"))
}

pub(in crate::import) fn read_xml_schema_file(
    schema_path: &Path,
    root: Option<&str>,
) -> Result<SchemaNode, String> {
    let extension = schema_path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    if extension.eq_ignore_ascii_case("dtd") {
        format_xml::dtd::import_root(schema_path, root).map_err(|error| error.to_string())
    } else {
        format_xml::xsd::import_root(schema_path, root).map_err(|error| error.to_string())
    }
}

pub(in crate::import) fn parse_u32(attr: Option<&str>) -> Option<u32> {
    attr.and_then(|attribute| attribute.parse().ok())
}

pub(in crate::import) fn entry_key_sets(root: &roxmltree::Node) -> (BTreeSet<u32>, BTreeSet<u32>) {
    let mut inputs = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    for entry in root.descendants().filter(|node| node.has_tag_name("entry")) {
        if let Some(key) = parse_u32(entry.attribute("inpkey")) {
            inputs.insert(key);
        }
        if let Some(key) = parse_u32(entry.attribute("outkey")) {
            outputs.insert(key);
        }
    }
    (inputs, outputs)
}

pub(in crate::import) fn is_default_output(component: &roxmltree::Node) -> bool {
    component
        .children()
        .find(|node| node.has_tag_name("properties"))
        .and_then(|properties| properties.attribute("XSLTDefaultOutput"))
        == Some("1")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "ferrule_mfd_schema_reference_{}_{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn exact_schema_reference_takes_precedence() {
        let dir = TempDir::new();
        let exact = dir.0.join("input.xsd");
        std::fs::write(&exact, "exact").unwrap();
        std::fs::write(dir.0.join("INPUT.XSD"), "fallback").unwrap();

        assert_eq!(
            resolve_xml_schema_reference(&dir.0.join("mapping.mfd"), "input.xsd").unwrap(),
            exact
        );
    }

    #[test]
    fn schema_reference_fallback_is_unique_and_stays_in_its_parent() {
        let dir = TempDir::new();
        let schemas = dir.0.join("schemas");
        std::fs::create_dir(&schemas).unwrap();
        let matched = schemas.join("Input.xsd");
        std::fs::write(&matched, "schema").unwrap();
        std::fs::write(dir.0.join("INPUT.XSD"), "wrong parent").unwrap();

        assert_eq!(
            resolve_xml_schema_reference(&dir.0.join("mapping.mfd"), "schemas\\input.xsd").unwrap(),
            matched
        );
    }

    #[test]
    fn ambiguous_or_missing_schema_references_are_rejected() {
        let dir = TempDir::new();
        std::fs::write(dir.0.join("Input.xsd"), "first").unwrap();
        std::fs::write(dir.0.join("INPUT.XSD"), "second").unwrap();

        let ambiguous =
            resolve_xml_schema_reference(&dir.0.join("mapping.mfd"), "input.xsd").unwrap_err();
        assert!(ambiguous.contains("multiple case-insensitive sibling matches"));

        let missing =
            resolve_xml_schema_reference(&dir.0.join("mapping.mfd"), "missing.xsd").unwrap_err();
        assert!(missing.contains("was not found"));
    }
}
