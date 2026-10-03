//! Closed flat String root-view import and export.
mod project;
mod proof;
mod render;

use crate::resource::ResourceResolver;
use std::path::Path;

use std::collections::{BTreeMap, BTreeSet};

use ir::{SchemaKind, SchemaNode, Value, XmlNamespace};
use mapping::{FormatOptions, Node, Project, Scope};

use super::schema::{KeyAlloc, SideFormat, side_format};
use crate::MfdError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    Source,
    Target,
}

/// Identity owns a real conditional leaf, never a dormant display-root key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct LeafIdentity {
    role: Role,
    component_uid: u32,
    root_name: String,
    root_namespace: Option<String>,
    canonical_type: String,
    field_name: String,
    field_namespace: Option<String>,
    attribute: bool,
}

#[derive(Debug)]
struct RootTypeViewPlan {
    canonical_type: String,
    field_pairs: Vec<(LeafIdentity, LeafIdentity)>,
}

#[derive(Debug)]
struct AllocatedPlan {
    leaves: BTreeMap<LeafIdentity, u32>,
    edges: Vec<(u32, u32)>,
}

impl RootTypeViewPlan {
    fn build(
        project: &Project,
        source_uid: u32,
        target_uid: u32,
    ) -> Result<Option<Self>, MfdError> {
        if !project.source_options.xml_root_view_read_policy
            || project.source_options.xml_schema_hints.is_some()
            || project.target_options.xml_schema_hints.is_some()
            || !project.graph.nodes.values().any(|node| {
                matches!(
                    node,
                    Node::SourceRootXmlTypeEquals { .. } | Node::SourceRootField { .. }
                )
            })
        {
            return Ok(None);
        }
        let reject = || unsupported("requires one completely accounted flat XML root pair");
        if source_uid <= 1
            || target_uid <= 1
            || source_uid == target_uid
            || !project.extra_sources.is_empty()
            || !project.extra_targets.is_empty()
            || !project.failure_rules.is_empty()
            || !project.user_functions.is_empty()
            || project.graph.nodes.len() > 512
            || project.root.bindings.len() > 128
            || project.source_options
                != (FormatOptions {
                    xml_document: true,
                    xml_allow_inactive_root_type_members: true,
                    xml_root_view_read_policy: true,
                    ..FormatOptions::default()
                })
            || project.target_options != FormatOptions::default()
            || !local_xml_path(&project.source_path)
            || !local_xml_path(&project.target_path)
            || side_format(&project.source_path, &project.source_options) != SideFormat::Xml
            || side_format(&project.target_path, &project.target_options) != SideFormat::Xml
            || !flat_root(&project.root)
            || !closed_attribute_schema(&project.source)
            || !closed_attribute_schema(&project.target)
            || !engine::validate(project).is_empty()
        {
            return Err(reject());
        }
        let marker: Vec<_> = project
            .root
            .bindings
            .iter()
            .filter(|binding| binding.target_field == ir::XML_TYPE_FIELD)
            .collect();
        let [marker] = marker.as_slice() else {
            return Err(unsupported("requires one unconditional target type marker"));
        };
        let Some(Node::Const {
            value: Value::String(canonical_type),
        }) = project.graph.nodes.get(&marker.node)
        else {
            return Err(unsupported(
                "target type marker must be one canonical constant",
            ));
        };
        if !ir::primary_root_schema_has_type(&project.source, canonical_type)
            || !ir::primary_root_schema_has_type(&project.target, canonical_type)
            || project.source.xml_default_type.as_deref() == Some(canonical_type)
            || project.target.xml_default_type.as_deref() == Some(canonical_type)
            || project.source.name != project.target.name
            || namespace(&project.source.xml_namespace) != namespace(&project.target.xml_namespace)
        {
            return Err(unsupported(
                "type identity must exist on both exact root schemas",
            ));
        }
        let mut absorbed_nodes = BTreeSet::from([marker.node]);
        let mut field_pairs = Vec::new();
        let mut targets = BTreeSet::new();
        let mut owner_gate = None;
        for binding in &project.root.bindings {
            if binding.target_field == ir::XML_TYPE_FIELD {
                continue;
            }
            if !targets.insert(binding.target_field.as_str()) {
                return Err(unsupported("duplicate conditional target binding"));
            }
            let Some(Node::If {
                condition,
                then,
                else_,
            }) = project.graph.nodes.get(&binding.node)
            else {
                return Err(unsupported(
                    "every field must be one lazy root-type projection",
                ));
            };
            let Some(Node::SourceRootXmlTypeEquals {
                canonical_expanded_type,
            }) = project.graph.nodes.get(condition)
            else {
                return Err(unsupported(
                    "conditional projection lacks its exact primary owner",
                ));
            };
            if canonical_expanded_type != canonical_type
                || owner_gate.is_some_and(|owner| owner != *condition)
            {
                return Err(unsupported(
                    "all projections must share one exact type predicate",
                ));
            }
            owner_gate = Some(*condition);
            let Some(Node::SourceRootField { path, required }) = project.graph.nodes.get(then)
            else {
                return Err(unsupported(
                    "true branch must read one exact primary root field",
                ));
            };
            let [source_name] = path.as_slice() else {
                return Err(unsupported("only direct scalar root attributes are proved"));
            };
            if !matches!(
                project.graph.nodes.get(else_),
                Some(Node::Const { value: Value::Null })
            ) {
                return Err(unsupported("false branch must be absent Null"));
            }
            let source = selected_member(&project.source, canonical_type, source_name)?;
            if *required != source.xml_attribute_required {
                return Err(unsupported(
                    "primary field requirement must equal its physical source use",
                ));
            }
            let target = selected_member(&project.target, canonical_type, &binding.target_field)?;
            if source.kind != target.kind {
                return Err(unsupported("attribute wire requires equal scalar domains"));
            }
            field_pairs.push((
                leaf(
                    Role::Source,
                    source_uid,
                    &project.source,
                    canonical_type,
                    source,
                ),
                leaf(
                    Role::Target,
                    target_uid,
                    &project.target,
                    canonical_type,
                    target,
                ),
            ));
            absorbed_nodes.extend([binding.node, *condition, *then, *else_]);
        }
        if field_pairs.is_empty() || absorbed_nodes != project.graph.nodes.keys().copied().collect()
        {
            return Err(unsupported(
                "every graph use must belong to an observable root projection",
            ));
        }
        Ok(Some(Self {
            canonical_type: canonical_type.clone(),
            field_pairs,
        }))
    }

