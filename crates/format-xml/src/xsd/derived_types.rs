use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ir::{SchemaKind, SchemaNode, XML_ELEMENTS_FIELD};
use roxmltree::Node;

use crate::XmlFormatError;

use super::resources::SchemaReader;
use super::schema_links::{self, Follow, LinkedSchema};
use super::type_alternative_profiles::{
    incompatible_attribute_use, incompatible_optional_occurrence,
};
use super::{
    ALTERNATIVE_VIEW_NAMESPACE, ComplexTypeResolution, ComplexTypeUse,
    MAX_MATERIALIZED_SCHEMA_ELEMENTS, MAX_TYPE_DERIVATION_DEPTH, ParseState, expanded_name,
    is_local_qname, local_name, parse_complex_type_declaration, top_level, unsupported_wildcard,
};

pub(super) fn apply_exported_alternative_view(
    el: &Node,
    node: &mut SchemaNode,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
) {
    let names = el
        .children()
        .find(|child| child.is_element() && child.tag_name().name() == "annotation")
        .and_then(|annotation| {
            annotation.children().find(|child| {
                child.is_element()
                    && child.tag_name().name() == "appinfo"
                    && child.attribute("source") == Some(ALTERNATIVE_VIEW_NAMESPACE)
            })
        })
        .into_iter()
        .flat_map(|appinfo| appinfo.children())
        .filter(|child| {
            child.is_element()
                && child.tag_name().name() == "type"
                && child.tag_name().namespace() == Some(ALTERNATIVE_VIEW_NAMESPACE)
        })
        .filter_map(|child| child.attribute("name").map(str::to_string))
        .collect::<Vec<_>>();
    if names.is_empty() {
        return;
    }
    // A single retained concrete default needs no derived declaration. Its
    // exported view can retain that typed projection without changing plain
    // named-complex-type imports which have no such view metadata.
    if node.alternatives().is_empty()
        && node.recursive_ref.is_none()
        && names.len() == 1
        && !el.children().any(|child| {
            child.is_element() && matches!(child.tag_name().name(), "complexType" | "simpleType")
        })
        && let Some(identity) = el.attribute("type").and_then(|qname| {
            if complex_type_abstractness(schema_el, schema_path, qname, state) != Some(false) {
                return None;
            }
            expanded_qname_identity(
                el,
                schema_el
                    .attribute("targetNamespace")
                    .or_else(|| state.substitutions.effective_namespace(schema_path)),
                qname,
            )
        })
        && names[0] == identity
        && let SchemaKind::Group { children, .. } = &node.kind
    {
        let members = children.iter().map(|child| child.name.clone()).collect();
        if node.set_alternatives(vec![ir::GroupAlternative {
            name: identity.clone(),
            members,
            required: Vec::new(),
            constraints: Vec::new(),
        }]) {
            node.xml_type_alternatives = true;
            node.xml_default_type = Some(identity);
        }
    }
    let SchemaKind::Group {
        children,
        alternatives,
        ..
    } = &node.kind
    else {
        return;
    };
    let selected = names
        .iter()
        .map(|name| {
            alternatives
                .iter()
                .find(|alternative| alternative.name == *name)
                .cloned()
        })
        .collect::<Option<Vec<_>>>();
    let Some(selected) = selected else {
        return;
    };
    let selected_members = selected
        .iter()
        .flat_map(|alternative| alternative.members.iter().cloned())
        .collect::<BTreeSet<_>>();
    let selected_restrictions = node
        .xml_restricted_alternatives()
        .iter()
        .filter(|name| names.contains(name))
        .cloned()
        .collect::<Vec<_>>();
    let original_default_type = node.xml_default_type.clone();
    let original_children = children.clone();
    let retained = children
        .iter()
        .filter(|child| selected_members.contains(&child.name))
        .cloned()
        .collect::<Vec<_>>();
    let SchemaKind::Group { children, .. } = &mut node.kind else {
        return;
    };
    *children = retained;
    if node.set_alternatives(selected)
        && node.set_xml_restricted_alternatives(selected_restrictions)
    {
        node.xml_type_alternatives = true;
        node.xml_default_type = original_default_type.filter(|identity| names.contains(identity));
    } else {
        // The exported view is advisory metadata; malformed external metadata
        // leaves the ordinary XSD-derived alternatives intact.
        if let SchemaKind::Group { children, .. } = &mut node.kind {
            *children = original_children;
        }
    }
}

