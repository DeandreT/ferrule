//! Document-local `.mfd` libraries. No mapping import, module discovery, schema
//! parsing, payload reads or evaluation occurs here. XML borrows only the local
//! immutable buffer; exported recipes/evidence are owned before it is dropped.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use sha2::{Digest, Sha256};

use super::{Definition, OutputExpr, Registry, ScalarExpr, limits};
use crate::resource::{
    BoundResource, MappingLibraryDefinitionRecordV1, MappingLibraryFunctionIdentityV1,
    ResourceResolver,
};

type Key = (String, String);

mod consumers;

const MAX_FILES: usize = 32;
const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
const MAX_DEPTH: usize = 16;
const MAX_IMPORTS: usize = 128;
const MAX_NODES: usize = 131_072;
const MAX_DEFINITIONS: usize = 512;
const MAX_SYMBOL_BYTES: usize = 256;
const MAX_LITERAL_BYTES: usize = 1024;

pub(super) fn reserved_owner(library: &str) -> bool {
    matches!(
        library,
        "core"
            | "lang"
            | "xml"
            | "json"
            | "text"
            | "db"
            | "ferrule"
            | "xpath2"
            | "mapforce_nodefunction"
            | "webservice"
            | "wsdl"
    )
}

pub(super) fn has_mapping_imports(mapping: roxmltree::Node<'_, '_>) -> bool {
    mapping
        .children()
        .filter(|n| n.has_tag_name("settings"))
        .flat_map(|n| n.children().filter(|n| n.has_tag_name("libraries")))
        .flat_map(|n| n.children().filter(|n| n.has_tag_name("import")))
        .any(|n| {
            n.attribute("path")
                .is_some_and(|p| p.to_ascii_lowercase().ends_with(".mfd"))
        })
}

struct Export {
    key: Key,
    definition: Result<Definition, String>,
    origin: MappingLibraryDefinitionRecordV1,
}

#[derive(Default)]
struct Budget {
    files: BTreeMap<PathBuf, [u8; 32]>,
    bytes: usize,
    imports: usize,
    nodes: usize,
    definitions: usize,
    construction: limits::ConstructionBudget,
    evidence_bytes: usize,
    active: Vec<PathBuf>,
}

pub(super) fn read(
    mapping: &roxmltree::Node<'_, '_>,
    resources: &ResourceResolver,
    registry: &mut Registry,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    let references = imports(*mapping)?;
    let origin_sha256 = Sha256::digest(mapping.document().input_text().as_bytes()).into();
    let mut budget = Budget::default();
    registry.construction = Some(budget.construction.clone());
    let mut exports = Vec::new();
    for reference in references {
        let reader = resources.mapping_library(&reference, origin_sha256)?;
        exports.extend(
            reader
                .finish(read_document(
                    reader.path(),
                    &reader,
                    resources.mapping_path(),
                    origin_sha256,
                    &reference,
                    resources,
                    &mut budget,
                    warnings,
                ))
                .map_err(|reason| format!("`{reference}`: {reason}"))?,
        );
    }
    // No partial imported closure is registered if an import could not be
    // acquired/validated. Embedded declarations are still read by the caller.
    merge(registry, exports, warnings);
    Ok(())
}

fn imports(mapping: roxmltree::Node<'_, '_>) -> Result<Vec<String>, String> {
    let mut references = Vec::new();
    let mut seen = BTreeSet::new();
    for settings in mapping.children().filter(|n| n.has_tag_name("settings")) {
        for libraries in settings.children().filter(|n| n.has_tag_name("libraries")) {
            for import in libraries.children().filter(|n| n.is_element()) {
                if !import.has_tag_name("import")
                    || import.tag_name().namespace().is_some()
                    || import.attributes().any(|a| {
                        a.namespace().is_some() || !matches!(a.name(), "path" | "saverelative")
                    })
                    || import.children().any(|n| n.is_element())
                    || import.attribute("saverelative") != Some("1")
                {
                    return Err("only document-local path/saverelative=1 MFD imports are supported; aliases/global/custom formats are not inferred".into());
                }
                let path = import.attribute("path").unwrap_or_default();
                if path.is_empty()
                    || path.len() > 4096
                    || path.contains('\0')
                    || !path.to_ascii_lowercase().ends_with(".mfd")
                {
                    return Err("declared library must name a bounded .mfd metadata file".into());
                }
                if !seen.insert(path.to_string()) || references.len() >= MAX_IMPORTS {
                    return Err("duplicate or excessive declared library imports".into());
                }
                references.push(path.into());
            }
        }
    }
    Ok(references)
}

