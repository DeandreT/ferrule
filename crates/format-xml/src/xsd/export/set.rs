use std::collections::BTreeMap;
use std::path::{Component, Path};

use ir::{SchemaKind, SchemaNode, XmlNamespace};

use crate::XmlFormatError;

use super::{
    ExternalReference, alternatives, attribute_value_constraint, export_document, export_namespace,
    xsd_type_name,
};

const MAX_NAMESPACE_ARTIFACTS: usize = 64;
const MAX_NAMESPACE_REFERENCES: usize = 4_096;

/// One generated dependency of a multi-file XSD export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XsdExportArtifact {
    pub filename: String,
    pub contents: String,
}

/// A root schema plus every local schema it imports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XsdExportSet {
    pub namespace: Option<String>,
    pub root: String,
    pub dependencies: Vec<XsdExportArtifact>,
}

/// Exports a schema as one traversal-safe local XSD graph.
///
/// Foreign expanded names become global declarations in deterministic sibling
/// files. The owning schema imports those files and refers to their expanded
/// names, preserving namespace identity without embedding namespace text in a
/// filesystem path.
pub fn export_set(
    schema: &SchemaNode,
    root_filename: &str,
) -> Result<XsdExportSet, XmlFormatError> {
    let stem = artifact_stem(root_filename)?;
    let mut planner = ExportSetPlanner::new(stem, schema);
    let target_namespace = match &schema.xml_namespace {
        Some(XmlNamespace::Qualified(namespace)) => Some(namespace.as_str().to_string()),
        _ => export_namespace(schema)?,
    };
    let mut active = root_key(schema, target_namespace.as_deref())
        .into_iter()
        .collect();
    let root_imports = planner.scan_document(schema, target_namespace.as_deref(), &mut active)?;
    let root_references = planner.references(&root_imports);
    let root = export_document(schema, &root_references)?;

    let mut dependencies = Vec::with_capacity(planner.dependencies.len());
    for dependency in &planner.dependencies {
        let references = planner.references(&dependency.imports);
        let contents = if dependency.key.attribute {
            export_attribute_document(&dependency.declaration, &dependency.key)?
        } else {
            export_document(&dependency.declaration, &references)?
        };
        dependencies.push(XsdExportArtifact {
            filename: dependency.filename.clone(),
            contents,
        });
    }
    Ok(XsdExportSet {
        namespace: target_namespace,
        root,
        dependencies,
    })
}

fn artifact_stem(filename: &str) -> Result<&str, XmlFormatError> {
    let path = Path::new(filename);
    let valid_component = matches!(
        path.components().collect::<Vec<_>>().as_slice(),
        [Component::Normal(_)]
    );
    let portable = !filename.is_empty()
        && filename.len() <= 255
        && !filename.contains(['/', '\\', ':', '\0'])
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("xsd"));
    let stem = path.file_stem().and_then(|stem| stem.to_str());
    if !valid_component || !portable || stem.is_none_or(str::is_empty) {
        return Err(XmlFormatError::InvalidXsdArtifactName {
            name: filename.to_string(),
        });
    }
    Ok(stem.unwrap_or_default())
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DeclarationKey {
    attribute: bool,
    namespace: String,
    name: String,
}

impl DeclarationKey {
    fn new(node: &SchemaNode, namespace: &str) -> Self {
        Self {
            attribute: node.attribute,
            namespace: namespace.to_string(),
            name: node.name.clone(),
        }
    }

    fn role(&self) -> &'static str {
        if self.attribute {
            "attribute"
        } else {
            "element"
        }
    }
}

struct DependencyPlan {
    key: DeclarationKey,
    declaration: SchemaNode,
    filename: String,
    imports: Vec<DeclarationKey>,
}

struct ExportSetPlanner<'a> {
    stem: String,
    root_schema: &'a SchemaNode,
    dependencies: Vec<DependencyPlan>,
    by_key: BTreeMap<DeclarationKey, usize>,
    references: usize,
}

impl<'a> ExportSetPlanner<'a> {
    fn new(stem: &str, root_schema: &'a SchemaNode) -> Self {
        Self {
            stem: stem.to_string(),
            root_schema,
            dependencies: Vec::new(),
            by_key: BTreeMap::new(),
            references: 0,
        }
    }

