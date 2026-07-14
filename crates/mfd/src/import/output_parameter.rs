use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, SchemaNode};
use mapping::FormatOptions;

use crate::MfdError;

use super::schema::{ComponentFormat, SchemaComponent, parse_u32};

pub(super) struct OutputParameter {
    name: String,
    input_key: u32,
    scalar_type: ScalarType,
}

pub(super) fn read(component: &roxmltree::Node<'_, '_>) -> Result<OutputParameter, String> {
    let label = component.attribute("name").unwrap_or("output");
    let data = component
        .children()
        .find(|node| node.has_tag_name("data"))
        .ok_or_else(|| format!("output parameter `{label}` has no data declaration"))?;
    let parameter = data
        .children()
        .find(|node| {
            node.has_tag_name("parameter") && node.attribute("usageKind") == Some("output")
        })
        .ok_or_else(|| format!("core kind=7 component `{label}` is not an output parameter"))?;
    let name = parameter
        .attribute("name")
        .filter(|name| !name.is_empty())
        .ok_or_else(|| format!("output parameter `{label}` has no name"))?
        .to_string();
    let datatype = data
        .children()
        .find(|node| node.has_tag_name("output"))
        .and_then(|output| output.attribute("datatype"))
        .unwrap_or("string");
    let scalar_type = match datatype {
        "string" => ScalarType::String,
        "integer" | "int" | "long" => ScalarType::Int,
        "decimal" | "double" | "float" => ScalarType::Float,
        "boolean" => ScalarType::Bool,
        other => {
            return Err(format!(
                "output parameter `{name}` uses unsupported datatype `{other}`"
            ));
        }
    };

    let pins: Vec<_> = component
        .children()
        .find(|node| node.has_tag_name("sources"))
        .into_iter()
        .flat_map(|sources| {
            sources
                .children()
                .filter(|node| node.has_tag_name("datapoint"))
        })
        .collect();
    if pins.len() != 1 || pins[0].attribute("pos").is_some_and(|pos| pos != "0") {
        return Err(format!(
            "output parameter `{name}` must declare exactly one pos=0 input"
        ));
    }
    let input_key = parse_u32(pins[0].attribute("key"))
        .ok_or_else(|| format!("output parameter `{name}` has no keyed input"))?;

    Ok(OutputParameter {
        name,
        input_key,
        scalar_type,
    })
}

struct BuildResult {
    component: Option<SchemaComponent>,
    issues: Vec<String>,
}

fn build(
    parameters: Vec<Result<OutputParameter, String>>,
    edge_from: &BTreeMap<u32, u32>,
) -> BuildResult {
    let mut issues = Vec::new();
    let mut fields = Vec::new();
    let mut ports = BTreeMap::new();
    let mut names = BTreeSet::new();
    let mut keys = BTreeSet::new();

    for parameter in parameters {
        let parameter = match parameter {
            Ok(parameter) => parameter,
            Err(issue) => {
                issues.push(issue);
                continue;
            }
        };
        if !edge_from.contains_key(&parameter.input_key) {
            issues.push(format!(
                "output parameter `{}` has no connected value; skipped",
                parameter.name
            ));
            continue;
        }
        if !names.insert(parameter.name.clone()) {
            issues.push(format!(
                "duplicate output parameter `{}`; later declaration skipped",
                parameter.name
            ));
            continue;
        }
        if !keys.insert(parameter.input_key) {
            issues.push(format!(
                "output parameter `{}` reuses another parameter's input key {}; skipped",
                parameter.name, parameter.input_key
            ));
            continue;
        }
        ports.insert(parameter.input_key, vec![parameter.name.clone()]);
        fields.push(SchemaNode::scalar(parameter.name, parameter.scalar_type));
    }

    let component = (!fields.is_empty()).then(|| SchemaComponent {
        name: "Outputs".to_string(),
        format: ComponentFormat::Xml,
        schema: SchemaNode::group("Outputs", fields),
        input_instance: None,
        output_instance: None,
        options: FormatOptions::default(),
        is_source: false,
        is_default_output: true,
        is_variable: false,
        compute_when_key: None,
        input_keys: ports.keys().copied().collect(),
        output_keys: BTreeSet::new(),
        ports,
        db_queries: Vec::new(),
        dynamic_json: None,
    });
    BuildResult { component, issues }
}

pub(super) fn install_fallback(
    components: &mut Vec<SchemaComponent>,
    parameters: Vec<Result<OutputParameter, String>>,
    edge_from: &BTreeMap<u32, u32>,
    warnings: &mut Vec<String>,
) -> bool {
    if parameters.is_empty()
        || components
            .iter()
            .any(|component| !component.is_variable && !component.is_source)
    {
        return false;
    }
    let built = build(parameters, edge_from);
    warnings.extend(built.issues);
    match built.component {
        Some(component) => {
            components.push(component);
            false
        }
        None => true,
    }
}

pub(super) fn missing_error(
    side: &str,
    skipped_libraries: &[String],
    output_parameter_target_failed: bool,
) -> MfdError {
    if side == "target" && output_parameter_target_failed {
        return MfdError::Unsupported(
            "no importable target component found; core output parameters were present but none had a unique supported datatype and connected value"
                .to_string(),
        );
    }
    MfdError::Unsupported(if skipped_libraries.is_empty() {
        format!(
            "no importable {side} component (xml/json/csv/fixed-length/edi/db/xlsx) found in this design"
        )
    } else {
        format!(
            "no importable {side} component (xml/json/csv/fixed-length/edi/db/xlsx) found; this design uses {} components, which ferrule cannot import yet",
            skipped_libraries.join("/")
        )
    })
}
