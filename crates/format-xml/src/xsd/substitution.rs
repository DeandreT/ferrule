use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use roxmltree::Node;

use crate::XmlFormatError;

use super::schema_links::{self, Follow, LinkedSchema};
use super::{
    MAX_MATERIALIZED_SCHEMA_ELEMENTS, SchemaReader, expanded_qname_identity, normalized_path,
    type_identity_in_namespace,
};

#[derive(Debug, Clone)]
pub(super) struct ElementDeclaration {
    pub path: PathBuf,
    pub local: String,
    pub identity: String,
    pub namespace: Option<String>,
    pub abstract_element: bool,
}

#[derive(Debug, Clone)]
struct IndexedDeclaration {
    element: ElementDeclaration,
    head: String,
}

#[derive(Default)]
pub(super) struct SubstitutionIndex {
    declarations: Vec<IndexedDeclaration>,
    by_identity: BTreeMap<String, usize>,
    global_elements: Vec<ElementDeclaration>,
    global_by_identity: BTreeMap<String, usize>,
    global_attributes: Vec<ElementDeclaration>,
    global_attribute_by_identity: BTreeMap<String, usize>,
    effective_namespaces: BTreeMap<PathBuf, BTreeSet<Option<String>>>,
    unresolved_namespaces: BTreeSet<Option<String>>,
    redefinition: bool,
    conflicting_identity: Option<String>,
    conflicting_global_identity: Option<String>,
    conflicting_global_attribute_identity: Option<String>,
    limit_reached: bool,
    supplied_identities: bool,
}

impl SubstitutionIndex {
    fn insert_global(&mut self, declaration: ElementDeclaration) {
        if let Some(existing) = self
            .global_by_identity
            .get(&declaration.identity)
            .and_then(|index| self.global_elements.get(*index))
        {
            if existing.path != declaration.path
                || existing.local != declaration.local
                || existing.namespace != declaration.namespace
                || existing.abstract_element != declaration.abstract_element
            {
                self.conflicting_global_identity
                    .get_or_insert_with(|| declaration.identity.clone());
            }
            return;
        }
        if self.global_elements.len() >= MAX_MATERIALIZED_SCHEMA_ELEMENTS {
            self.limit_reached = true;
            return;
        }
        self.global_by_identity
            .insert(declaration.identity.clone(), self.global_elements.len());
        self.global_elements.push(declaration);
    }

    fn insert_global_attribute(&mut self, declaration: ElementDeclaration) {
        if let Some(existing) = self
            .global_attribute_by_identity
            .get(&declaration.identity)
            .and_then(|index| self.global_attributes.get(*index))
        {
            if existing.path != declaration.path
                || existing.local != declaration.local
                || existing.namespace != declaration.namespace
            {
                self.conflicting_global_attribute_identity
                    .get_or_insert_with(|| declaration.identity.clone());
            }
            return;
        }
        if self.global_attributes.len() + self.global_elements.len()
            >= MAX_MATERIALIZED_SCHEMA_ELEMENTS
        {
            self.limit_reached = true;
            return;
        }
        self.global_attribute_by_identity
            .insert(declaration.identity.clone(), self.global_attributes.len());
        self.global_attributes.push(declaration);
    }

    fn insert(&mut self, declaration: IndexedDeclaration) {
        if let Some(existing) = self
            .by_identity
            .get(&declaration.element.identity)
            .and_then(|index| self.declarations.get(*index))
        {
            if existing.element.path != declaration.element.path
                || existing.element.local != declaration.element.local
                || existing.element.namespace != declaration.element.namespace
                || existing.element.abstract_element != declaration.element.abstract_element
                || existing.head != declaration.head
            {
                self.conflicting_identity
                    .get_or_insert_with(|| declaration.element.identity.clone());
            }
            return;
        }
        if self.declarations.len() >= MAX_MATERIALIZED_SCHEMA_ELEMENTS {
            self.limit_reached = true;
            return;
        }
        self.by_identity.insert(
            declaration.element.identity.clone(),
            self.declarations.len(),
        );
        self.declarations.push(declaration);
    }

