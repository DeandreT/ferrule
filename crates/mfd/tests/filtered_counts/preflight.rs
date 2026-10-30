//! Test-fixture XML/identity checks only. This module never invokes the importer.
//! The frozen catalog binds every rendered variant, including intentionally
//! malformed raw pins, to its reviewed component/declaration/edge identities.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct Identity {
    sha256: String,
    components: usize,
    pins: usize,
    edges: usize,
}

#[derive(Debug, Deserialize)]
struct Expected {
    #[serde(flatten)]
    identity: Identity,
    resource_group: String,
}

fn catalog() -> &'static BTreeMap<String, Expected> {
    static CATALOG: std::sync::OnceLock<BTreeMap<String, Expected>> = std::sync::OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../fixtures/filtered-counts/preflight-identities.json"
        ))
        .unwrap()
    })
}

fn resources() -> &'static BTreeMap<String, BTreeMap<String, String>> {
    static RESOURCES: std::sync::OnceLock<BTreeMap<String, BTreeMap<String, String>>> =
        std::sync::OnceLock::new();
    RESOURCES.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../fixtures/filtered-counts/preflight-resources.json"
        ))
        .unwrap()
    })
}

fn expanded(namespace: Option<&str>, name: &str) -> String {
    match namespace {
        Some(namespace) => format!("{{{namespace}}}{name}"),
        None => name.into(),
    }
}

fn identity(xml: &str) -> Result<Identity, String> {
    let document = roxmltree::Document::parse(xml)
        .map_err(|error| format!("fixture preflight XML parse: {error}"))?;
    let root = document.root_element();
    if !root.has_tag_name("mapping") || root.attribute("version") != Some("26") {
        return Err("fixture preflight requires the expected mapping root/version".into());
    }
    let mut rows = Vec::new();
    let (mut components, mut pins, mut edges) = (0, 0, 0);
    for node in document.descendants().filter(roxmltree::Node::is_element) {
        components += usize::from(node.has_tag_name("component"));
        pins += usize::from(
            node.has_tag_name("datapoint")
                || node.has_tag_name("entry")
                    && (node.attribute("inpkey").is_some() || node.attribute("outkey").is_some()),
        );
        edges += usize::from(node.has_tag_name("edge"));
        let mut path = Vec::new();
        let mut current = Some(node);
        while let Some(part) = current {
            if part.is_element() {
                let index = part
                    .parent()
                    .unwrap()
                    .children()
                    .filter(roxmltree::Node::is_element)
                    .position(|sibling| sibling == part)
                    .unwrap();
                path.push(format!(
                    "{}[{index}]",
                    expanded(part.tag_name().namespace(), part.tag_name().name())
                ));
            }
            current = part.parent();
        }
        path.reverse();
        let mut attributes = node
            .attributes()
            .map(|attribute| {
                (
                    expanded(attribute.namespace(), attribute.name()),
                    attribute.value().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        attributes.sort();
        rows.push((path.join("/"), attributes));
    }
    let bytes = serde_json::to_vec(&rows).map_err(|error| error.to_string())?;
    Ok(Identity {
        sha256: format!("{:x}", Sha256::digest(bytes)),
        components,
        pins,
        edges,
    })
}

pub(super) fn check(label: &str, xml: &str) -> Result<(), String> {
    let actual = identity(xml).map_err(|error| format!("{label}: {error}"))?;
    let expected = catalog()
        .get(label)
        .ok_or_else(|| format!("{label}: no frozen fixture identity"))?;
    if actual != expected.identity {
        return Err(format!(
            "{label}: fixture component/pin/edge identity differs: expected {expected:?}, actual {actual:?}"
        ));
    }
    Ok(())
}

pub(super) fn check_package(label: &str, path: &Path) -> Result<(), String> {
    let xml = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    check(label, &xml)?;
    let expected = &catalog()[label];
    let files = resources()
        .get(&expected.resource_group)
        .ok_or_else(|| format!("{label}: unknown fixture resource group"))?;
    for (name, hash) in files {
        if Path::new(name).file_name().and_then(|name| name.to_str()) != Some(name.as_str()) {
            return Err("fixture resource identity must be one local filename".into());
        }
        let bytes =
            std::fs::read(path.parent().unwrap().join(name)).map_err(|error| error.to_string())?;
        if format!("{:x}", Sha256::digest(&bytes)) != *hash {
            return Err(format!(
                "{label}: fixture resource {name} differs from its frozen identity"
            ));
        }
        if name.ends_with(".json") {
            serde_json::from_slice::<serde_json::Value>(&bytes)
                .map_err(|error| error.to_string())?;
        } else {
            let text = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
            roxmltree::Document::parse(text)
                .map_err(|error| format!("{label}: resource XML parse: {error}"))?;
        }
    }
    Ok(())
}

pub(super) fn check_all(designs: &[(String, String)]) -> Result<(), String> {
    let labels = designs
        .iter()
        .map(|(label, _)| label)
        .collect::<BTreeSet<_>>();
    if labels.len() != designs.len() || labels != catalog().keys().collect::<BTreeSet<_>>() {
        return Err(
            "rendered fixture catalog membership differs from the frozen identities".into(),
        );
    }
    for (label, xml) in designs {
        check(label, xml)?;
    }
    Ok(())
}