    fn scan_document(
        &mut self,
        schema: &SchemaNode,
        target_namespace: Option<&str>,
        active: &mut Vec<DeclarationKey>,
    ) -> Result<Vec<DeclarationKey>, XmlFormatError> {
        let mut imports = Vec::new();
        self.scan_node(schema, target_namespace, active, &mut imports, true)?;
        Ok(imports)
    }

    fn scan_node(
        &mut self,
        node: &SchemaNode,
        target_namespace: Option<&str>,
        active: &mut Vec<DeclarationKey>,
        imports: &mut Vec<DeclarationKey>,
        document_root: bool,
    ) -> Result<(), XmlFormatError> {
        if !document_root
            && let Some(XmlNamespace::Qualified(namespace)) = &node.xml_namespace
            && Some(namespace.as_str()) != target_namespace
        {
            self.references = self.references.saturating_add(1);
            if self.references > MAX_NAMESPACE_REFERENCES {
                return Err(XmlFormatError::NamespaceReferenceLimit {
                    limit: MAX_NAMESPACE_REFERENCES,
                });
            }
            let key = DeclarationKey::new(node, namespace.as_str());
            if active.contains(&key) {
                return Err(XmlFormatError::NamespaceDependencyCycle {
                    role: key.role(),
                    namespace: key.namespace,
                    name: key.name,
                });
            }
            let declaration = self.materialize_declaration(node)?;
            if let Some(index) = self.by_key.get(&key).copied() {
                if self.dependencies[index].declaration != declaration {
                    return Err(XmlFormatError::ConflictingNamespaceDeclaration {
                        role: key.role(),
                        namespace: key.namespace,
                        name: key.name,
                    });
                }
            } else {
                if self.dependencies.len() >= MAX_NAMESPACE_ARTIFACTS {
                    return Err(XmlFormatError::NamespaceArtifactLimit {
                        limit: MAX_NAMESPACE_ARTIFACTS,
                    });
                }
                let index = self.dependencies.len();
                let filename = format!("{}-ns{}.xsd", self.stem, index + 1);
                self.by_key.insert(key.clone(), index);
                self.dependencies.push(DependencyPlan {
                    key: key.clone(),
                    declaration: declaration.clone(),
                    filename,
                    imports: Vec::new(),
                });
                active.push(key.clone());
                let nested = self.scan_document(&declaration, Some(namespace.as_str()), active)?;
                active.pop();
                self.dependencies[index].imports = nested;
            }
            if !imports.contains(&key) {
                imports.push(key);
            }
            return Ok(());
        }

        if let SchemaKind::Group { children, .. } = &node.kind {
            for child in children {
                if child.text {
                    continue;
                }
                self.scan_node(child, target_namespace, active, imports, false)?;
            }
        }
        Ok(())
    }

    fn materialize_declaration(
        &self,
        occurrence: &SchemaNode,
    ) -> Result<SchemaNode, XmlFormatError> {
        let Some(anchor) = occurrence.recursive_ref.as_deref() else {
            let mut declaration = occurrence.clone();
            declaration.repeating = false;
            return Ok(declaration);
        };
        let mut candidates = Vec::new();
        collect_concrete_anchors(self.root_schema, anchor, &mut candidates);
        let Some(candidate) = candidates.first().copied() else {
            return Err(XmlFormatError::UnsupportedRecursiveAnchor {
                node: occurrence.name.clone(),
                anchor: anchor.to_string(),
            });
        };
        if !candidates
            .iter()
            .skip(1)
            .all(|other| same_recursive_anchor_definition(candidate, other))
        {
            return Err(XmlFormatError::UnsupportedRecursiveAnchor {
                node: occurrence.name.clone(),
                anchor: anchor.to_string(),
            });
        }

        let mut declaration = candidate.clone();
        declaration.name.clone_from(&occurrence.name);
        declaration
            .xml_namespace
            .clone_from(&occurrence.xml_namespace);
        declaration.repeating = false;
        declaration.attribute = occurrence.attribute;
        declaration.text = occurrence.text;
        declaration.nillable = occurrence.nillable;
        declaration.fixed.clone_from(&occurrence.fixed);
        declaration.default.clone_from(&occurrence.default);
        declaration.value_generation = occurrence.value_generation;
        declaration.recursive_ref = None;
        rebase_recursive_anchor(&mut declaration, anchor, &occurrence.name);
        Ok(declaration)
    }

