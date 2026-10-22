//! Authored JSON target properties that an open object admits without
//! declaring them.
//!
//! A design may add a named property such as `code` beside a declared `Code`
//! when the referenced schema permits additional properties. The entry name is
//! kept exactly and recorded with [`SchemaNode::add_json_undeclared_property`];
//! lookups are never case-folded and the authored schema is not rewritten.
//! Closed objects still refuse the port through the ordinary missing-path
//! diagnostic.

use ir::{JsonUndeclaredPropertyError, SchemaKind, SchemaNode};

/// Records every input-bearing `json-property` entry missing from an open
/// object of `schema`. Computed-name entries remain dynamic targets.
pub(super) fn attach(
    entry: &roxmltree::Node<'_, '_>,
    schema: &mut SchemaNode,
    component: &str,
    warnings: &mut Vec<String>,
) {
    attach_below(entry, schema, &mut Vec::new(), component, warnings);
}

fn attach_below(
    entry: &roxmltree::Node<'_, '_>,
    owner: &mut SchemaNode,
    path: &mut Vec<String>,
    component: &str,
    warnings: &mut Vec<String>,
) {
    for child in entry.children().filter(|node| node.has_tag_name("entry")) {
        match child.attribute("type") {
            Some("json-property") => {
                if is_computed_name(&child) {
                    continue;
                }
                let name = child.attribute("name").unwrap_or_default();
                path.push(name.to_owned());
                if let Some(member) = member(owner, name, &child, path, component, warnings) {
                    attach_below(&child, member, path, component, warnings);
                }
                path.pop();
            }
            Some("json-propertyname") => {}
            _ => attach_below(&child, owner, path, component, warnings),
        }
    }
}

fn member<'a>(
    owner: &'a mut SchemaNode,
    name: &str,
    entry: &roxmltree::Node<'_, '_>,
    path: &[String],
    component: &str,
    warnings: &mut Vec<String>,
) -> Option<&'a mut SchemaNode> {
    if !matches!(owner.kind, SchemaKind::Group { .. }) {
        return None;
    }
    if owner.child(name).is_none() {
        if !declares_input(entry) {
            return None;
        }
        match owner.add_json_undeclared_property(name) {
            Ok(()) => {}
            // A closed object keeps the ordinary missing-path diagnostic.
            Err(JsonUndeclaredPropertyError::Closed) => return None,
            Err(error) => {
                warnings.push(format!(
                    "JSON target `{component}` property `{}` is not admitted by its object: {error}",
                    path.join("/")
                ));
                return None;
            }
        }
    }
    let SchemaKind::Group { children, .. } = &mut owner.kind else {
        return None;
    };
    children.iter_mut().find(|child| child.name == name)
}

fn is_computed_name(property: &roxmltree::Node<'_, '_>) -> bool {
    property.children().any(|node| {
        node.has_tag_name("entry") && node.attribute("type") == Some("json-propertyname")
    })
}

fn declares_input(entry: &roxmltree::Node<'_, '_>) -> bool {
    entry
        .descendants()
        .any(|node| node.has_tag_name("entry") && node.attribute("inpkey").is_some())
}
