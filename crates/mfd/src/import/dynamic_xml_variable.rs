use ir::{SchemaKind, Value, XML_ELEMENTS_FIELD, XML_LOCAL_NAME_FIELD, XML_TEXT_FIELD};
use mapping::{Node, NodeId};

use super::graph::GraphBuilder;
use super::schema::SchemaComponent;

impl GraphBuilder<'_> {
    /// Lowers a bounded XML variable pattern that pivots key/value rows into
    /// statically named output fields. MapForce represents this by constructing
    /// generic elements whose names come from one row field and whose text comes
    /// from another, then exposing ordinary schema fields on the variable.
    pub(super) fn dynamic_xml_variable_lookup_node(&mut self, output_key: u32) -> Option<NodeId> {
        let component = self
            .intermediates
            .iter()
            .copied()
            .find(|component| component.output_keys.contains(&output_key))?;
        let output_path = component.ports.get(&output_key)?;

        let (owner_path, owner_feed) = connected_owner(component, output_path, self.edge_from)?;
        let [output_name] = output_path.strip_prefix(owner_path.as_slice())? else {
            return None;
        };
        if !has_unique_scalar_descendant(&component.schema, output_name) {
            return None;
        }

        let mut elements_path = owner_path;
        elements_path.push(XML_ELEMENTS_FIELD.to_string());
        let elements_feed = connected_input(component, &elements_path, self.edge_from)?;

        let mut name_path = elements_path.clone();
        name_path.push(XML_LOCAL_NAME_FIELD.to_string());
        let name_feed = connected_input(component, &name_path, self.edge_from)?;

        let mut text_path = elements_path;
        text_path.push(XML_TEXT_FIELD.to_string());
        let text_feed = connected_input(component, &text_path, self.edge_from)?;

        let collection = self.source_abs_path(elements_feed)?;
        let owner = self.source_abs_path(owner_feed)?;
        let key = self.source_abs_path(name_feed)?;
        let value = self.source_abs_path(text_feed)?;
        if collection.source != owner.source
            || collection.source != key.source
            || collection.source != value.source
            || collection.path.split_last()?.1 != owner.path
        {
            return None;
        }

        let collection_node = self.schema_node(&collection)?;
        if !collection_node.repeating || !matches!(collection_node.kind, SchemaKind::Group { .. }) {
            return None;
        }

        let key = self.source_value_path(key.source, key.path);
        let value = self.source_value_path(value.source, value.path);
        let key_path = scalar_suffix(&collection.path, &key.path, self.schema_node(&key)?)?;
        let value_path = scalar_suffix(&collection.path, &value.path, self.schema_node(&value)?)?;
        let collection_path = self.collection_path(collection.source, &collection.path)?;

        let matches = self.alloc(Node::Const {
            value: Value::String(output_name.clone()),
        });
        Some(self.alloc(Node::Lookup {
            collection: collection_path,
            key: key_path,
            matches,
            value: value_path,
        }))
    }
}

fn connected_owner(
    component: &SchemaComponent,
    output_path: &[String],
    edge_from: &std::collections::BTreeMap<u32, u32>,
) -> Option<(Vec<String>, u32)> {
    let mut owners = component
        .ports
        .iter()
        .filter(|(key, path)| {
            component.input_keys.contains(key)
                && edge_from.contains_key(key)
                && output_path.starts_with(path)
        })
        .collect::<Vec<_>>();
    owners.sort_by_key(|(_, path)| path.len());
    let (key, path) = owners.pop()?;
    if owners
        .last()
        .is_some_and(|(_, other)| other.len() == path.len())
    {
        return None;
    }
    Some((path.clone(), *edge_from.get(key)?))
}

fn connected_input(
    component: &SchemaComponent,
    path: &[String],
    edge_from: &std::collections::BTreeMap<u32, u32>,
) -> Option<u32> {
    let mut feeds = component.ports.iter().filter_map(|(key, port_path)| {
        (component.input_keys.contains(key) && port_path == path)
            .then(|| edge_from.get(key).copied())
            .flatten()
    });
    let feed = feeds.next()?;
    feeds.next().is_none().then_some(feed)
}

fn scalar_suffix(
    collection: &[String],
    path: &[String],
    node: &ir::SchemaNode,
) -> Option<Vec<String>> {
    if node.repeating || !matches!(node.kind, SchemaKind::Scalar { .. }) {
        return None;
    }
    let suffix = path.strip_prefix(collection)?.to_vec();
    (!suffix.is_empty()).then_some(suffix)
}

fn has_unique_scalar_descendant(schema: &ir::SchemaNode, name: &str) -> bool {
    fn count(schema: &ir::SchemaNode, name: &str) -> usize {
        let own = usize::from(
            schema.name == name
                && !schema.repeating
                && matches!(schema.kind, SchemaKind::Scalar { .. }),
        );
        let children = match &schema.kind {
            SchemaKind::Group { children, .. } => children,
            SchemaKind::Scalar { .. } => return own,
        };
        children
            .iter()
            .try_fold(own, |total, child| {
                let total = total + count(child, name);
                (total < 2).then_some(total)
            })
            .unwrap_or(2)
    }

    count(schema, name) == 1
}