#[allow(clippy::too_many_arguments)]
fn read_document(
    path: &Path,
    reader: &Rc<BoundResource>,
    declaring_document: &Path,
    declaring_sha256: [u8; 32],
    reference: &str,
    resources: &ResourceResolver,
    budget: &mut Budget,
    warnings: &mut Vec<String>,
) -> Result<Vec<Export>, String> {
    budget.imports += 1;
    if budget.imports > MAX_IMPORTS || budget.active.len() >= MAX_DEPTH {
        return Err("mapping-library import count/depth budget exceeded".into());
    }
    if budget.active.iter().any(|p| p == path) {
        return Err("mapping-library file import cycle refused".into());
    }
    if !budget.files.contains_key(path) && budget.files.len() >= MAX_FILES {
        return Err("mapping-library file-count budget exceeded before resource read".into());
    }
    let bytes = reader
        .bytes_bounded(path, MAX_BYTES.saturating_sub(budget.bytes))
        .map_err(|e| format!("unresolved resource: {e}"))?;
    let sha256 = Sha256::digest(bytes.as_ref()).into();
    if let Some(previous) = budget.files.insert(path.to_path_buf(), sha256) {
        if previous != sha256 {
            return Err(
                "mapping-library identity delivered different immutable byte hashes".into(),
            );
        }
    }
    budget.bytes = budget.bytes.saturating_add(bytes.len());
    if bytes.len() > MAX_FILE_BYTES || budget.bytes > MAX_BYTES || budget.files.len() > MAX_FILES {
        return Err("mapping-library file/byte budget exceeded".into());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "mapping library is not UTF-8")?;
    // roxmltree's default also refuses DTDs. Refuse explicitly rather than
    // permitting entity-based metadata or another library format.
    if text.contains("<!DOCTYPE") || text.contains("<!ENTITY") {
        return Err("mapping-library DTD/entity declarations are refused".into());
    }
    let document = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: MAX_NODES.saturating_sub(budget.nodes) as u32,
            ..roxmltree::ParsingOptions::default()
        },
    )
    .map_err(|e| e.to_string())?;
    let mapping = document.root_element();
    if !mapping.has_tag_name("mapping") || mapping.tag_name().namespace().is_some() {
        return Err("declared mapping library root must be unnamespaced <mapping>".into());
    }
    budget.nodes += document.descendants().count();
    if budget.nodes > MAX_NODES {
        return Err("mapping-library XML/graph node budget exceeded".into());
    }
    let declaration_count = mapping
        .children()
        .filter(|n| {
            n.has_tag_name("component") && n.attribute("library").is_some_and(|l| !l.is_empty())
        })
        .count();
    budget.definitions = budget.definitions.saturating_add(declaration_count);
    if budget.definitions > MAX_DEFINITIONS {
        return Err("mapping-library definition count budget exceeded".into());
    }
    budget
        .construction
        .claim(declaration_count.saturating_mul(8), 0, 0)?;
    let declarations = mapping
        .children()
        .filter(|n| {
            n.has_tag_name("component") && n.attribute("library").is_some_and(|l| !l.is_empty())
        })
        .collect::<Vec<_>>();
    // Namespace keys, saved XML/evidence copies and graph metadata are bounded
    // before the registry creates any owned declarations. Recursive expression
    // work has a separate per-construction charge against this SAME quota.
    for declaration in &declarations {
        budget.construction.claim(
            declaration.descendants().count().saturating_mul(8),
            limits::decoded_bytes(*declaration)
                .saturating_mul(16)
                .saturating_add(declaration.range().len().saturating_mul(2)),
            0,
        )?;
    }

    budget.active.push(path.to_path_buf());
    let result = (|| {
        let mut registry = Registry {
            external_owner: true,
            construction: Some(budget.construction.clone()),
            ..Registry::default()
        };
        let mut imported = Vec::new();
        for reference in imports(mapping)? {
            let dependency = dependency_path(reader.root(), path, &reference)?;
            imported.extend(read_document(
                &dependency,
                reader,
                path,
                sha256,
                &reference,
                resources,
                budget,
                warnings,
            )?);
        }
        merge(&mut registry, imported, warnings);
        registry.read_declarations(&mapping, path, None, warnings);
        registry.finish_construction()?;
        // Transfer recipes; never clone a complete expanded Definition just to
        // export it from its temporary owner registry.
        budget
            .construction
            .claim(registry.definitions.len(), 0, 0)?;
        let mut definitions = std::mem::take(&mut registry.definitions)
            .into_iter()
            .map(Some)
            .collect::<Vec<_>>();

        // Only this document's own declarations are exported. Transitive
        // re-export visibility is not established by the saved format/docs;
        // nested calls can use this owner's direct imports, never main scope.
        let mut exports = Vec::new();
        for component in declarations {
            let key = (
                component
                    .attribute("library")
                    .unwrap_or_default()
                    .to_string(),
                component.attribute("name").unwrap_or_default().to_string(),
            );
            let definition = registry
                .supported(&key.0, &key.1)
                .and_then(|index| definitions.get_mut(index).and_then(Option::take))
                .ok_or_else(|| {
                    registry
                        .unsupported_reason(&key.0, &key.1)
                        .unwrap_or("unresolved imported declaration")
                        .to_string()
                });
            if let Err(reason) = &definition {
                warnings.push(format!(
                    "declared mapping-library function `{}` ({}) refused: {reason}",
                    key.1, key.0
                ));
            }
            let origin = MappingLibraryDefinitionRecordV1 {
                declaring_document: declaring_document.into(),
                declaring_sha256,
                original_reference: reference.into(),
                identity: MappingLibraryFunctionIdentityV1 {
                    document: path.into(),
                    sha256,
                    library: key.0.clone(),
                    name: key.1.clone(),
                },
                declaration_xml: text[component.range()].into(),
                // v2 accepts only leaf call boundaries. A refused declaration's
                // raw XML records unresolved references, not an admitted closure.
                dependencies: Vec::new(),
                refusal: definition.as_ref().err().cloned(),
            };
            budget.evidence_bytes = budget
                .evidence_bytes
                .saturating_add(origin.retained_bytes());
            if budget.evidence_bytes > MAX_BYTES {
                return Err(
                    "mapping-library declaration/dependency evidence budget exceeded".into(),
                );
            }
            resources.record_mapping_library_definition(origin.clone())?;
            exports.push(Export {
                key,
                definition,
                origin,
            });
        }
        Ok(exports)
    })();
    budget.active.pop();
    result
}

pub(super) fn reserve_helper(
    definition: &Definition,
    library: &str,
    name: &str,
    budget: &limits::ConstructionBudget,
) -> Result<(), String> {
    let interface = definition
        .scalar_interface
        .as_ref()
        .ok_or("imported scalar interface absent")?;
    budget.claim(
        8usize.saturating_add(interface.parameters.len().saturating_mul(4)),
        library
            .len()
            .saturating_add(name.len())
            .saturating_add(
                interface
                    .parameters
                    .iter()
                    .map(|p| p.name.len())
                    .sum::<usize>(),
            )
            .saturating_add(
                interface
                    .outputs
                    .iter()
                    .map(|p| p.name.len())
                    .sum::<usize>(),
            ),
        0,
    )?;
    for output in definition.outputs.values() {
        let OutputExpr::Scalar(expression) = output else {
            return Err("imported non-scalar output recipe refused".into());
        };
        reserve_expression(expression, budget, 0)?;
    }
    Ok(())
}

fn reserve_expression(
    expression: &ScalarExpr,
    budget: &limits::ConstructionBudget,
    depth: usize,
) -> Result<(), String> {
    budget.claim(1, 0, depth)?;
    match expression {
        ScalarExpr::Parameter(_) => Ok(()),
        ScalarExpr::Const(value) => budget.claim(0, limits::scalar_text(value)?, depth),
        ScalarExpr::Call { function, args } => {
            budget.claim(args.len(), function.len(), depth)?;
            for arg in args {
                reserve_expression(arg, budget, depth + 1)?;
            }
            Ok(())
        }
        ScalarExpr::If {
            condition,
            then,
            else_,
        } => {
            budget.claim(3, 0, depth)?;
            for arg in [condition.as_ref(), then.as_ref(), else_.as_ref()] {
                reserve_expression(arg, budget, depth + 1)?;
            }
            Ok(())
        }
        _ => Err("imported inline/default/runtime/sequence recipe is unsupported".into()),
    }
}

fn merge(registry: &mut Registry, exports: Vec<Export>, warnings: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    for export in exports {
        if !seen.insert(export.key.clone()) {
            warnings.push(format!("ambiguous exact function `{}` ({}) in local mapping-library imports; no import-order winner is selected", export.key.1, export.key.0));
            registry.supported.remove(&export.key);
            registry.unsupported.insert(export.key, "ambiguous exact function in multiple local imports; import-order ownership is not inferred".into());
            continue;
        }
        match export.definition {
            Ok(definition) => {
                let index = registry.definitions.len();
                registry.definitions.push(definition);
                registry.origins.insert(index, export.origin);
                registry.supported.insert(export.key, index);
            }
            Err(reason) => {
                registry.unsupported.insert(export.key, reason);
            }
        }
    }
}