    fn allocate(&self, keys: &mut KeyAlloc) -> Result<AllocatedPlan, MfdError> {
        let distinct = self
            .field_pairs
            .iter()
            .flat_map(|(source, target)| [source, target])
            .collect::<BTreeSet<_>>()
            .len();
        let required =
            u32::try_from(distinct).map_err(|_| unsupported("has too many distinct leaf ports"))?;
        if keys.next == 0 || keys.next.checked_add(required).is_none() {
            return Err(unsupported(
                "cannot allocate all leaf ports without overflow",
            ));
        }
        let mut leaves = BTreeMap::new();
        for (source, target) in &self.field_pairs {
            for identity in [source, target] {
                leaves
                    .entry(identity.clone())
                    .or_insert_with(|| keys.next());
            }
        }
        let edges = self
            .field_pairs
            .iter()
            .map(|(source, target)| (leaves[source], leaves[target]))
            .collect();
        Ok(AllocatedPlan { leaves, edges })
    }
}

fn unsupported(reason: &str) -> MfdError {
    MfdError::Unsupported(format!("conditional XML root export {reason}"))
}

fn flat_root(root: &Scope) -> bool {
    let mut controls = root.clone();
    controls.bindings.clear();
    matches!((serde_json::to_value(controls), serde_json::to_value(Scope::default())),
        (Ok(actual), Ok(default)) if actual == default)
}

fn local_xml_path(path: &Option<String>) -> bool {
    path.as_deref().is_some_and(|path| {
        !path.is_empty()
            && path.len() <= 4096
            && !path.contains("://")
            && !path.chars().any(char::is_control)
            && std::path::Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
                == Some("xml")
    })
}