pub(super) fn attach_type_alternatives(
    node: &mut SchemaNode,
    base_qname: &str,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
) {
    let base_identity = expanded_qname_identity(
        schema_el,
        schema_el.attribute("targetNamespace"),
        base_qname,
    );
    let Some(base_identity) = base_identity else {
        return;
    };
    let base_is_abstract = complex_type_is_abstract(schema_el, schema_path, base_qname, state);
    let mut index = DerivedTypeIndex::default();
    collect_derived_type_declarations(schema_el, schema_path, &mut index, &state.reader);
    if index.limit_reached {
        state.materialization_limit_reached = true;
        return;
    }
    let Some(derived) = index.concrete_descendants(&base_identity) else {
        return;
    };
    if derived.is_empty() {
        return;
    }

    let SchemaKind::Group {
        children: base_children,
        ..
    } = &node.kind
    else {
        return;
    };
    let base_members = base_children
        .iter()
        .map(|child| child.name.clone())
        .collect::<Vec<_>>();
    let original_children = base_children.clone();
    // One `element()` field serves every alternative, so all must resolve
    // the shared wildcard to one excluded-name set. Each alternative is
    // checked first, as every later difference refuses or returns silently.
    let base_excluded = node
        .child(XML_ELEMENTS_FIELD)
        .map(|wildcard| wildcard.xml_wildcard_excluded_names.clone());
    let mut resolved = Vec::new();
    for derived in &derived {
        let Ok(text) = state.reader.read(&derived.path) else {
            return;
        };
        let Ok(document) = roxmltree::Document::parse(&text) else {
            return;
        };
        let derived_schema = document.root_element();
        let Some(declaration) = top_level(&derived_schema, "complexType", &derived.local) else {
            return;
        };
        let Some(ComplexTypeResolution::Group(group)) = parse_complex_type_declaration(
            &declaration,
            &derived_schema,
            &derived.path,
            &derived.local,
            state,
            None,
            ComplexTypeUse::Complete,
        ) else {
            return;
        };
        if let Some(base_excluded) = &base_excluded
            && group
                .children
                .iter()
                .find(|child| child.name == XML_ELEMENTS_FIELD)
                .is_some_and(|wildcard| &wildcard.xml_wildcard_excluded_names != base_excluded)
        {
            state.reject_wildcard(unsupported_wildcard(
                "the declared type and an xsi:type alternative resolve one wildcard to different excluded names",
            ));
            return;
        }
        if !group.repeating_sequences.is_empty()
            || group.repeating_choices != node.xml_repeating_choices
        {
            state.unsupported_alternative_occurrence.get_or_insert(
                XmlFormatError::UnsupportedXmlAlternativeCompositor {
                    group: node.name.clone(),
                },
            );
            return;
        }
        resolved.push((
            derived.identity.clone(),
            derived.restriction,
            group.children,
        ));
    }
    let alternative_count = resolved.len() + usize::from(!base_is_abstract);

    let mut merged = base_children.clone();
    for (_, _, children) in &resolved {
        for child in children {
            if let Some(existing) = merged.iter().find(|existing| existing.name == child.name) {
                if let Some(field) = incompatible_attribute_use(existing, child) {
                    state.unsupported_alternative_occurrence.get_or_insert(
                        XmlFormatError::UnsupportedXmlAlternativeAttributeUse {
                            group: node.name.clone(),
                            field,
                        },
                    );
                    return;
                }
                if let Some(field) = incompatible_optional_occurrence(existing, child) {
                    state.unsupported_alternative_occurrence.get_or_insert(
                        XmlFormatError::UnsupportedXmlAlternativeOccurrence {
                            group: node.name.clone(),
                            field,
                        },
                    );
                    return;
                }
                if existing != child {
                    return;
                }
            } else {
                merged.push(child.clone());
            }
        }
    }
    let mut alternatives = Vec::with_capacity(alternative_count);
    if !base_is_abstract {
        alternatives.push(ir::GroupAlternative {
            name: base_identity.clone(),
            members: base_members,
            required: Vec::new(),
            constraints: Vec::new(),
        });
    }
    alternatives.extend(
        resolved
            .into_iter()
            .map(|(name, _, children)| ir::GroupAlternative {
                name,
                members: children.into_iter().map(|child| child.name).collect(),
                required: Vec::new(),
                constraints: Vec::new(),
            }),
    );
    if let SchemaKind::Group { children, .. } = &mut node.kind {
        *children = merged;
    }
    if node.set_alternatives(alternatives) {
        let restricted = derived
            .iter()
            .filter(|derived| derived.restriction)
            .map(|derived| derived.identity.clone())
            .collect();
        if node.set_xml_restricted_alternatives(restricted) {
            node.xml_type_alternatives = true;
            node.xml_default_type = (!base_is_abstract).then_some(base_identity);
        } else if let SchemaKind::Group { children, .. } = &mut node.kind {
            *children = original_children;
        }
    } else if let SchemaKind::Group { children, .. } = &mut node.kind {
        *children = original_children;
    }
}

