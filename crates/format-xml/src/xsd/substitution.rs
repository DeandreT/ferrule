use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use roxmltree::Node;

use crate::XmlFormatError;

use super::{
    MAX_MATERIALIZED_SCHEMA_ELEMENTS, expanded_qname_identity, normalized_path, read_xml_text,
    type_identity_in_namespace,
};

#[derive(Debug, Clone)]
pub(super) struct ElementDeclaration {
    pub path: PathBuf,
    pub local: String,
    pub identity: String,
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
    conflicting_identity: Option<String>,
    limit_reached: bool,
}

impl SubstitutionIndex {
    fn insert(&mut self, declaration: IndexedDeclaration) {
        if let Some(existing) = self
            .by_identity
            .get(&declaration.element.identity)
            .and_then(|index| self.declarations.get(*index))
        {
            if existing.element.path != declaration.element.path
                || existing.element.local != declaration.element.local
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
}

pub(super) fn build(schema: &Node<'_, '_>, schema_path: &Path) -> SubstitutionIndex {
    let mut index = SubstitutionIndex::default();
    collect_declarations(
        schema,
        schema_path,
        schema.attribute("targetNamespace"),
        &mut BTreeSet::new(),
        &mut index,
    );
    index
}

fn collect_declarations(
    schema: &Node<'_, '_>,
    schema_path: &Path,
    inherited_namespace: Option<&str>,
    visited: &mut BTreeSet<(PathBuf, Option<String>)>,
    index: &mut SubstitutionIndex,
) {
    if index.limit_reached {
        return;
    }
    let path = normalized_path(schema_path);
    let effective_namespace = schema.attribute("targetNamespace").or(inherited_namespace);
    if !visited.insert((path.clone(), effective_namespace.map(str::to_string))) {
        return;
    }

    for declaration in schema.children().filter(|candidate| {
        candidate.is_element()
            && candidate.tag_name().name() == "element"
            && candidate.attribute("substitutionGroup").is_some()
    }) {
        let Some(local) = declaration.attribute("name") else {
            continue;
        };
        let Some(head) = declaration
            .attribute("substitutionGroup")
            .and_then(|head| expanded_qname_identity(schema, effective_namespace, head))
        else {
            continue;
        };
        let Some(identity) = type_identity_in_namespace(effective_namespace, local) else {
            continue;
        };
        index.insert(IndexedDeclaration {
            element: ElementDeclaration {
                path: path.clone(),
                local: local.to_string(),
                identity,
                abstract_element: declaration
                    .attribute("abstract")
                    .is_some_and(|value| matches!(value, "true" | "1")),
            },
            head,
        });
        if index.limit_reached {
            return;
        }
    }

    for link in schema
        .children()
        .filter(|node| node.is_element() && matches!(node.tag_name().name(), "include" | "import"))
    {
        let Some(location) = link.attribute("schemaLocation") else {
            continue;
        };
        if location.contains("://") {
            continue;
        }
        let dependency = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(location);
        let Ok(text) = read_xml_text(&dependency) else {
            continue;
        };
        let Ok(document) = roxmltree::Document::parse(&text) else {
            continue;
        };
        let dependency_schema = document.root_element();
        let dependency_inherited = (link.tag_name().name() == "include")
            .then_some(effective_namespace)
            .flatten();
        collect_declarations(
            &dependency_schema,
            &dependency,
            dependency_inherited,
            visited,
            index,
        );
    }
}
