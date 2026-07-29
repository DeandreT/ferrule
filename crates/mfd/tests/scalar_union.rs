use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{ScalarType, ScalarTypeSet, SchemaKind, SchemaNode};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_scalar_union_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn schema_node_mut<'a>(schema: &'a mut SchemaNode, path: &[&str]) -> Option<&'a mut SchemaNode> {
    let Some((head, tail)) = path.split_first() else {
        return Some(schema);
    };
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        return None;
    };
    let child = children.iter_mut().find(|child| child.name == *head)?;
    schema_node_mut(child, tail)
}

#[test]
fn json_scalar_union_survives_export_and_reimport() -> Result<(), Box<dyn std::error::Error>> {
    let mut project = mfd::import(&fixture("inventory.mfd"))?.project;
    let types = ScalarTypeSet::new([ScalarType::String, ScalarType::Int])
        .ok_or("test scalar union members must be distinct")?;
    let store = schema_node_mut(&mut project.source, &["store"])
        .ok_or("inventory source store field is missing")?;
    store.kind = SchemaKind::ScalarUnion { types };

    let directory = TempDir::new()?;
    let design = directory.0.join("mapping.mfd");
    assert!(mfd::export(&project, &design)?.is_empty());

    let sibling = std::fs::read_to_string(directory.0.join("mapping-source.schema.json"))?;
    let sibling: serde_json::Value = serde_json::from_str(&sibling)?;
    assert_eq!(
        sibling
            .pointer("/properties/store/type")
            .and_then(serde_json::Value::as_array),
        Some(&vec![
            serde_json::Value::String("string".to_string()),
            serde_json::Value::String("integer".to_string())
        ])
    );
    let design_xml = std::fs::read_to_string(&design)?;
    let design_document = roxmltree::Document::parse(&design_xml)?;
    let store_property = design_document.descendants().find(|node| {
        node.has_tag_name("entry")
            && node.attribute("type") == Some("json-property")
            && node.attribute("name") == Some("store")
    });
    let store_property = store_property.ok_or("exported store property is missing")?;
    let union_pin = store_property
        .children()
        .find(|node| node.has_tag_name("entry") && node.attribute("outkey").is_some())
        .ok_or("exported store union pin is missing")?;
    assert_eq!(union_pin.attribute("name"), Some("value"));

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let store = imported
        .project
        .source
        .child("store")
        .ok_or("round-trip store field is missing")?;
    assert!(matches!(
        store.kind,
        SchemaKind::ScalarUnion { types: actual } if actual == types
    ));
    Ok(())
}

#[test]
fn non_json_scalar_union_export_fails_before_writing_artifacts()
-> Result<(), Box<dyn std::error::Error>> {
    let mut project = mfd::import(&fixture("inventory.mfd"))?.project;
    let types = ScalarTypeSet::new([ScalarType::String, ScalarType::Int])
        .ok_or("test scalar union members must be distinct")?;
    let store = schema_node_mut(&mut project.source, &["store"])
        .ok_or("inventory source store field is missing")?;
    store.kind = SchemaKind::ScalarUnion { types };
    project.source_path = Some("inventory.xml".to_string());
    project.source_options = mapping::FormatOptions::default();

    let directory = TempDir::new()?;
    let design = directory.0.join("mapping.mfd");
    let error = match mfd::export(&project, &design) {
        Ok(_) => return Err("XML scalar union unexpectedly exported".into()),
        Err(error) => error,
    };
    let message = error.to_string();
    assert!(message.contains("source XML field `store`"), "{message}");
    assert!(
        message.contains("require one concrete scalar type"),
        "{message}"
    );
    assert!(!design.exists());
    assert!(!directory.0.join("mapping-source.xsd").exists());
    Ok(())
}