    pub(super) fn concrete_descendants(
        &self,
        head: &str,
    ) -> Result<Vec<ElementDeclaration>, XmlFormatError> {
        if self.limit_reached {
            return Err(XmlFormatError::SubstitutionGroupLimit {
                limit: MAX_MATERIALIZED_SCHEMA_ELEMENTS,
            });
        }
        if let Some(member) = &self.conflicting_identity {
            return Err(XmlFormatError::ConflictingSubstitutionMember {
                head: head.to_string(),
                member: member.clone(),
            });
        }
        let mut by_head = BTreeMap::<&str, Vec<&IndexedDeclaration>>::new();
        for declaration in &self.declarations {
            by_head
                .entry(&declaration.head)
                .or_default()
                .push(declaration);
        }

        let mut descendants = Vec::new();
        let mut active = BTreeSet::from([head.to_string()]);
        let mut visited = BTreeSet::new();
        let mut stack = vec![(head.to_string(), 0usize)];
        while let Some((identity, child_index)) = stack.last_mut() {
            let children = by_head
                .get(identity.as_str())
                .map_or(&[][..], Vec::as_slice);
            let Some(declaration) = children.get(*child_index).copied() else {
                let Some((completed, _)) = stack.pop() else {
                    break;
                };
                active.remove(&completed);
                continue;
            };
            *child_index += 1;
            if active.contains(&declaration.element.identity) {
                return Err(XmlFormatError::SubstitutionGroupCycle {
                    head: head.to_string(),
                    member: declaration.element.identity.clone(),
                });
            }
            if stack.len() >= super::MAX_TYPE_DERIVATION_DEPTH {
                return Err(XmlFormatError::SubstitutionGroupLimit {
                    limit: super::MAX_TYPE_DERIVATION_DEPTH,
                });
            }
            if !visited.insert(declaration.element.identity.clone()) {
                continue;
            }
            if !declaration.element.abstract_element {
                descendants.push(declaration.element.clone());
            }
            active.insert(declaration.element.identity.clone());
            stack.push((declaration.element.identity.clone(), 0));
        }
        Ok(descendants)
    }

    pub(super) fn concrete_global_elements(&self) -> Result<&[ElementDeclaration], XmlFormatError> {
        if self.limit_reached {
            return Err(XmlFormatError::SubstitutionGroupLimit {
                limit: MAX_MATERIALIZED_SCHEMA_ELEMENTS,
            });
        }
        if let Some(identity) = &self.conflicting_global_identity {
            return Err(XmlFormatError::ConflictingSubstitutionMember {
                head: "global element declarations".to_string(),
                member: identity.clone(),
            });
        }
        Ok(&self.global_elements)
    }

    pub(super) fn concrete_global_attributes(
        &self,
    ) -> Result<&[ElementDeclaration], XmlFormatError> {
        if self.limit_reached {
            return Err(XmlFormatError::SubstitutionGroupLimit {
                limit: MAX_MATERIALIZED_SCHEMA_ELEMENTS,
            });
        }
        if let Some(identity) = &self.conflicting_global_attribute_identity {
            return Err(XmlFormatError::ConflictingSubstitutionMember {
                head: "global attribute declarations".to_string(),
                member: identity.clone(),
            });
        }
        Ok(&self.global_attributes)
    }

    pub(super) fn unresolved_namespaces(&self) -> &BTreeSet<Option<String>> {
        &self.unresolved_namespaces
    }

    /// Whether a document in the graph uses `xs:redefine` or `xs:override`,
    /// whose changed declarations are not indexed.
    pub(super) fn has_redefinition(&self) -> bool {
        self.redefinition
    }

    /// The number of namespace contexts through which this graph reaches a
    /// physical schema document.
    pub(super) fn namespace_contexts(&self, path: &Path) -> usize {
        let path = if self.supplied_identities {
            super::resources::lexical_path(path)
        } else {
            normalized_path(path)
        };
        self.effective_namespaces
            .get(&path)
            .map_or(0, BTreeSet::len)
    }

    /// Returns a chameleon schema's adopted namespace only when this graph
    /// reaches the physical file through exactly one namespace context.
    pub(super) fn effective_namespace(&self, path: &Path) -> Option<&str> {
        let path = if self.supplied_identities {
            super::resources::lexical_path(path)
        } else {
            normalized_path(path)
        };
        let namespaces = self.effective_namespaces.get(&path)?;
        if namespaces.len() != 1 {
            return None;
        }
        namespaces.first()?.as_deref()
    }
}

