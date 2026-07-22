mod csharp;
mod java;
mod lexer;
mod xquery;

use std::io::Read;
use std::path::{Path, PathBuf};

use ir::Value;
use mapping::{Node, NodeId};

use super::graph::GraphBuilder;
use super::schema::parse_u32;

const MAX_MODULE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Expr {
    Input(usize),
    Const(Value),
    Call { function: String, args: Vec<Expr> },
}

#[derive(Clone)]
pub(super) struct Recipe {
    pub(super) output: u32,
    inputs: Vec<u32>,
    expression: Expr,
}

pub(super) fn read(
    component: &roxmltree::Node<'_, '_>,
    mapping_path: &Path,
    selected_language: &str,
) -> Result<Option<Recipe>, String> {
    if component.attribute("kind") != Some("5") {
        return Ok(None);
    }
    let library = component.attribute("library").unwrap_or_default();
    let name = component.attribute("name").unwrap_or_default();
    let module = match selected_language {
        "cs" | "csharp" => csharp::source_path(mapping_path, library, name)
            .filter(|path| path.is_file())
            .map(Module::CSharp),
        "java" => java::source_path(mapping_path, library)
            .filter(|path| path.is_file())
            .map(Module::Java),
        "xquery" => xquery::source_path(mapping_path, library).map(Module::XQuery),
        _ => None,
    };
    let Some(module) = module else {
        return Ok(None);
    };
    let inputs = ordered_pins(component, "sources")?;
    let outputs = ordered_pins(component, "targets")?;
    let [output] = outputs.as_slice() else {
        return Ok(None);
    };

    let expression = match module {
        Module::CSharp(path) => {
            if inputs.len() != 1 {
                return Err("C# numeric formatter must have exactly one input".to_string());
            }
            let method = name
                .rsplit('.')
                .next()
                .filter(|method| !method.is_empty())
                .ok_or_else(|| "C# component has no method name".to_string())?;
            let picture = csharp::parse(&read_module(&path)?, method)?;
            format_number_expr(picture)
        }
        Module::Java(path) => {
            if inputs.len() != 1 {
                return Err("Java numeric formatter must have exactly one input".to_string());
            }
            let method = name
                .rsplit('.')
                .next()
                .filter(|method| !method.is_empty())
                .ok_or_else(|| "Java component has no method name".to_string())?;
            let picture = java::parse(&read_module(&path)?, method)?;
            format_number_expr(picture)
        }
        Module::XQuery(path) => {
            let source = read_module(&path)?;
            xquery::parse(&source, name, inputs.len())?
        }
    };

    Ok(Some(Recipe {
        output: *output,
        inputs,
        expression,
    }))
}

enum Module {
    CSharp(PathBuf),
    Java(PathBuf),
    XQuery(PathBuf),
}

fn format_number_expr(picture: String) -> Expr {
    Expr::Call {
        function: "format_number".to_string(),
        args: vec![Expr::Input(0), Expr::Const(Value::String(picture))],
    }
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

fn ordered_pins(component: &roxmltree::Node<'_, '_>, container: &str) -> Result<Vec<u32>, String> {
    let mut pins = Vec::<Option<u32>>::new();
    for (fallback, point) in component
        .children()
        .find(|node| node.has_tag_name(container))
        .into_iter()
        .flat_map(|node| {
            node.children()
                .filter(|child| child.has_tag_name("datapoint"))
        })
        .enumerate()
    {
        let position = parse_u32(point.attribute("pos"))
            .and_then(|position| usize::try_from(position).ok())
            .unwrap_or(fallback);
        if position >= 64 {
            return Err("external scalar component pin position exceeds 63".to_string());
        }
        if pins.len() <= position {
            pins.resize(position + 1, None);
        }
        if pins[position].is_some() {
            return Err(format!(
                "external scalar component duplicates pin {position}"
            ));
        }
        pins[position] = parse_u32(point.attribute("key"));
    }
    if pins.iter().any(Option::is_none) {
        return Err("external scalar component contains an unkeyed pin".to_string());
    }
    Ok(pins.into_iter().flatten().collect())
}

impl GraphBuilder<'_> {
    pub(super) fn external_scalar_node(&mut self, output: u32) -> Option<NodeId> {
        if let Some(node) = self.external_scalar_nodes.get(&output) {
            return Some(*node);
        }
        let recipe = self
            .external_scalar_recipes
            .iter()
            .find(|recipe| recipe.output == output)?
            .clone();
        let node = self.lower_external_expr(&recipe, &recipe.expression)?;
        self.external_scalar_nodes.insert(output, node);
        Some(node)
    }

    fn lower_external_expr(&mut self, recipe: &Recipe, expression: &Expr) -> Option<NodeId> {
        match expression {
            Expr::Input(index) => recipe
                .inputs
                .get(*index)
                .and_then(|input| self.edge_from.get(input))
                .copied()
                .and_then(|feed| self.value_node(feed))
                .or_else(|| Some(self.const_null())),
            Expr::Const(value) => Some(self.alloc(Node::Const {
                value: value.clone(),
            })),
            Expr::Call { function, args } => {
                let args = args
                    .iter()
                    .map(|argument| self.lower_external_expr(recipe, argument))
                    .collect::<Option<Vec<_>>>()?;
                Some(self.alloc(Node::Call {
                    function: function.clone(),
                    args,
                }))
            }
        }
    }
}
