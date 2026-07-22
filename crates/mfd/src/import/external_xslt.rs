use std::io::Read;
use std::path::{Path, PathBuf};

use ir::SchemaKind;
use mapping::{AggregateOp, Node, NodeId};

use super::graph::GraphBuilder;
use super::iteration::split_at_innermost_repeating;
use super::schema::{SchemaComponent, parse_u32, schema_node_at, schema_node_at_resolved};

const XSLT_NAMESPACE: &str = "http://www.w3.org/1999/XSL/Transform";
const MAX_MODULE_BYTES: u64 = 1024 * 1024;
const MAX_SELECTOR_BYTES: usize = 4096;
const MAX_SELECTOR_SEGMENTS: usize = 64;

#[derive(Clone)]
pub(super) struct Recipe {
    pub(super) input: u32,
    pub(super) output: u32,
    function: AggregateOp,
    selector: Vec<String>,
}

/// Imports a deliberately small, executable XSLT extension-function shape:
/// one named template with one parameter and one aggregate `value-of`.
/// Unknown libraries without an adjacent module remain available to the
/// ordinary opaque-UDF fallback.
pub(super) fn read(
    component: &roxmltree::Node<'_, '_>,
    mapping_path: &Path,
) -> Result<Option<Recipe>, String> {
    if component.attribute("kind") != Some("5") {
        return Ok(None);
    }
    let library = component.attribute("library").unwrap_or_default();
    let Some(module_path) = module_path(mapping_path, library) else {
        return Ok(None);
    };
    if !module_path.is_file() {
        return Ok(None);
    }

    let name = component
        .attribute("name")
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "component has no function name".to_string())?;
    let input = exactly_one_pin(component, "sources", "input")?;
    let output = exactly_one_pin(component, "targets", "output")?;
    let text = read_module(&module_path)?;
    let document = roxmltree::Document::parse(&text)
        .map_err(|error| format!("module `{}` is not XML: {error}", module_path.display()))?;
    let templates = document
        .descendants()
        .filter(|node| is_xslt(*node, "template") && node.attribute("name") == Some(name))
        .collect::<Vec<_>>();
    let [template] = templates.as_slice() else {
        return Err(format!(
            "module `{}` must contain exactly one XSLT template named `{name}`",
            module_path.display()
        ));
    };
    let parameters = template
        .children()
        .filter(|node| is_xslt(*node, "param"))
        .filter_map(|node| node.attribute("name"))
        .collect::<Vec<_>>();
    let [parameter] = parameters.as_slice() else {
        return Err("template must declare exactly one direct parameter".to_string());
    };
    let values = template
        .descendants()
        .filter(|node| is_xslt(*node, "value-of"))
        .collect::<Vec<_>>();
    let [value] = values.as_slice() else {
        return Err("template must contain exactly one XSLT value-of".to_string());
    };
    let select = value
        .attribute("select")
        .ok_or_else(|| "XSLT value-of has no select expression".to_string())?;
    let (function, referenced_parameter, selector) = parse_selector(select)?;
    if referenced_parameter != *parameter {
        return Err(format!(
            "aggregate selector references `${referenced_parameter}` instead of template parameter `${parameter}`"
        ));
    }

    Ok(Some(Recipe {
        input,
        output,
        function,
        selector,
    }))
}

fn module_path(mapping_path: &Path, library: &str) -> Option<PathBuf> {
    if library.is_empty()
        || library.len() > 255
        || library.contains('/')
        || library.contains('\\')
        || matches!(library, "." | "..")
    {
        return None;
    }
    let parent = mapping_path.parent().unwrap_or_else(|| Path::new("."));
    ["xslt", "xsl"]
        .into_iter()
        .map(|extension| parent.join(format!("{library}.{extension}")))
        .find(|path| path.is_file())
}

