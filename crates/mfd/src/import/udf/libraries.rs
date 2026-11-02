//! Document-owned declarative metadata, never a library's main mapping or code.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use sha2::{Digest, Sha256};

use super::{Definition, OutputExpr, Registry, ScalarExpr, limits};
use crate::resource::{
    BoundResource, MappingLibraryDefinitionRecordV1, MappingLibraryFunctionIdentityV1,
    ResourceResolver,
};

mod consumers;
#[cfg(test)]
mod tests;
mod validation;
pub(super) use validation::{required_parameters, validate_call, validate_definition};

type Key = (String, String);
const MAX_FILES: usize = 32;
const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_DEPTH: usize = 16;
const MAX_IMPORTS: usize = 128;
const MAX_NODES: usize = 131_072;
const MAX_DEFINITIONS: usize = 512;
const MAX_SYMBOL_BYTES: usize = 256;

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
            | "http"
            | "edi"
            | "xlsx"
            | "protobuf"
            | "pdf"
            | "xbrl"
            | "binary"
            | "edifact"
            | "IsbnConverterService"
    )
}

pub(super) fn has_mapping_imports(mapping: roxmltree::Node<'_, '_>) -> bool {
    // Unknown formats/namespace aliases still select the refusal boundary;
    // they must not restore executable-module discovery.
    mapping
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "settings")
        .flat_map(|n| {
            n.children()
                .filter(|n| n.is_element() && n.tag_name().name() == "libraries")
        })
        .any(|n| n.children().any(|n| n.is_element()))
}

struct Export {
    key: Key,
    definition: Result<Definition, String>,
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
    if !has_mapping_imports(*mapping) {
        return Ok(());
    }
    let references = imports(*mapping)?;
    let origin_hash = Sha256::digest(mapping.document().input_text().as_bytes()).into();
    let mut budget = Budget::default();
    registry.construction = Some(budget.construction.clone());
    let mut exports = Vec::new();
    for reference in references {
        let reader = resources.mapping_library(&reference, origin_hash)?;
        exports.extend(
            reader
                .finish(read_document(
                    reader.path(),
                    &reader,
                    resources.mapping_path(),
                    origin_hash,
                    &reference,
                    resources,
                    &mut budget,
                    warnings,
                ))
                .map_err(|reason| format!("`{reference}`: {reason}"))?,
        );
    }
    // Acquisition is staged: an incomplete file closure publishes no imports.
    merge(registry, exports, warnings);
    Ok(())
}