fn dependency_path(root: &Path, owner: &Path, reference: &str) -> Result<PathBuf, String> {
    if reference.len() > 4096 || reference.contains([':', '\0']) {
        return Err("mapping-library dependency path refused".into());
    }
    let portable = reference.replace('\\', "/");
    let mut relative = owner
        .parent()
        .and_then(|p| p.strip_prefix(root).ok())
        .ok_or("mapping-library owner is outside authorized root")?
        .to_path_buf();
    for component in Path::new(&portable).components() {
        match component {
            Component::Normal(name) => relative.push(name),
            Component::CurDir => {}
            Component::ParentDir if relative.pop() => {}
            _ => return Err("mapping-library dependency escapes its authorizing root".into()),
        }
    }
    Ok(root.join(relative))
}

/// Conservative source-proven subset. In particular, scan *all* body
/// components, not just output-reachable ones: unknown/effectful dead branches
/// cannot be silently discarded by an expression recognizer.
pub(super) fn validate_definition(component: roxmltree::Node<'_, '_>) -> Result<(), String> {
    let refuse = |detail: &str| format!("imported definition refused: {detail}");
    if ["name", "library"].iter().any(|key| {
        component
            .attribute(*key)
            .is_none_or(|v| v.is_empty() || v.len() > MAX_SYMBOL_BYTES)
    }) {
        return Err(refuse("missing/oversized qualified function identity"));
    }
    if reserved_owner(component.attribute("library").unwrap_or_default()) {
        return Err(refuse(
            "reserved/built-in/node-function library ownership is unsupported",
        ));
    }
    if component
        .attribute("name")
        .is_none_or(|n| n.trim().is_empty())
        || component.descendants().any(|n| {
            n.is_element()
                && (n.tag_name().namespace().is_some()
                    || n.attributes().any(|a| a.namespace().is_some()))
        })
    {
        return Err(refuse("missing name or unsupported XML namespace"));
    }
    if component
        .attributes()
        .any(|a| !matches!(a.name(), "library" | "name" | "inline" | "uid" | "kind"))
    {
        return Err(refuse("unknown declaration context/visibility metadata"));
    }
    if component
        .attribute("inline")
        .is_some_and(|v| !matches!(v, "0" | "1"))
        || component.attribute("kind").is_some_and(|v| v != "19")
    {
        return Err(refuse("unknown declaration kind/inline contract"));
    }
    if component.attribute("inline") == Some("1") {
        return Err(refuse(
            "inline declaration needs declared type adapters and call-consumption proof; v2 supports regular first-class boundaries only",
        ));
    }
    let structures = component
        .children()
        .filter(|n| n.has_tag_name("structure"))
        .collect::<Vec<_>>();
    let [structure] = structures.as_slice() else {
        return Err(refuse("missing/duplicate declarative structure"));
    };
    if component
        .children()
        .filter(|n| n.is_element())
        .any(|n| !n.has_tag_name("structure"))
        || structure
            .children()
            .filter(|n| n.is_element())
            .any(|n| !matches!(n.tag_name().name(), "children" | "graph"))
        || structure
            .children()
            .filter(|n| n.has_tag_name("graph"))
            .count()
            != 1
    {
        return Err(refuse("unsupported function structure/connection metadata"));
    }
    let lists = structure
        .children()
        .filter(|n| n.has_tag_name("children"))
        .collect::<Vec<_>>();
    let [children] = lists.as_slice() else {
        return Err(refuse("missing/duplicate component list"));
    };
    let mut pins = BTreeSet::new();
    let mut parameter_names = BTreeSet::new();
    let mut output_names = BTreeSet::new();
    for child in children.children().filter(|n| n.is_element()) {
        let library = child.attribute("library").unwrap_or_default();
        let name = child.attribute("name").unwrap_or_default();
        let kind = child.attribute("kind").unwrap_or_default();
        if kind == "19" {
            return Err(refuse(
                "nested call needs declared input/output type adaptation and eager regular-call consumption boundary; unsupported before expansion",
            ));
        }
        if name.len() > MAX_SYMBOL_BYTES || library.len() > MAX_SYMBOL_BYTES {
            return Err(refuse("scalar component symbol byte budget exceeded"));
        }
        if !child.has_tag_name("component")
            || child.descendants().any(|n| n.has_tag_name("structure"))
        {
            return Err(refuse(
                "nested structure/non-component metadata is unsupported",
            ));
        }
        let allowed = match (library, kind) {
            ("core", "2" | "3" | "4" | "6" | "7") => true,
            ("core" | "lang", "5") => {
                !matches!(
                    name,
                    "sleep"
                        | "create-guid"
                        | "random"
                        | "now"
                        | "mfd-filepath"
                        | "main-mfd-filepath"
                        | "resolve-filepath"
                        | "set-xsi-nil"
                ) && (super::super::function::map_name(name).is_some()
                    || matches!(name, "set-empty" | "not-exists"))
            }
            _ => false,
        };
        if !allowed {
            return Err(refuse(&format!(
                "unknown, effectful, structured or origin-sensitive body component `{name}` ({library}, kind {kind})"
            )));
        }
        if child
            .attributes()
            .any(|a| !matches!(a.name(), "name" | "library" | "uid" | "kind"))
            || child.children().filter(|n| n.is_element()).any(|n| {
                !matches!(
                    n.tag_name().name(),
                    "sources" | "targets" | "data" | "properties"
                )
            })
        {
            return Err(refuse("unknown scalar component metadata"));
        }
        if ["sources", "targets", "data"]
            .iter()
            .any(|tag| child.children().filter(|n| n.has_tag_name(*tag)).count() > 1)
        {
            return Err(refuse("duplicate scalar port/data containers"));
        }
        for container in child
            .children()
            .filter(|n| n.has_tag_name("sources") || n.has_tag_name("targets"))
        {
            if container.attributes().len() != 0 {
                return Err(refuse("unknown scalar pin-container metadata"));
            }
            let mut positions = BTreeSet::new();
            for (index, pin) in container.children().filter(|n| n.is_element()).enumerate() {
                if !pin.has_tag_name("datapoint")
                    || pin.children().any(|n| n.is_element())
                    || pin.attributes().any(|a| !matches!(a.name(), "key" | "pos"))
                {
                    return Err(refuse("unknown scalar pin metadata"));
                }
                let position = match pin.attribute("pos") {
                    Some(value) => super::parse_u32(Some(value)).map(|p| p as usize),
                    None => Some(index),
                }
                .ok_or_else(|| refuse("invalid scalar pin position"))?;
                if position >= 64 || !positions.insert(position) {
                    return Err(refuse(
                        "scalar pin position must be unique and below the reader's 64-slot bound; no child-index fallback",
                    ));
                }
                if container.has_tag_name("targets") && kind != "3" && (position != 0 || index != 0)
                {
                    return Err(refuse(
                        "scalar producer supports only its primary output at position 0",
                    ));
                }
                if kind == "3" && position >= 2
                    || kind == "4" && container.has_tag_name("sources") && position >= 3
                {
                    return Err(refuse(
                        "saved control port is not consumed by the supported scalar reader",
                    ));
                }
                if kind == "2"
                    && container.has_tag_name("sources")
                    && pin.attribute("key").is_some()
                {
                    return Err(refuse(
                        "constant trigger/context input consumption is unsupported",
                    ));
                }
                if matches!(kind, "6" | "7") && (position != 0 || index != 0) {
                    return Err(refuse(
                        "parameter default/output supports exactly one scalar slot at position 0",
                    ));
                }
            }
        }
        for data in child.children().filter(|n| n.has_tag_name("data")) {
            if data.attributes().len() != 0 {
                return Err(refuse("unknown scalar data attributes"));
            }
            for node in data.descendants().filter(|n| n.is_element() && *n != data) {
                let attributes: &[&str] = match (kind, node.tag_name().name()) {
                    ("2", "constant") => &["value", "datatype"],
                    ("6", "input") | ("7", "output") => &["datatype"],
                    ("6" | "7", "parameter") => &["usageKind", "name", "optional"],
                    ("19", "root") => &["rootindex"],
                    ("19", "entry") => &["name", "componentid", "inpkey", "outkey"],
                    _ => return Err(refuse("unknown scalar data/signature namespace metadata")),
                };
                if node.attributes().any(|a| !attributes.contains(&a.name())) {
                    return Err(refuse("unknown scalar data/signature attributes"));
                }
                let parent = node
                    .parent()
                    .ok_or_else(|| refuse("unowned scalar metadata"))?;
                if node.has_tag_name("entry") {
                    if !parent.has_tag_name("root") || node.children().any(|n| n.is_element()) {
                        return Err(refuse(
                            "structured/nested call-entry namespace is unsupported",
                        ));
                    }
                    if node
                        .attribute("name")
                        .is_none_or(|n| n.len() > MAX_SYMBOL_BYTES)
                    {
                        return Err(refuse("missing/oversized scalar call-port name"));
                    }
                } else if parent != data
                    || (!node.has_tag_name("root") && node.children().any(|n| n.is_element()))
                {
                    return Err(refuse("nested scalar metadata is unsupported"));
                }
            }
        }
        if child
            .descendants()
            .any(|n| n.has_tag_name("import") || n.has_tag_name("settings"))
        {
            return Err(refuse("function-local import ownership is not established"));
        }
        if kind == "2" {
            let constants = child
                .descendants()
                .filter(|n| n.has_tag_name("constant"))
                .collect::<Vec<_>>();
            let [constant] = constants.as_slice() else {
                return Err(refuse("missing/duplicate scalar literal"));
            };
            let ty = constant.attribute("datatype").unwrap_or_default();
            if constant
                .attribute("value")
                .is_some_and(|v| v.len() > MAX_LITERAL_BYTES)
            {
                return Err(refuse("scalar literal/default byte budget exceeded"));
            }
            if !matches!(
                ty,
                "string"
                    | "anySimpleType"
                    | "boolean"
                    | "integer"
                    | "int"
                    | "long"
                    | "decimal"
                    | "double"
                    | "float"
            ) || constant.attribute("value").is_none()
                || matches!(
                    super::super::function::parse_constant(
                        constant.attribute("value").unwrap_or_default(),
                        ty
                    ),
                    ir::Value::Null
                )
            {
                return Err(refuse(
                    "unsupported or invalid typed scalar literal/default",
                ));
            }
        }
        if child
            .children()
            .filter(|n| n.has_tag_name("properties"))
            .any(|n| n.attributes().len() != 0 || n.children().any(|n| n.is_element()))
            || child.descendants().any(|n| {
                n.attributes().any(|a| {
                    matches!(
                        a.name(),
                        "default" | "defaultValue" | "minOccurs" | "maxOccurs" | "priority"
                    )
                })
            })
        {
            return Err(refuse(
                "unsupported body/default/cardinality/context metadata",
            ));
        }
        for pin in child
            .descendants()
            .filter(|n| n.has_tag_name("datapoint") || n.has_tag_name("entry"))
        {
            for attribute in ["key", "inpkey", "outkey"] {
                if let Some(key) = pin.attribute(attribute) {
                    let key = super::parse_u32(Some(key))
                        .ok_or_else(|| refuse("invalid function-owned port key"))?;
                    if !pins.insert(key) {
                        return Err(refuse("invalid or colliding function-owned port key"));
                    }
                }
            }
        }
        if matches!(kind, "6" | "7") {
            let names = if kind == "6" {
                &mut parameter_names
            } else {
                &mut output_names
            };
            if name.trim().is_empty() || !names.insert(name.to_string()) {
                return Err(refuse("missing or duplicate scalar parameter name"));
            }
            let tag = if kind == "6" { "input" } else { "output" };
            let declarations = child
                .descendants()
                .filter(|n| n.has_tag_name(tag))
                .collect::<Vec<_>>();
            let [declaration] = declarations.as_slice() else {
                return Err(refuse(
                    "missing/duplicate typed scalar parameter declaration",
                ));
            };
            if !matches!(
                declaration.attribute("datatype"),
                Some(
                    "string"
                        | "boolean"
                        | "integer"
                        | "int"
                        | "long"
                        | "decimal"
                        | "double"
                        | "float"
                )
            ) {
                return Err(refuse(
                    "missing or unsupported scalar parameter type/default contract",
                ));
            }
            if child
                .descendants()
                .filter(|n| n.has_tag_name("parameter"))
                .any(|n| {
                    n.attribute("optional")
                        .is_some_and(|v| !matches!(v, "0" | "1"))
                        || n.attributes()
                            .any(|a| !matches!(a.name(), "usageKind" | "name" | "optional"))
                })
            {
                return Err(refuse("unknown parameter requiredness"));
            }
            let parameters = child
                .descendants()
                .filter(|n| n.has_tag_name("parameter"))
                .collect::<Vec<_>>();
            if parameters.len() > 1
                || parameters.iter().any(|p| {
                    p.attribute("name") != Some(name) || p.attribute("usageKind") != Some(tag)
                })
            {
                return Err(refuse(
                    "conflicting scalar parameter identity/direction metadata",
                ));
            }
            if parameters
                .iter()
                .any(|p| p.attribute("optional") == Some("1"))
            {
                return Err(refuse(
                    "optional/defaulted signature needs declared type adaptation; unsupported before scalar construction",
                ));
            }
        }
    }
    if output_names.len() > 1 {
        return Err(refuse(
            "multiple scalar outputs require unsupported inline signature adaptation",
        ));
    }
    for node in structure
        .children()
        .filter(|n| n.has_tag_name("graph"))
        .flat_map(|n| n.descendants())
    {
        if node.is_element()
            && !matches!(
                node.tag_name().name(),
                "graph" | "vertices" | "vertex" | "edges" | "edge"
            )
        {
            return Err(refuse("unsupported imported graph vocabulary/context"));
        }
        if node.is_element()
            && node.attributes().any(|a| {
                !matches!(
                    (node.tag_name().name(), a.name()),
                    ("graph", "directed") | ("vertex" | "edge", "vertexkey")
                )
            })
        {
            return Err(refuse("unsupported imported graph connection metadata"));
        }
        if node.has_tag_name("graph") && node.attribute("directed").is_some_and(|v| v != "1")
            || (node.has_tag_name("vertex") || node.has_tag_name("edge"))
                && node.attribute("vertexkey").is_none()
        {
            return Err(refuse(
                "missing or unsupported imported graph direction/key",
            ));
        }
        if let Some(key) = node.attribute("vertexkey") {
            if super::parse_u32(Some(key)).is_none_or(|key| !pins.contains(&key)) {
                return Err(refuse("graph references a port outside this definition"));
            }
        }
    }
    if component
        .children()
        .chain(structure.children())
        .any(|n| n.has_tag_name("connections"))
    {
        return Err(refuse(
            "legacy imported connection ownership is unsupported",
        ));
    }
    let feeds = super::read_edges(structure, Some(&component));
    for input in children
        .children()
        .filter(|n| n.has_tag_name("component") && n.attribute("kind") == Some("6"))
    {
        if supported_default_port(input)?.is_some_and(|port| feeds.contains_key(&port)) {
            return Err(refuse(
                "connected typed default requires inline input/output adaptation; unsupported before scalar construction",
            ));
        }
    }
    Ok(())
}

