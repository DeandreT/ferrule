use std::collections::BTreeSet;

use ir::{SchemaKind, SchemaNode};

use crate::{Binding, ScalarTargetDomain, TargetConstruction, TargetScope};

use super::Json5BoundaryPolicyError;

fn refusal(field: &'static str) -> Json5BoundaryPolicyError {
    Json5BoundaryPolicyError::TargetScope { field }
}

pub(super) fn validate(
    root: &TargetScope,
    schema: &SchemaNode,
) -> Result<(), Json5BoundaryPolicyError> {
    let mut pending = vec![(root, schema, true)];
    while let Some((scope, schema, primary)) = pending.pop() {
        let TargetScope {
            target_field,
            repeating,
            iteration,
            construction,
            bindings,
            children,
        } = scope;
        if (primary && !target_field.is_empty()) || (!primary && *target_field != schema.name) {
            return Err(refusal("target_field"));
        }
        if *repeating {
            return Err(refusal("repeating"));
        }
        if iteration.is_some() {
            return Err(refusal("iteration"));
        }
        match construction {
            TargetConstruction::Group => {}
            TargetConstruction::DynamicGroup {
                fixed_fields: _,
                bindings: _,
                children: _,
                merge: _,
            }
            | TargetConstruction::CopyCurrentSource
            | TargetConstruction::Scalar {
                expression: _,
                target_domain: _,
            }
            | TargetConstruction::XmlMixedContent { elements: _ }
            | TargetConstruction::RecursiveFilter {
                children: _,
                items: _,
                predicate: _,
            }
            | TargetConstruction::PathHierarchy {
                collection: _,
                separator: _,
                directories: _,
                files: _,
                name: _,
            }
            | TargetConstruction::AdjacencyTree {
                collection: _,
                key: _,
                parent: _,
                target_key: _,
                target_children: _,
                root: _,
            } => return Err(refusal("construction")),
        }
        let SchemaKind::Group {
            children: declared,
            alternatives: _,
            required: _,
            xml_restricted_alternatives: _,
            dynamic: _,
        } = &schema.kind
        else {
            return Err(refusal("schema"));
        };
        // The prior complete schema census and sibling uniqueness proof bound this
        // scope walk: each scope consumes a distinct declared group child.
        if bindings.len().saturating_add(children.len()) > declared.len() {
            return Err(refusal("member_count"));
        }
        let mut seen = BTreeSet::new();
        for binding in bindings {
            let Binding {
                target_field,
                expression: _,
                target_domain,
                repeating,
            } = binding;
            if !seen.insert(target_field.as_str()) {
                return Err(refusal("duplicate_member"));
            }
            let Some(field) = declared.iter().find(|field| field.name == *target_field) else {
                return Err(refusal("binding_field"));
            };
            let SchemaKind::Scalar { ty } = &field.kind else {
                return Err(refusal("binding_scalar"));
            };
            if *repeating {
                return Err(refusal("binding_repeating"));
            }
            match target_domain {
                ScalarTargetDomain::Single(actual) if actual == ty => {}
                ScalarTargetDomain::Single(_) | ScalarTargetDomain::Union(_) => {
                    return Err(refusal("binding_domain"));
                }
            }
        }
        for child in children {
            if !seen.insert(child.target_field.as_str()) {
                return Err(refusal("duplicate_member"));
            }
            let Some(field) = declared
                .iter()
                .find(|field| field.name == child.target_field)
            else {
                return Err(refusal("child_field"));
            };
            pending.push((child, field, false));
        }
    }
    Ok(())
}