/// Reconstructing the admitted metadata makes future schema fields reject
/// conservatively instead of silently accepting an unproved wire policy.
fn closed_attribute_schema(schema: &SchemaNode) -> bool {
    if !ir::xml_root_view_read_policy_is_supported(schema)
        || !ir::primary_root_schema_is_supported(schema)
        || schema.xml_namespace.is_none()
        || schema.xml_default_type.is_none()
        || !schema.xml_default_type_is_valid()
    {
        return false;
    }
    let SchemaKind::Group {
        children,
        alternatives,
        required,
        xml_restricted_alternatives,
        dynamic,
    } = &schema.kind
    else {
        return false;
    };
    if children.len() > 32
        || !required.is_empty()
        || !xml_restricted_alternatives.is_empty()
        || dynamic.is_some()
        || alternatives
            .iter()
            .any(|alternative| !alternative.constraints.is_empty())
    {
        return false;
    }
    // A closed XSD without imports can declare only types and qualified
    // local attributes in its own target namespace.
    let root_namespace = namespace(&schema.xml_namespace);
    if alternatives
        .iter()
        .any(|alternative| type_namespace(&alternative.name) != root_namespace.as_deref())
    {
        return false;
    }
    for child in children {
        if child.xml_namespace.is_none()
            || matches!(&child.xml_namespace, Some(XmlNamespace::Qualified(uri))
                if Some(uri.as_str()) != root_namespace.as_deref())
        {
            return false;
        }
        let SchemaKind::Scalar { ty } = child.kind else {
            return false;
        };
        let mut expected = SchemaNode::scalar(&child.name, ty).attribute();
        expected.xml_namespace = child.xml_namespace.clone();
        expected.xml_attribute_required = child.xml_attribute_required;
        if &expected != child {
            return false;
        }
    }
    let mut expected = SchemaNode::group(&schema.name, children.clone());
    expected.kind = schema.kind.clone();
    expected.xml_namespace = schema.xml_namespace.clone();
    expected.xml_type_alternatives = true;
    expected.xml_default_type = schema.xml_default_type.clone();
    expected == *schema
}

fn selected_member<'a>(
    schema: &'a SchemaNode,
    identity: &str,
    name: &str,
) -> Result<&'a SchemaNode, MfdError> {
    let Some(alternative) = schema
        .alternatives()
        .iter()
        .find(|item| item.name == identity)
    else {
        return Err(unsupported("selected type is absent"));
    };
    if !alternative.members.iter().any(|member| member == name) {
        return Err(unsupported("field is outside its selected type"));
    }
    schema
        .child(name)
        .ok_or_else(|| unsupported("field has no exact schema declaration"))
}

fn type_namespace(identity: &str) -> Option<&str> {
    identity
        .strip_prefix('{')
        .and_then(|expanded| expanded.split_once('}').map(|(namespace, _)| namespace))
}

fn namespace(namespace: &Option<XmlNamespace>) -> Option<String> {
    match namespace {
        Some(XmlNamespace::Qualified(uri)) => Some(uri.as_str().to_owned()),
        Some(XmlNamespace::Unqualified) | None => None,
    }
}

fn leaf(role: Role, uid: u32, root: &SchemaNode, ty: &str, field: &SchemaNode) -> LeafIdentity {
    LeafIdentity {
        role,
        component_uid: uid,
        root_name: root.name.clone(),
        root_namespace: namespace(&root.xml_namespace),
        canonical_type: ty.to_owned(),
        field_name: field.name.clone(),
        field_namespace: namespace(&field.xml_namespace),
        attribute: field.attribute,
    }
}

/// Recognize connected root-level conditioned views with explicit base mode.
/// Ordinary, disconnected and child views remain on their existing import path.
pub(crate) fn has_qualified_envelope(structure: roxmltree::Node<'_, '_>) -> bool {
    let numeric_key = |raw: Option<&str>| {
        raw.filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|key| *key > 0)
    };
    let mut connected = BTreeSet::new();
    for graph in structure.children().filter(|n| n.has_tag_name("graph")) {
        for vertices in graph.children().filter(|n| n.has_tag_name("vertices")) {
            for vertex in vertices.children().filter(|n| n.has_tag_name("vertex")) {
                for edges in vertex.children().filter(|n| n.has_tag_name("edges")) {
                    for edge in edges.children().filter(|n| n.has_tag_name("edge")) {
                        if let (Some(a), Some(b)) = (
                            numeric_key(vertex.attribute("vertexkey")),
                            numeric_key(edge.attribute("vertexkey")),
                        ) {
                            connected.extend([a, b]);
                        }
                    }
                }
            }
        }
    }
    structure
        .children()
        .filter(|n| n.has_tag_name("children"))
        .flat_map(|n| n.children().filter(|n| n.has_tag_name("component")))
        .filter(|n| n.attribute("library") == Some("xml") && n.attribute("kind") == Some("14"))
        .any(|component| {
            let views = component
                .children()
                .filter(|n| n.has_tag_name("data"))
                .flat_map(|n| n.children().filter(|n| n.has_tag_name("root")))
                .flat_map(|n| {
                    n.children().filter(|n| {
                        n.has_tag_name("entry") && n.attribute("name") == Some("FileInstance")
                    })
                })
                .flat_map(|n| {
                    n.children().filter(|n| {
                        n.has_tag_name("entry") && n.attribute("name") == Some("document")
                    })
                })
                .flat_map(|n| n.children().filter(|n| n.has_tag_name("entry")))
                .collect::<Vec<_>>();
            let base_mode = views.iter().any(|view| {
                !view
                    .children()
                    .any(|n| n.has_tag_name("condition") || n.has_tag_name("conditions"))
                    && view.attribute("displayselectionmode").is_some()
            });
            base_mode
                && views.iter().any(|view| {
                    let type_condition = view
                        .children()
                        .filter(|n| n.has_tag_name("condition") || n.has_tag_name("conditions"))
                        .any(|condition| {
                            condition.descendants().any(|n| {
                                n.has_tag_name("attribute")
                                    && n.attribute("name") == Some("type")
                                    && n.attribute("ns")
                                        == Some("http://www.w3.org/2001/XMLSchema-instance")
                            })
                        });
                    type_condition
                        && view.descendants().any(|n| {
                            ["outkey", "inpkey"].iter().any(|kind| {
                                numeric_key(n.attribute(*kind))
                                    .is_some_and(|key| connected.contains(&key))
                            })
                        })
                })
        })
}