fn supported_default_port(input: roxmltree::Node<'_, '_>) -> Result<Option<u32>, String> {
    let mut sources = input.children().filter(|n| n.has_tag_name("sources"));
    let Some(source) = sources.next() else {
        return Ok(None);
    };
    if sources.next().is_some() {
        return Err(
            "duplicate default source containers are ambiguous; no alternate container selection"
                .into(),
        );
    }
    let mut points = source.children().filter(|n| n.is_element());
    let Some(point) = points.next() else {
        return Ok(None);
    };
    if !point.has_tag_name("datapoint")
        || points.next().is_some()
        || point.attribute("pos").is_some_and(|p| p != "0")
    {
        return Err("imported parameter default supports only a single position-0 slot; ambiguous/default slot selection refused".into());
    }
    // With this exact shape the raw point is precisely read_function's
    // inputs.first().flatten(), never 'any connected source'.
    match point.attribute("key") {
        Some(key) => super::parse_u32(Some(key))
            .map(Some)
            .ok_or_else(|| "invalid default-slot key".into()),
        None => Ok(None),
    }
}

pub(super) fn required_parameters(
    component: roxmltree::Node<'_, '_>,
) -> Result<BTreeSet<u32>, String> {
    let Some(structure) = component.children().find(|n| n.has_tag_name("structure")) else {
        return Ok(BTreeSet::new());
    };
    let feeds = super::read_edges(&structure, Some(&component));
    let mut required = BTreeSet::new();
    for input in structure
        .children()
        .find(|n| n.has_tag_name("children"))
        .into_iter()
        .flat_map(|n| n.children())
        .filter(|n| {
            n.has_tag_name("component")
                && n.attribute("library") == Some("core")
                && n.attribute("kind") == Some("6")
        })
        .filter(|n| {
            !n.descendants()
                .any(|p| p.has_tag_name("parameter") && p.attribute("optional") == Some("1"))
        })
    {
        if supported_default_port(input)?.is_none_or(|key| !feeds.contains_key(&key)) {
            if let Some(id) = super::parse_u32(input.attribute("uid")) {
                required.insert(id);
            }
        }
    }
    Ok(required)
}