fn imports(mapping: roxmltree::Node<'_, '_>) -> Result<Vec<String>, String> {
    let mut references = Vec::new();
    let mut seen = BTreeSet::new();
    for settings in mapping
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "settings")
    {
        for libraries in settings
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "libraries")
        {
            if settings.tag_name().namespace().is_some()
                || libraries.tag_name().namespace().is_some()
                || libraries.attributes().len() != 0
            {
                return Err("namespaced/aliased mapping-library metadata is unsupported".into());
            }
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
    declaring_hash: [u8; 32],
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
    let hash = Sha256::digest(bytes.as_ref()).into();
    if budget
        .files
        .insert(path.to_path_buf(), hash)
        .is_some_and(|previous| previous != hash)
    {
        return Err("mapping-library identity delivered different immutable byte hashes".into());
    }
    budget.bytes = budget.bytes.saturating_add(bytes.len());
    let text = std::str::from_utf8(&bytes).map_err(|_| "mapping library is not UTF-8")?;
    if text.contains("<!DOCTYPE") || text.contains("<!ENTITY") {
        return Err("mapping-library DTD/entity declarations are refused".into());
    }
    let document = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: MAX_NODES.saturating_sub(budget.nodes) as u32,
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())?;
    let mapping = document.root_element();
    if !mapping.has_tag_name("mapping") || mapping.tag_name().namespace().is_some() {
        return Err("declared library root must be unnamespaced <mapping>".into());
    }
    budget.nodes += document.descendants().count();
    if budget.nodes > MAX_NODES {
        return Err("mapping-library XML node budget exceeded".into());
    }
    let declarations = || {
        mapping.children().filter(|n| {
            n.has_tag_name("component") && n.attribute("library").is_some_and(|l| !l.is_empty())
        })
    };
    let count = declarations().count();
    budget.definitions = budget.definitions.saturating_add(count);
    if budget.definitions > MAX_DEFINITIONS {
        return Err("mapping-library definition count budget exceeded".into());
    }
    budget.construction.claim(count.saturating_mul(8), 0, 0)?;
    for declaration in declarations() {
        // Registry keys, pin/interface metadata and private evidence copies are
        // reserved BEFORE any of those owned allocations. Expression builds
        // consume the same quota separately, including duplicated DAG paths.
        budget.construction.claim(
            declaration.descendants().count().saturating_mul(8),
            limits::decoded_bytes(declaration)
                .saturating_mul(16)
                .saturating_add(declaration.range().len().saturating_mul(2)),
            0,
        )?;
    }
    budget.active.push(path.to_path_buf());
    let result = (|| {
        let mut owner = Registry {
            external_owner: true,
            construction: Some(budget.construction.clone()),
            ..Default::default()
        };
        let mut dependencies = Vec::new();
        for dependency in imports(mapping)? {
            let dependency_path = dependency_path(reader.root(), path, &dependency)?;
            dependencies.extend(read_document(
                &dependency_path,
                reader,
                path,
                hash,
                &dependency,
                resources,
                budget,
                warnings,
            )?);
        }
        merge(&mut owner, dependencies, warnings);
        owner.read_declarations(&mapping, path, None, warnings);
        owner.finish_construction()?;
        budget.construction.claim(owner.definitions.len(), 0, 0)?;
        let mut definitions = std::mem::take(&mut owner.definitions)
            .into_iter()
            .map(Some)
            .collect::<Vec<_>>();
        let mut exports = Vec::new();
        for component in declarations() {
            let key = (
                component
                    .attribute("library")
                    .unwrap_or_default()
                    .to_string(),
                component.attribute("name").unwrap_or_default().to_string(),
            );
            let definition = owner
                .supported(&key.0, &key.1)
                .and_then(|index| definitions.get_mut(index).and_then(Option::take))
                .ok_or_else(|| {
                    owner
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
                declaring_sha256: declaring_hash,
                original_reference: reference.into(),
                identity: MappingLibraryFunctionIdentityV1 {
                    document: path.into(),
                    sha256: hash,
                    library: key.0.clone(),
                    name: key.1.clone(),
                },
                declaration_xml: text[component.range()].into(),
                refusal: definition.as_ref().err().cloned(),
            };
            budget.evidence_bytes = budget
                .evidence_bytes
                .saturating_add(origin.retained_bytes());
            if budget.evidence_bytes > MAX_BYTES {
                return Err("mapping-library private evidence budget exceeded".into());
            }
            resources.record_mapping_library_definition(origin)?;
            exports.push(Export { key, definition });
        }
        Ok(exports)
    })();
    budget.active.pop();
    result
}

fn merge(registry: &mut Registry, exports: Vec<Export>, warnings: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    for export in exports {
        if !seen.insert(export.key.clone()) {
            warnings.push(format!("ambiguous exact function `{}` ({}) in local mapping-library imports; no import-order winner is selected", export.key.1, export.key.0));
            registry.supported.remove(&export.key);
            registry.unsupported.insert(
                export.key,
                "ambiguous exact function in multiple local imports".into(),
            );
            continue;
        }
        match export.definition {
            Ok(definition) => {
                let index = registry.definitions.len();
                registry.definitions.push(definition);
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

pub(super) fn reserve_helper(
    definition: &Definition,
    library: &str,
    name: &str,
    budget: &limits::ConstructionBudget,
) -> Result<(), String> {
    let interface = definition
        .scalar_interface
        .as_ref()
        .ok_or("imported interface absent")?;
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
    fn reserve(
        expr: &ScalarExpr,
        budget: &limits::ConstructionBudget,
        depth: usize,
    ) -> Result<(), String> {
        budget.claim(1, 0, depth)?;
        match expr {
            ScalarExpr::Parameter(_) => Ok(()),
            ScalarExpr::Const(ir::Value::String(text)) => budget.claim(0, text.len(), depth),
            ScalarExpr::Const(
                ir::Value::Null | ir::Value::Bool(_) | ir::Value::Int(_) | ir::Value::Float(_),
            ) => Ok(()),
            ScalarExpr::Call { function, args } => {
                budget.claim(args.len(), function.len(), depth)?;
                for arg in args {
                    reserve(arg, budget, depth + 1)?;
                }
                Ok(())
            }
            ScalarExpr::If {
                condition,
                then,
                else_,
            } => {
                budget.claim(3, 0, depth)?;
                for arg in [condition, then, else_] {
                    reserve(arg, budget, depth + 1)?;
                }
                Ok(())
            }
            _ => Err("imported inline/default/runtime/sequence recipe is unsupported".into()),
        }
    }
    for output in definition.outputs.values() {
        let OutputExpr::Scalar(expression) = output else {
            return Err("imported output is not scalar".into());
        };
        reserve(expression, budget, 0)?;
    }
    Ok(())
}