    fn references(&self, imports: &[DeclarationKey]) -> Vec<ExternalReference> {
        let mut namespaces = Vec::<&str>::new();
        imports
            .iter()
            .filter_map(|key| {
                let dependency = self
                    .by_key
                    .get(key)
                    .and_then(|index| self.dependencies.get(*index))?;
                let namespace_index = namespaces
                    .iter()
                    .position(|namespace| *namespace == key.namespace)
                    .unwrap_or_else(|| {
                        namespaces.push(&key.namespace);
                        namespaces.len() - 1
                    });
                Some(ExternalReference {
                    attribute: key.attribute,
                    namespace: key.namespace.clone(),
                    name: key.name.clone(),
                    prefix: format!("ns{}", namespace_index + 1),
                    location: dependency.filename.clone(),
                })
            })
            .collect()
    }
}

fn collect_concrete_anchors<'a>(
    node: &'a SchemaNode,
    anchor: &str,
    candidates: &mut Vec<&'a SchemaNode>,
) {
    if node.recursive_ref.is_some() {
        return;
    }
    let SchemaKind::Group { children, .. } = &node.kind else {
        return;
    };
    if node.name == anchor {
        candidates.push(node);
    }
    for child in children {
        collect_concrete_anchors(child, anchor, candidates);
    }
}

fn same_recursive_anchor_definition(left: &SchemaNode, right: &SchemaNode) -> bool {
    left.name == right.name
        && left.xml_namespace == right.xml_namespace
        && left.recursive_ref == right.recursive_ref
        && left.attribute == right.attribute
        && left.text == right.text
        && left.fixed == right.fixed
        && left.default == right.default
        && left.value_generation == right.value_generation
        && left.alternative_mode == right.alternative_mode
        && left.xml_alternative_kind == right.xml_alternative_kind
        && left.xml_repeating_sequences == right.xml_repeating_sequences
        && left.kind == right.kind
}

fn rebase_recursive_anchor(node: &mut SchemaNode, old_anchor: &str, new_anchor: &str) {
    if node.recursive_ref.as_deref() == Some(old_anchor) {
        node.recursive_ref = Some(new_anchor.to_string());
        return;
    }
    if let SchemaKind::Group { children, .. } = &mut node.kind {
        for child in children {
            rebase_recursive_anchor(child, old_anchor, new_anchor);
        }
    }
}

fn root_key(schema: &SchemaNode, target_namespace: Option<&str>) -> Option<DeclarationKey> {
    target_namespace.map(|namespace| DeclarationKey::new(schema, namespace))
}

fn export_attribute_document(
    attribute: &SchemaNode,
    key: &DeclarationKey,
) -> Result<String, XmlFormatError> {
    let SchemaKind::Scalar { ty } = attribute.kind else {
        return Err(XmlFormatError::UnsupportedSchemaRole {
            node: attribute.name.clone(),
            role: "attribute",
            kind: "group",
        });
    };
    if attribute.repeating || attribute.text || !attribute.attribute {
        return Err(XmlFormatError::RepeatingSchemaRole {
            node: attribute.name.clone(),
            role: "attribute",
        });
    }
    let value_constraint = attribute_value_constraint(attribute)
        .map_or_else(String::new, |(kind, value)| {
            format!(" {kind}=\"{}\"", alternatives::xml_escape(value))
        });
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" xmlns:tns=\"{}\" targetNamespace=\"{}\" elementFormDefault=\"unqualified\" attributeFormDefault=\"unqualified\">\n  <xs:attribute name=\"{}\" type=\"{}\"{value_constraint}/>\n</xs:schema>\n",
        alternatives::xml_escape(&key.namespace),
        alternatives::xml_escape(&key.namespace),
        alternatives::xml_escape(&attribute.name),
        xsd_type_name(&ty),
    ))
}