#[derive(Debug, Clone)]
struct DerivedTypeDeclaration {
    path: PathBuf,
    local: String,
    identity: String,
    base_identity: String,
    abstract_type: bool,
    restriction: bool,
}

#[derive(Default)]
struct DerivedTypeIndex {
    declarations: Vec<DerivedTypeDeclaration>,
    by_identity: BTreeMap<String, usize>,
    conflicting_identity: bool,
    limit_reached: bool,
}

impl DerivedTypeIndex {
    fn insert(&mut self, declaration: DerivedTypeDeclaration) {
        if let Some(existing) = self
            .by_identity
            .get(&declaration.identity)
            .and_then(|index| self.declarations.get(*index))
        {
            if existing.path != declaration.path
                || existing.local != declaration.local
                || existing.base_identity != declaration.base_identity
                || existing.abstract_type != declaration.abstract_type
                || existing.restriction != declaration.restriction
            {
                self.conflicting_identity = true;
            }
            return;
        }
        if self.declarations.len() >= MAX_MATERIALIZED_SCHEMA_ELEMENTS {
            self.limit_reached = true;
            return;
        }
        self.by_identity
            .insert(declaration.identity.clone(), self.declarations.len());
        self.declarations.push(declaration);
    }

    fn concrete_descendants(&self, base_identity: &str) -> Option<Vec<DerivedTypeDeclaration>> {
        if self.conflicting_identity {
            return None;
        }
        let mut by_base = BTreeMap::<&str, Vec<&DerivedTypeDeclaration>>::new();
        for declaration in &self.declarations {
            by_base
                .entry(&declaration.base_identity)
                .or_default()
                .push(declaration);
        }
        let mut descendants = Vec::new();
        let mut active = BTreeSet::from([base_identity.to_string()]);
        let mut visited = BTreeSet::new();
        let mut stack = vec![(base_identity.to_string(), 0usize)];
        while let Some((identity, child_index)) = stack.last_mut() {
            let children = by_base
                .get(identity.as_str())
                .map_or(&[][..], Vec::as_slice);
            let Some(declaration) = children.get(*child_index).copied() else {
                let (completed, _) = stack.pop()?;
                active.remove(&completed);
                continue;
            };
            *child_index += 1;
            if active.contains(&declaration.identity) {
                return None;
            }
            if stack.len() >= MAX_TYPE_DERIVATION_DEPTH {
                return None;
            }
            if !visited.insert(declaration.identity.clone()) {
                continue;
            }
            if !declaration.abstract_type {
                descendants.push(declaration.clone());
            }
            active.insert(declaration.identity.clone());
            stack.push((declaration.identity.clone(), 0));
        }
        Some(descendants)
    }
}

fn complex_type_is_abstract(
    schema_el: &Node,
    schema_path: &Path,
    qname: &str,
    state: &mut ParseState,
) -> bool {
    complex_type_abstractness(schema_el, schema_path, qname, state).unwrap_or(false)
}

/// Resolves the declaration before classifying it; absence is not concrete.
fn complex_type_abstractness(
    schema_el: &Node,
    schema_path: &Path,
    qname: &str,
    state: &mut ParseState,
) -> Option<bool> {
    if qname.is_empty() || qname.chars().any(char::is_whitespace) {
        return None;
    }
    if let Some((prefix, local)) = qname.split_once(':') {
        if prefix.is_empty() || local.is_empty() || local.contains(':') {
            return None;
        }
        schema_el.lookup_namespace_uri(Some(prefix))?;
    }
    fn abstractness(declaration: &Node) -> Option<bool> {
        match declaration.attribute("abstract").map(str::trim) {
            None | Some("false" | "0") => Some(false),
            Some("true" | "1") => Some(true),
            Some(_) => None,
        }
    }
    let local = local_name(qname);
    if is_local_qname(schema_el, qname)
        && let Some(declaration) = top_level(schema_el, "complexType", local)
    {
        return abstractness(&declaration);
    }
    let path = state.find_external_declaration(schema_el, schema_path, "complexType", qname)?;
    let text = state.reader.read(&path).ok()?;
    let document = roxmltree::Document::parse(&text).ok()?;
    top_level(&document.root_element(), "complexType", local)
        .and_then(|declaration| abstractness(&declaration))
}