pub(super) fn build(
    schema: &Node<'_, '_>,
    schema_path: &Path,
    reader: &SchemaReader,
) -> SubstitutionIndex {
    let mut index = SubstitutionIndex {
        supplied_identities: reader.is_supplied(),
        ..SubstitutionIndex::default()
    };
    let mut visited = BTreeSet::new();
    let Some(root) = collect_declarations(
        schema,
        schema_path,
        schema.attribute("targetNamespace"),
        &mut visited,
        &mut index,
        reader,
    ) else {
        return index;
    };
    schema_links::walk::<()>(root, |linking, link| {
        let dependency_namespace = if link.include {
            linking.effective_namespace.as_deref()
        } else {
            link.namespace.as_deref()
        }
        .filter(|namespace| !namespace.is_empty())
        .map(str::to_string);
        let Some(location) = &link.location else {
            index.unresolved_namespaces.insert(dependency_namespace);
            return Follow::Skip;
        };
        let Some(dependency) = reader.dependency(&linking.path, location) else {
            index.unresolved_namespaces.insert(dependency_namespace);
            return Follow::Skip;
        };
        let Ok(text) = reader.read(&dependency) else {
            index.unresolved_namespaces.insert(dependency_namespace);
            return Follow::Skip;
        };
        let Ok(document) = roxmltree::Document::parse(&text) else {
            index.unresolved_namespaces.insert(dependency_namespace);
            return Follow::Skip;
        };
        collect_declarations(
            &document.root_element(),
            &dependency,
            linking.inherited_namespace(link),
            &mut visited,
            &mut index,
            reader,
        )
        .map_or(Follow::Skip, Follow::Descend)
    });
    index
}

/// Indexes one schema document's global declarations, returning the document
/// for its links unless it was already visited or the index is full.
fn collect_declarations(
    schema: &Node<'_, '_>,
    schema_path: &Path,
    inherited_namespace: Option<&str>,
    visited: &mut BTreeSet<(PathBuf, Option<String>)>,
    index: &mut SubstitutionIndex,
    reader: &SchemaReader,
) -> Option<LinkedSchema> {
    if index.limit_reached {
        return None;
    }
    let path = reader.identity(schema_path);
    let effective_namespace = schema.attribute("targetNamespace").or(inherited_namespace);
    index
        .effective_namespaces
        .entry(path.clone())
        .or_default()
        .insert(effective_namespace.map(str::to_string));
    if !visited.insert((path.clone(), effective_namespace.map(str::to_string))) {
        return None;
    }
    index.redefinition |= schema.children().any(|child| {
        child.is_element() && matches!(child.tag_name().name(), "redefine" | "override")
    });

    for declaration in schema
        .children()
        .filter(|candidate| candidate.is_element() && candidate.tag_name().name() == "element")
    {
        let Some(local) = declaration.attribute("name") else {
            continue;
        };
        let Some(identity) = type_identity_in_namespace(effective_namespace, local) else {
            continue;
        };
        let element = ElementDeclaration {
            path: path.clone(),
            local: local.to_string(),
            identity,
            namespace: effective_namespace.map(str::to_string),
            abstract_element: declaration
                .attribute("abstract")
                .is_some_and(|value| matches!(value, "true" | "1")),
        };
        index.insert_global(element.clone());
        if let Some(head) = declaration
            .attribute("substitutionGroup")
            .and_then(|head| expanded_qname_identity(schema, effective_namespace, head))
        {
            index.insert(IndexedDeclaration { element, head });
        }
        if index.limit_reached {
            return None;
        }
    }
    for declaration in schema
        .children()
        .filter(|candidate| candidate.is_element() && candidate.tag_name().name() == "attribute")
    {
        let Some(local) = declaration.attribute("name") else {
            continue;
        };
        let Some(identity) = type_identity_in_namespace(effective_namespace, local) else {
            continue;
        };
        index.insert_global_attribute(ElementDeclaration {
            path: path.clone(),
            local: local.to_string(),
            identity,
            namespace: effective_namespace.map(str::to_string),
            abstract_element: false,
        });
    }
    Some(LinkedSchema::new(schema, path, effective_namespace))
}
