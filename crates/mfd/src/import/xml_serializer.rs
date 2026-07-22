use mapping::{Node, NodeId};

use super::graph::GraphBuilder;
use super::schema::{SchemaComponent, parse_u32};

pub(super) struct Recipe {
    pub(super) output: u32,
    input: u32,
    schema: ir::SchemaNode,
    declaration: bool,
    indent: bool,
    namespace: Option<String>,
}

pub(super) fn read(
    component: &roxmltree::Node<'_, '_>,
    schema: &SchemaComponent,
) -> Result<Option<Recipe>, String> {
    let string_serializer = component.descendants().any(|node| {
        node.has_tag_name("parameter") && node.attribute("usageKind") == Some("stringserialize")
    });
    if !string_serializer {
        return Ok(None);
    }

    let outputs = component
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
        .filter_map(|entry| parse_u32(entry.attribute("outkey")))
        .collect::<Vec<_>>();
    let [output] = outputs.as_slice() else {
        return Err("expected exactly one serialized string output".to_string());
    };
    let root_inputs = schema
        .input_keys
        .iter()
        .filter(|input| schema.ports.get(input).is_some_and(Vec::is_empty))
        .copied()
        .collect::<Vec<_>>();
    let [input] = root_inputs.as_slice() else {
        return Err("expected exactly one structural document-root input".to_string());
    };

    let properties = component
        .children()
        .find(|node| node.has_tag_name("properties"));
    let encoding = properties
        .and_then(|node| node.attribute("XSLTTargetEncoding"))
        .unwrap_or("UTF-8");
    if !encoding.eq_ignore_ascii_case("UTF-8") {
        return Err(format!(
            "encoding `{encoding}` is unsupported; expected UTF-8"
        ));
    }
    let declaration =
        properties.and_then(|node| node.attribute("WriteXMLDeclaration")) != Some("0");
    let indent = properties.and_then(|node| node.attribute("ferrule-indent")) != Some("0");
    let namespace = component
        .descendants()
        .find(|node| node.has_tag_name("document"))
        .and_then(|document| document.attribute("instanceroot"))
        .and_then(expanded_namespace)
        .map(str::to_string);

    Ok(Some(Recipe {
        output: *output,
        input: *input,
        schema: schema.schema.clone(),
        declaration,
        indent,
        namespace,
    }))
}

impl GraphBuilder<'_> {
    pub(super) fn xml_serializer_node(&mut self, output: u32) -> Option<NodeId> {
        if let Some(node) = self.xml_serializer_nodes.get(&output) {
            return Some(*node);
        }
        let recipe = self
            .xml_serializers
            .iter()
            .find(|serializer| serializer.output == output)?;
        let feed = self.edge_from.get(&recipe.input).copied()?;
        let source = self.sequence_source_path(feed)?;
        let (frame, path) = self.source_location_at(&source)?;
        let node = self.alloc(Node::XmlSerialize {
            path,
            frame,
            schema: recipe.schema.clone(),
            declaration: recipe.declaration,
            indent: recipe.indent,
            namespace: recipe.namespace.clone(),
        });
        self.xml_serializer_nodes.insert(output, node);
        Some(node)
    }
}

fn expanded_namespace(name: &str) -> Option<&str> {
    let rest = name.strip_prefix('{')?;
    let (namespace, _) = rest.split_once('}')?;
    (!namespace.is_empty()).then_some(namespace)
}

#[cfg(test)]
mod tests {
    use super::expanded_namespace;

    #[test]
    fn reads_expanded_root_namespaces() {
        assert_eq!(
            expanded_namespace("{urn:company}Person"),
            Some("urn:company")
        );
        assert_eq!(expanded_namespace("{}Person"), None);
        assert_eq!(expanded_namespace("Person"), None);
    }
}