/// v2 refuses unproved consumption roles rather than moving a fallible eager
/// call beneath an inline/default/failure/filename/root-ABI expression.
pub(super) fn validate_call(
    component: &roxmltree::Node<'_, '_>,
    budget: &limits::ConstructionBudget,
) -> Result<(), String> {
    let refuse = |reason: &str| format!("imported call consumption refused: {reason}");
    if component.attribute("kind") != Some("19") {
        return Err(refuse(
            "only the exact saved kind-19 call contract is supported",
        ));
    }
    let structure = component
        .ancestors()
        .find(|n| n.has_tag_name("structure"))
        .ok_or_else(|| refuse("no owning graph"))?;
    let owner = structure
        .parent()
        .ok_or_else(|| refuse("no owning mapping"))?;
    if owner.attribute("library").is_some_and(|l| !l.is_empty())
        || owner.parent().is_none_or(|n| !n.has_tag_name("mapping"))
    {
        return Err(refuse(
            "nested regular-call boundary/type adaptation is unsupported; no flattening or unused-argument pruning",
        ));
    }
    // Includes two owning-graph feed censuses plus call-port vectors/maps and
    // materializer argument slots, reserved before either parser allocates.
    budget.claim(
        512usize.saturating_add(structure.descendants().count().saturating_mul(8)),
        limits::decoded_bytes(*component).saturating_mul(8),
        0,
    )?;
    if component.tag_name().namespace().is_some()
        || component.attributes().any(|a| {
            a.namespace().is_some() || !matches!(a.name(), "name" | "library" | "kind" | "uid")
        })
    {
        return Err(refuse(
            "unproved call-site inline/context/ownership attributes",
        ));
    }
    for node in component
        .descendants()
        .filter(|n| n.is_element() && *n != *component)
    {
        let attributes: &[&str] = match node.tag_name().name() {
            "data" => &[],
            "root" => &["rootindex"],
            "entry" => &["name", "componentid", "inpkey", "outkey"],
            "properties" if !node.children().any(|n| n.is_element()) => &[],
            _ => return Err(refuse("unproved scalar call-site metadata/namespace")),
        };
        if node.tag_name().namespace().is_some()
            || node
                .attributes()
                .any(|a| a.namespace().is_some() || !attributes.contains(&a.name()))
        {
            return Err(refuse(
                "unproved scalar call-site metadata/namespace attributes",
            ));
        }
    }
    let children = structure
        .children()
        .find(|n| n.has_tag_name("children"))
        .ok_or_else(|| refuse("no owning component list"))?;
    if children
        .children()
        .filter(|n| n.has_tag_name("component"))
        .any(|n| {
            n.attribute("library") == Some("core") && matches!(n.attribute("kind"), Some("6" | "7"))
                || n.descendants().any(|p| {
                    p.has_tag_name("properties") && p.attribute("UsageKind").is_some()
                        || p.has_tag_name("parameter") && p.attribute("usageKind").is_some()
                })
        })
    {
        return Err(refuse(
            "RootFunctionABI parameter consumption has no external admission proof",
        ));
    }
    for entry in component.descendants().filter(|n| n.has_tag_name("entry")) {
        if entry.parent().is_none_or(|n| !n.has_tag_name("root"))
            || entry.children().any(|n| n.is_element())
            || entry
                .attribute("name")
                .is_none_or(|n| n.len() > MAX_SYMBOL_BYTES)
        {
            return Err(refuse("only bounded flat scalar call ports are supported"));
        }
    }
    let feeds = super::read_edges(&structure, Some(&owner));
    consumers::validate(component, children, &feeds).map_err(|reason| refuse(reason.message()))?;
    Ok(())
}