fn read_module(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("could not read module `{}`: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_MODULE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("could not read module `{}`: {error}", path.display()))?;
    if bytes.len() as u64 > MAX_MODULE_BYTES {
        return Err(format!(
            "module `{}` exceeds the {MAX_MODULE_BYTES}-byte limit",
            path.display()
        ));
    }
    String::from_utf8(bytes).map_err(|_| format!("module `{}` is not UTF-8", path.display()))
}

fn exactly_one_pin(
    component: &roxmltree::Node<'_, '_>,
    container: &str,
    label: &str,
) -> Result<u32, String> {
    let pins = component
        .children()
        .find(|node| node.has_tag_name(container))
        .into_iter()
        .flat_map(|node| {
            node.children()
                .filter(|child| child.has_tag_name("datapoint"))
        })
        .filter_map(|node| parse_u32(node.attribute("key")))
        .collect::<Vec<_>>();
    let [pin] = pins.as_slice() else {
        return Err(format!(
            "component must declare exactly one keyed {label} pin"
        ));
    };
    Ok(*pin)
}

fn is_xslt(node: roxmltree::Node<'_, '_>, local_name: &str) -> bool {
    node.is_element()
        && node.tag_name().name() == local_name
        && node.tag_name().namespace() == Some(XSLT_NAMESPACE)
}

fn parse_selector(select: &str) -> Result<(AggregateOp, String, Vec<String>), String> {
    let select = select.trim();
    if select.len() > MAX_SELECTOR_BYTES {
        return Err(format!(
            "aggregate selector exceeds the {MAX_SELECTOR_BYTES}-byte limit"
        ));
    }
    let (function, argument) = select
        .split_once('(')
        .and_then(|(function, rest)| {
            rest.strip_suffix(')')
                .map(|rest| (function.trim(), rest.trim()))
        })
        .ok_or_else(|| "value-of must be one aggregate function call".to_string())?;
    if argument.contains(['(', ')', ',', '[', ']']) {
        return Err("aggregate argument must be one direct parameter path".to_string());
    }
    let function = match function.rsplit(':').next().unwrap_or(function) {
        "count" => AggregateOp::Count,
        "sum" => AggregateOp::Sum,
        "avg" => AggregateOp::Avg,
        "min" => AggregateOp::Min,
        "max" => AggregateOp::Max,
        other => return Err(format!("aggregate function `{other}` is unsupported")),
    };
    let argument = argument
        .strip_prefix('$')
        .ok_or_else(|| "aggregate argument must start with a template parameter".to_string())?;
    let (parameter, path) = argument
        .split_once('/')
        .ok_or_else(|| "aggregate parameter must select a descendant value path".to_string())?;
    if !is_safe_name(parameter) {
        return Err("aggregate parameter name is invalid".to_string());
    }
    let selector = path.split('/').map(str::to_string).collect::<Vec<_>>();
    if selector.is_empty()
        || selector.len() > MAX_SELECTOR_SEGMENTS
        || selector.iter().any(|segment| !is_safe_name(segment))
    {
        return Err(format!(
            "aggregate selector must contain 1..={MAX_SELECTOR_SEGMENTS} direct XML name segments"
        ));
    }
    Ok((function, parameter.to_string(), selector))
}

fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 1024
        && name != "."
        && name != ".."
        && name.chars().all(|character| {
            character.is_alphanumeric() || matches!(character, '_' | '-' | '.' | ':')
        })
}

impl GraphBuilder<'_> {
    pub(super) fn external_xslt_aggregate_node(&mut self, output: u32) -> Option<NodeId> {
        if let Some(node) = self.external_xslt_nodes.get(&output) {
            return Some(*node);
        }
        let recipe = self
            .external_xslt_aggregates
            .iter()
            .find(|recipe| recipe.output == output)?
            .clone();
        let feed = self.edge_from.get(&recipe.input).copied()?;
        let source = self.source_abs_path(feed)?;
        let component = self.sources.get(source.source)?;
        let full_path = resolve_selector(component, &source.path, &recipe.selector)?;
        let (collection, value) = split_at_innermost_repeating(&component.schema, &full_path);
        if collection.is_empty() || value.is_empty() || !collection.starts_with(&source.path) {
            return None;
        }
        if !schema_node_at(&component.schema, &full_path)
            .is_some_and(|node| !node.repeating && matches!(node.kind, SchemaKind::Scalar { .. }))
        {
            return None;
        }
        let collection = self.collection_path(source.source, &collection)?;
        let node = self.alloc(Node::Aggregate {
            function: recipe.function,
            collection,
            value,
            expression: None,
            arg: None,
        });
        self.external_xslt_nodes.insert(output, node);
        Some(node)
    }
}

fn resolve_selector(
    component: &SchemaComponent,
    base: &[String],
    selector: &[String],
) -> Option<Vec<String>> {
    let mut path = base.to_vec();
    for requested in selector {
        let parent = schema_node_at_resolved(&component.schema, &path)?;
        let SchemaKind::Group { children, .. } = &parent.kind else {
            return None;
        };
        let selected = match children.iter().find(|child| child.name == *requested) {
            Some(child) => child.name.clone(),
            None => {
                let local = local_name(requested);
                let mut matches = children
                    .iter()
                    .filter(|child| local_name(&child.name) == local);
                let child = matches.next()?;
                if matches.next().is_some() {
                    return None;
                }
                child.name.clone()
            }
        };
        path.push(selected);
    }
    Some(path)
}

fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use mapping::AggregateOp;

    use super::parse_selector;

    #[test]
    fn parses_bounded_parameter_aggregate_selectors() {
        assert_eq!(
            parse_selector("sum($rows/Item/Cost)"),
            Ok((
                AggregateOp::Sum,
                "rows".to_string(),
                vec!["Item".to_string(), "Cost".to_string()]
            ))
        );
        assert!(parse_selector("sum($rows/Item[@active]/Cost)").is_err());
        assert!(parse_selector("document($path)").is_err());
    }
}