fn source_options() -> FormatOptions {
    FormatOptions {
        xml_document: true,
        xml_allow_inactive_root_type_members: true,
        xml_root_view_read_policy: true,
        ..FormatOptions::default()
    }
}

pub(crate) fn import_project(
    text: &str,
    structure: roxmltree::Node<'_, '_>,
    resources: &ResourceResolver,
) -> Result<Option<(Project, u32)>, MfdError> {
    if !has_qualified_envelope(structure) {
        return Ok(None);
    }
    let load = |declared: &str| -> proof::Result<proof::SchemaResource> {
        let path = resources
            .resolve_file(declared, "root profile schema")
            .map_err(|detail| proof::Rejection {
                code: "schema",
                detail,
            })?;
        let file = std::fs::File::open(&path).map_err(|e| proof::Rejection {
            code: "schema",
            detail: e.to_string(),
        })?;
        Ok(proof::SchemaResource {
            path,
            bytes: proof::bounded_schema_bytes(file)?,
        })
    };
    let proved = proof::prove_qualified(text, &load).map_err(import_error)?;
    let uid = proved.as_plan().source.uid;
    let project = project::project(&proved, &source_options()).map_err(import_error)?;
    Ok(Some((project, uid)))
}

fn import_error(error: proof::Rejection) -> MfdError {
    MfdError::UnsupportedImport(format!(
        "closed XML root profile {}: {}",
        error.code, error.detail
    ))
}

pub(super) fn prepare_export(
    project: &Project,
    path: &Path,
) -> Result<Option<super::PreparedExport>, MfdError> {
    let Some(plan) = RootTypeViewPlan::build(project, 211, 307)? else {
        return Ok(None);
    };
    let allocated = plan.allocate(&mut KeyAlloc { next: 1001 })?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| unsupported("requires an output filename"))?;
    let source_name = format!("{stem}-source.xsd");
    let target_name = format!("{stem}-target.xsd");
    let directory = path.parent().unwrap_or(Path::new("."));
    let source_xsd = format_xml::xsd::export(&project.source)
        .map_err(|e| unsupported(&format!("source schema: {e}")))?;
    let target_xsd = format_xml::xsd::export(&project.target)
        .map_err(|e| unsupported(&format!("target schema: {e}")))?;
    let xml = render::mapping_xml(project, &plan, &allocated, &source_name, &target_name);
    let load = |declared: &str| -> proof::Result<proof::SchemaResource> {
        let bytes = if declared == source_name {
            source_xsd.as_bytes()
        } else if declared == target_name {
            target_xsd.as_bytes()
        } else {
            return Err(proof::Rejection {
                code: "schema",
                detail: "unknown generated schema identity".into(),
            });
        };
        Ok(proof::SchemaResource {
            path: directory.join(declared),
            bytes: bytes.to_vec(),
        })
    };
    let proved = proof::prove_qualified(&xml, &load)
        .map_err(|e| unsupported(&format!("fresh export {}: {}", e.code, e.detail)))?;
    if proved.as_plan().source.schema != project.source
        || proved.as_plan().target.schema != project.target
    {
        return Err(unsupported("fresh export changed physical schema identity"));
    }
    crate::design::validate_export(&xml)?;
    let mut artifacts = vec![
        (directory.join(&source_name), source_xsd),
        (directory.join(&target_name), target_xsd),
    ];
    let report = super::compatibility::profile(&xml, Vec::new(), path, &artifacts)?;
    if !report.is_native_compatible() {
        return Err(unsupported("fresh profile has compatibility dependencies"));
    }
    artifacts.push((path.to_path_buf(), xml));
    Ok(Some(super::PreparedExport { artifacts, report }))
}

#[cfg(test)]
mod tests;