fn ordinary_content_entry(entry: roxmltree::Node<'_, '_>) -> bool {
    let Some(data) = entry.ancestors().find(|n| n.has_tag_name("data")) else {
        return false;
    };
    if data.children().filter(|n| n.has_tag_name("root")).count() != 1 {
        return false;
    }
    let Some(saved_root) = data.children().find(|n| n.has_tag_name("root")) else {
        return false;
    };
    if saved_root
        .attribute("rootindex")
        .is_some_and(|index| index != "0")
    {
        return false;
    }
    if let Some(header) = saved_root.children().find(|n| n.has_tag_name("header")) {
        let zero = header
            .children()
            .find(|n| n.has_tag_name("namespaces"))
            .and_then(|namespaces| namespaces.children().find(|n| n.has_tag_name("namespace")));
        if zero.is_none_or(|namespace| {
            namespace
                .attribute("uid")
                .is_some_and(|uri| !uri.is_empty())
        }) {
            return false;
        }
    }
    let mut documents = data.children().filter(|n| n.has_tag_name("document"));
    let Some(document) = documents.next() else {
        return false;
    };
    let Some(root_name) = document
        .attribute("instanceroot")
        .and_then(|name| name.strip_prefix("{}"))
    else {
        return false;
    };
    if documents.next().is_some()
        || root_name.is_empty()
        || document.attribute("schema").is_none_or(str::is_empty)
        || document
            .attribute("inputinstance")
            .is_some_and(|name| !name.is_empty())
    {
        return false;
    }
    let Some(content_root) = entry.ancestors().find(|n| {
        n.has_tag_name("entry")
            && n.attribute("name") == Some(root_name)
            && matches!(n.attribute("ns"), None | Some("0"))
    }) else {
        return false;
    };
    if content_root == entry {
        return false;
    } // not a root/protocol socket
    let Some(parent) = content_root.parent() else {
        return false;
    };
    if parent.has_tag_name("root") {
        return parent.parent() == Some(data);
    }
    // The existing exporter wraps content under FileInstance/document ns=1.
    // These protocol entries themselves are never scalar target consumers.
    let Some(file) = parent.parent() else {
        return false;
    };
    let Some(root) = file
        .parent()
        .filter(|root| root.has_tag_name("root") && root.parent() == Some(data))
    else {
        return false;
    };
    let namespace = root
        .children()
        .find(|node| node.has_tag_name("header"))
        .and_then(|header| {
            header
                .children()
                .find(|node| node.has_tag_name("namespaces"))
        })
        .and_then(|namespaces| {
            namespaces
                .children()
                .filter(|node| node.has_tag_name("namespace"))
                .nth(1)
        });
    parent.has_tag_name("entry")
        && parent.attribute("name") == Some("document")
        && parent.attribute("ns") == Some("1")
        && file.has_tag_name("entry")
        && file.attribute("name") == Some("FileInstance")
        && file.attribute("ns") == Some("1")
        && namespace.and_then(|node| node.attribute("uid"))
            == Some("http://www.altova.com/mapforce")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::udf::{OutputExpr, ScalarExpr};
    use ir::Value;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "ferrule_library_scope_{}_{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn write(&self, name: &str, text: &str) {
            std::fs::write(self.0.join(name), text).unwrap();
        }
        fn read(&self, xml: &str) -> (Registry, Vec<String>) {
            self.write("main.mfd", xml);
            let resources = ResourceResolver::new(&self.0.join("main.mfd"), Some(&self.0)).unwrap();
            let document = roxmltree::Document::parse(xml).unwrap();
            let mut warnings = Vec::new();
            let registry =
                Registry::read_in_package(&document.root_element(), &resources, &mut warnings);
            (registry, warnings)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn mapping(imports: &[&str], declarations: &str) -> String {
        format!(
            "<mapping><settings><libraries>{}</libraries></settings>{declarations}</mapping>",
            imports
                .iter()
                .map(|p| format!(r#"<import path="{p}" saverelative="1"/>"#))
                .collect::<String>()
        )
    }
    fn constant(library: &str, name: &str, value: &str) -> String {
        format!(
            r#"<component library="{library}" name="{name}"><structure><children>
        <component library="core" kind="2" uid="1" name="constant"><targets><datapoint key="10"/></targets><data><constant datatype="string" value="{value}"/></data></component>
        <component library="core" kind="7" uid="2" name="result"><sources><datapoint key="30"/></sources><data><output datatype="string"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex></vertices></graph></structure></component>"#
        )
    }
    fn forward(name: &str) -> String {
        format!(
            r#"<component library="helpers" name="{name}"><structure><children>
        <component library="helpers" kind="19" uid="1" name="Leaf"><data><root rootindex="1"><entry name="result" componentid="2" outkey="10"/></root></data></component>
        <component library="core" kind="7" uid="2" name="result"><sources><datapoint key="30"/></sources><data><output datatype="string"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex></vertices></graph></structure></component>"#
        )
    }
    fn value(registry: &Registry, library: &str, name: &str) -> Option<String> {
        let OutputExpr::Scalar(ScalarExpr::Const(Value::String(value))) =
            registry.definition_named(library, name)?.outputs.get(&2)?
        else {
            return None;
        };
        Some(value.clone())
    }

    #[test]
    #[ignore = "v2 pending capability: nested typed/eager call boundaries are explicitly refused; retain the stronger v1 positive oracle"]
    fn transitive_calls_resolve_in_owner_scope_and_do_not_reexport() {
        let dir = Directory::new();
        dir.write(
            "leaf.mfd",
            &mapping(&[], &constant("helpers", "Leaf", "right")),
        );
        dir.write("outer.mfd", &mapping(&["leaf.mfd"], &forward("Outer")));
        let (registry, warnings) = dir.read(&mapping(
            &["outer.mfd"],
            &constant("helpers", "Leaf", "wrong"),
        ));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            value(&registry, "helpers", "Outer").as_deref(),
            Some("right")
        );
        assert_eq!(
            value(&registry, "helpers", "Leaf").as_deref(),
            Some("wrong")
        );
        let origin = &registry.origins[&registry.supported("helpers", "Outer").unwrap()];
        assert_eq!(origin.dependencies.len(), 1);
        assert_eq!(origin.dependencies[0].name, "Leaf");

        let (registry, _) = dir.read(&mapping(&["outer.mfd"], ""));
        assert!(registry.supported("helpers", "Leaf").is_none());
        dir.write("outer.mfd", &mapping(&[], &forward("Outer")));
        let (registry, _) = dir.read(&mapping(
            &["outer.mfd"],
            &constant("helpers", "Leaf", "wrong"),
        ));
        assert!(registry.supported("helpers", "Outer").is_none());
        assert!(
            registry
                .unsupported_reason("helpers", "Outer")
                .unwrap()
                .contains("missing nested")
        );
    }

    #[test]
    fn document_and_definition_ids_are_not_a_global_namespace() {
        let dir = Directory::new();
        dir.write("a.mfd", &mapping(&[], &constant("alpha", "Same", "a")));
        dir.write("b.mfd", &mapping(&[], &constant("beta", "Same", "b")));
        let (registry, warnings) = dir.read(&mapping(
            &["a.mfd", "b.mfd"],
            &constant("local", "Same", "local"),
        ));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(value(&registry, "alpha", "Same").as_deref(), Some("a"));
        assert_eq!(value(&registry, "beta", "Same").as_deref(), Some("b"));
        assert_eq!(value(&registry, "local", "Same").as_deref(), Some("local"));
    }

    #[test]
    fn conflicting_exact_imports_and_declarations_never_choose_a_winner() {
        let dir = Directory::new();
        dir.write("a.mfd", &mapping(&[], &constant("helpers", "Same", "a")));
        dir.write("b.mfd", &mapping(&[], &constant("helpers", "Same", "b")));
        let (registry, _) = dir.read(&mapping(&["a.mfd", "b.mfd"], ""));
        assert!(registry.supported("helpers", "Same").is_none());
        assert!(
            registry
                .unsupported_reason("helpers", "Same")
                .unwrap()
                .contains("ambiguous")
        );
        // Documented local-definition precedence is distinct from import order.
        let (registry, _) = dir.read(&mapping(
            &["a.mfd", "b.mfd"],
            &constant("helpers", "Same", "local"),
        ));
        assert_eq!(
            value(&registry, "helpers", "Same").as_deref(),
            Some("local")
        );
        dir.write(
            "a.mfd",
            &mapping(
                &[],
                &(constant("helpers", "Same", "a") + &constant("helpers", "Same", "b")),
            ),
        );
        let (registry, _) = dir.read(&mapping(&["a.mfd"], ""));
        assert!(registry.supported("helpers", "Same").is_none());
    }

    #[test]
    fn namespace_unknown_dead_component_and_missing_type_are_explicit_refusals() {
        for body in [
            constant("helpers", "Same", "ok").replace("<output datatype=\"string\"/>", "<output/>"),
            constant("helpers", "Same", "ok")
                .replace("<structure>", "<structure xmlns=\"urn:unproved\">"),
            constant("helpers", "Same", "ok").replace(
                "</children>",
                "<component library=\"custom\" name=\"unused\" kind=\"5\" uid=\"3\"/></children>",
            ),
        ] {
            let dir = Directory::new();
            dir.write("a.mfd", &mapping(&[], &body));
            let (registry, _) = dir.read(&mapping(&["a.mfd"], ""));
            assert!(registry.supported("helpers", "Same").is_none());
            assert!(
                registry
                    .unsupported_reason("helpers", "Same")
                    .unwrap()
                    .contains("refused")
            );
        }
    }

    #[test]
    fn metadata_aliases_and_application_global_imports_are_not_invented() {
        for text in [
            r#"<mapping><settings><libraries><import path="a.mfd" saverelative="1" alias="made-up"/></libraries></settings></mapping>"#,
            r#"<mapping><settings><libraries><import path="a.mfd" saverelative="0"/></libraries></settings></mapping>"#,
        ] {
            let document = roxmltree::Document::parse(text).unwrap();
            assert!(imports(document.root_element()).is_err());
        }
    }

    #[test]
    fn outer_byte_and_file_budgets_apply_before_delivery_or_another_file_read() {
        let dir = Directory::new();
        let text = "<mapping/>";
        dir.write("main.mfd", text);
        dir.write("a.mfd", text);
        let resources = ResourceResolver::new(&dir.0.join("main.mfd"), Some(&dir.0)).unwrap();
        let digest = Sha256::digest(text.as_bytes()).into();
        let reader = resources.mapping_library("a.mfd", digest).unwrap();
        assert_eq!(
            reader
                .bytes_bounded(reader.path(), text.len())
                .unwrap()
                .as_ref(),
            text.as_bytes()
        );
        assert!(reader.bytes_bounded(reader.path(), text.len() - 1).is_err());
        let live = resources.mapping_library("a.mfd", digest).unwrap();
        assert!(live.bytes_bounded(live.path(), text.len() - 1).is_err());
        let reader = resources.mapping_library("a.mfd", digest).unwrap();
        let mut budget = Budget::default();
        for index in 0..MAX_FILES {
            budget
                .files
                .insert(PathBuf::from(format!("unread-{index}")), [0; 32]);
        }
        let error = read_document(
            reader.path(),
            &reader,
            resources.mapping_path(),
            digest,
            "a.mfd",
            &resources,
            &mut budget,
            &mut Vec::new(),
        )
        .err()
        .unwrap();
        assert!(error.contains("before resource read"));
    }

    fn identity_with_default_slots(slots: &str) -> String {
        format!(
            r#"<component library="helpers" name="Identity"><structure><children>
        <component library="core" kind="6" uid="1" name="value"><sources>{slots}</sources><targets><datapoint pos="0" key="10"/></targets><data><input datatype="string"/><parameter usageKind="input" name="value"/></data></component>
        <component library="core" kind="7" uid="2" name="result"><sources><datapoint pos="0" key="30"/></sources><data><output datatype="string"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex></vertices></graph></structure></component>"#
        )
    }

    #[test]
    fn default_slot_selection_and_parameter_output_positions_never_rebase() {
        for slots in ["", r#"<datapoint pos="0" key="20"/>"#] {
            let xml = identity_with_default_slots(slots);
            let document = roxmltree::Document::parse(&xml).unwrap();
            validate_definition(document.root_element()).unwrap();
            assert_eq!(
                required_parameters(document.root_element()).unwrap(),
                BTreeSet::from([1])
            );
        }
        for position in [1, 63, 64] {
            let xml =
                identity_with_default_slots(&format!(r#"<datapoint pos="{position}" key="20"/>"#));
            let document = roxmltree::Document::parse(&xml).unwrap();
            assert!(validate_definition(document.root_element()).is_err());
            assert!(required_parameters(document.root_element()).is_err());
            let xml = constant("helpers", "Output", "x").replace(
                "<datapoint key=\"30\"/>",
                &format!("<datapoint pos=\"{position}\" key=\"30\"/>"),
            );
            assert!(
                validate_definition(roxmltree::Document::parse(&xml).unwrap().root_element())
                    .is_err()
            );
        }
        let xml = identity_with_default_slots(
            r#"<datapoint pos="0" key="20"/><datapoint pos="1" key="21"/>"#,
        );
        let document = roxmltree::Document::parse(&xml).unwrap();
        assert!(validate_definition(document.root_element()).is_err());
        assert!(required_parameters(document.root_element()).is_err());
    }

    #[test]
    fn ordinary_argument_63_is_sparse_64_is_refused_before_reader_fallback() {
        for position in [63, 64] {
            let xml = constant("helpers", "Arguments", "x")
                .replace("</children>", &format!(r#"<component library="core" kind="5" uid="3" name="concat"><sources><datapoint pos="{position}" key="20"/></sources><targets><datapoint key="21"/></targets></component></children>"#))
                .replace("<edge vertexkey=\"30\"/>", "<edge vertexkey=\"20\"/>")
                .replace("</vertices>", "<vertex vertexkey=\"21\"><edges><edge vertexkey=\"30\"/></edges></vertex></vertices>");
            let document = roxmltree::Document::parse(&xml).unwrap();
            if position == 63 {
                validate_definition(document.root_element()).unwrap();
                let component = document
                    .descendants()
                    .find(|n| n.attribute("name") == Some("concat"))
                    .unwrap();
                let function = super::super::super::function::read(&component);
                assert_eq!(function.inputs.len(), 64);
                assert_eq!(function.inputs[0], None);
                assert_eq!(function.inputs[63], Some(20));
            } else {
                assert!(
                    validate_definition(document.root_element())
                        .unwrap_err()
                        .contains("64-slot")
                );
            }
            let output = xml.replace(
                "<datapoint key=\"21\"/>",
                &format!("<datapoint pos=\"{position}\" key=\"21\"/>"),
            );
            assert!(
                validate_definition(roxmltree::Document::parse(&output).unwrap().root_element())
                    .is_err()
            );
        }
    }

    #[test]
    fn nested_typed_and_failing_unused_argument_bodies_are_explicitly_refused() {
        let dir = Directory::new();
        // The regular callee has an unused *typed* argument. Its connected
        // parse-date producer must fail eagerly, not vanish during flattening.
        let leaf = constant("helpers", "Leaf", "unused-argument")
            .replace("</children>", r#"<component library="core" kind="6" uid="100" name="unused"><targets><datapoint key="100"/></targets><data><input datatype="integer"/><parameter usageKind="input" name="unused"/></data></component></children>"#);
        let outer = forward("Outer")
            .replace(r#"<root rootindex="1">"#, r#"<root><entry name="unused" componentid="100" inpkey="45"/></root><root rootindex="1">"#)
            .replace("</children>", r#"<component library="core" kind="2" uid="3" name="bad"><targets><datapoint key="40"/></targets><data><constant datatype="string" value="not-a-date"/></data></component><component library="core" kind="2" uid="4" name="picture"><targets><datapoint key="41"/></targets><data><constant datatype="string" value="[Y]-[M]-[D]"/></data></component><component library="core" kind="5" uid="5" name="parse-date"><sources><datapoint pos="0" key="42"/><datapoint pos="1" key="43"/></sources><targets><datapoint key="44"/></targets></component></children>"#)
            .replace("</vertices>", r#"<vertex vertexkey="40"><edges><edge vertexkey="42"/></edges></vertex><vertex vertexkey="41"><edges><edge vertexkey="43"/></edges></vertex><vertex vertexkey="44"><edges><edge vertexkey="45"/></edges></vertex></vertices>"#);
        dir.write("outer.mfd", &mapping(&[], &(leaf + &outer)));
        let (registry, warnings) = dir.read(&mapping(
            &["outer.mfd"],
            &constant("helpers", "Leaf", "must-not-be-borrowed"),
        ));
        assert!(registry.supported("helpers", "Outer").is_none());
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("eager regular-call consumption boundary")),
            "{warnings:?}"
        );
        for owner in ["webservice", "wsdl"] {
            let xml = constant(owner, "NotHttp", "x");
            assert!(
                validate_definition(roxmltree::Document::parse(&xml).unwrap().root_element())
                    .unwrap_err()
                    .contains("reserved")
            );
        }
    }

    #[test]
    fn unproved_root_abi_failure_filename_and_nested_roles_are_refused() {
        for role in ["target", "root-abi", "failure", "filename", "nested"] {
            let owner = if role == "nested" {
                " library=\"wrapper\""
            } else {
                ""
            };
            let consumer = match role {
                "failure" => {
                    r#"<component library="core" name="exception" kind="40"><sources><datapoint key="20"/></sources></component>"#
                }
                "filename" => {
                    r#"<component library="xml" name="Document" kind="14"><sources><datapoint key="20"/></sources><data><root><entry name="Result"/></root></data></component>"#
                }
                _ => {
                    r#"<component library="xml" name="Document" kind="14"><data><root><entry name="Result"><entry name="Value" inpkey="20"/></entry></root><document schema="generic.xsd" instanceroot="{}Result"/></data></component>"#
                }
            };
            let root = if role == "root-abi" {
                r#"<component library="core" name="root-input" kind="6"/>"#
            } else {
                ""
            };
            let xml = format!(
                r#"<mapping><component name="main"{owner}><structure><children><component library="helpers" kind="19" name="Leaf"><data><root rootindex="1"><entry name="result" componentid="2" outkey="10"/></root></data></component>{consumer}{root}</children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component></mapping>"#
            );
            let document = roxmltree::Document::parse(&xml).unwrap();
            let call = document
                .descendants()
                .find(|n| n.attribute("kind") == Some("19"))
                .unwrap();
            let result = validate_call(&call, &limits::ConstructionBudget::default());
            if role == "target" {
                result.unwrap();
            } else {
                assert!(result.unwrap_err().contains("consumption refused"));
            }
        }
    }

    #[test]
    fn file_protocol_namespace_and_root_are_not_target_field_aliases() {
        let xml = r#"<data><root><header><namespaces><namespace/><namespace uid="http://www.altova.com/mapforce"/></namespaces></header><entry name="FileInstance" ns="1" inpkey="1"><entry name="document" ns="1"><entry name="Result"><entry name="Value" inpkey="2"/></entry></entry></entry></root><document schema="generic.xsd" instanceroot="{}Result"/></data>"#;
        let document = roxmltree::Document::parse(xml).unwrap();
        let file = document
            .descendants()
            .find(|n| n.attribute("inpkey") == Some("1"))
            .unwrap();
        let value = document
            .descendants()
            .find(|n| n.attribute("inpkey") == Some("2"))
            .unwrap();
        assert!(!ordinary_content_entry(file));
        assert!(ordinary_content_entry(value));
        let collision = xml.replace("http://www.altova.com/mapforce", "urn:unproved-owner");
        let document = roxmltree::Document::parse(&collision).unwrap();
        assert!(!ordinary_content_entry(
            document
                .descendants()
                .find(|n| n.attribute("inpkey") == Some("2"))
                .unwrap()
        ));
    }
}