fn collect_derived_type_declarations(
    schema_el: &Node,
    schema_path: &Path,
    index: &mut DerivedTypeIndex,
    reader: &SchemaReader,
) {
    let mut visited = BTreeSet::new();
    let Some(root) = index_derived_type_declarations(
        schema_el,
        schema_path,
        schema_el.attribute("targetNamespace"),
        &mut visited,
        index,
        reader,
    ) else {
        return;
    };
    schema_links::walk::<()>(root, |schema, link| {
        let Some(location) = &link.location else {
            return Follow::Skip;
        };
        let Some(dependency) = reader.dependency(&schema.path, location) else {
            return Follow::Skip;
        };
        let Ok(text) = reader.read(&dependency) else {
            return Follow::Skip;
        };
        let Ok(document) = roxmltree::Document::parse(&text) else {
            return Follow::Skip;
        };
        index_derived_type_declarations(
            &document.root_element(),
            &dependency,
            schema.inherited_namespace(link),
            &mut visited,
            index,
            reader,
        )
        .map_or(Follow::Skip, Follow::Descend)
    });
}

/// Indexes one schema document's derived types, returning the document for
/// its links unless it was already visited or the index is full.
fn index_derived_type_declarations(
    schema_el: &Node,
    schema_path: &Path,
    inherited_namespace: Option<&str>,
    visited: &mut BTreeSet<(PathBuf, Option<String>)>,
    index: &mut DerivedTypeIndex,
    reader: &SchemaReader,
) -> Option<LinkedSchema> {
    if index.limit_reached {
        return None;
    }
    let path = reader.identity(schema_path);
    let effective_namespace = schema_el
        .attribute("targetNamespace")
        .or(inherited_namespace);
    if !visited.insert((path.clone(), effective_namespace.map(str::to_string))) {
        return None;
    }
    for declaration in schema_el
        .children()
        .filter(|candidate| candidate.is_element() && candidate.tag_name().name() == "complexType")
    {
        let Some(local) = declaration.attribute("name") else {
            continue;
        };
        let Some((base, restriction)) = direct_complex_derivation(&declaration) else {
            continue;
        };
        let Some(base_identity) = expanded_qname_identity(schema_el, effective_namespace, &base)
        else {
            continue;
        };
        let Some(identity) = type_identity_in_namespace(effective_namespace, local) else {
            continue;
        };
        index.insert(DerivedTypeDeclaration {
            path: path.clone(),
            local: local.to_string(),
            identity,
            base_identity,
            abstract_type: declaration
                .attribute("abstract")
                .is_some_and(|value| matches!(value, "true" | "1")),
            restriction,
        });
        if index.limit_reached {
            return None;
        }
    }
    Some(LinkedSchema::new(schema_el, path, effective_namespace))
}

pub(super) fn expanded_qname_identity(
    schema_el: &Node,
    effective_namespace: Option<&str>,
    qname: &str,
) -> Option<String> {
    if let Some((namespace, local)) = expanded_name(qname) {
        return (!namespace.is_empty() && !local.is_empty())
            .then(|| format!("{{{namespace}}}{local}"));
    }
    match qname.split_once(':') {
        Some((prefix, local)) if !prefix.is_empty() && !local.is_empty() => schema_el
            .lookup_namespace_uri(Some(prefix))
            .filter(|namespace| !namespace.is_empty())
            .map(|namespace| format!("{{{namespace}}}{local}")),
        Some(_) => None,
        None => type_identity_in_namespace(effective_namespace, qname),
    }
}

pub(super) fn type_identity_in_namespace(namespace: Option<&str>, local: &str) -> Option<String> {
    if local.is_empty() {
        return None;
    }
    Some(
        namespace
            .filter(|namespace| !namespace.is_empty())
            .map_or_else(
                || local.to_string(),
                |namespace| format!("{{{namespace}}}{local}"),
            ),
    )
}

fn direct_complex_derivation(declaration: &Node<'_, '_>) -> Option<(String, bool)> {
    let derivation = declaration
        .children()
        .find(|child| {
            child.is_element()
                && matches!(child.tag_name().name(), "complexContent" | "simpleContent")
        })?
        .children()
        .find(|child| {
            child.is_element() && matches!(child.tag_name().name(), "extension" | "restriction")
        })?;
    Some((
        derivation.attribute("base")?.to_string(),
        derivation.tag_name().name() == "restriction",
    ))
}
