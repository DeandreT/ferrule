use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, SchemaNode};
use mapping::{FixedFieldWidth, FixedWidthLayout, FormatOptions};

use super::csv::select_block;
use super::{ComponentFormat, SchemaComponent, entry_key_sets, is_default_output, parse_u32};

/// Reads an inline MapForce fixed-length text component. Field widths are
/// positional, like ferrule's flat-file runtime; the visible entry tree owns
/// graph ports while `<names>` owns field names and scalar types.
pub(super) fn read(
    component: &roxmltree::Node<'_, '_>,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    let name = component.attribute("name").unwrap_or_default().to_string();
    let data = component
        .children()
        .find(|node| node.has_tag_name("data"))?;
    let text = data
        .children()
        .find(|node| node.has_tag_name("text") && node.attribute("type") == Some("flf"))?;
    let Some(settings) = text.children().find(|node| node.has_tag_name("settings")) else {
        warnings.push(format!(
            "fixed-length component `{name}` has no inline settings"
        ));
        return None;
    };
    let Some(names) = settings.children().find(|node| node.has_tag_name("names")) else {
        warnings.push(format!(
            "fixed-length component `{name}` has no inline field declarations"
        ));
        return None;
    };

    let root = data.children().find(|node| node.has_tag_name("root"))?;
    let configured_block = names.attribute("block");
    let block = select_block(root, configured_block, &name, "fixed-length", warnings)?;
    let mut declarations = names
        .children()
        .filter(|node| node.is_element() && node.tag_name().name().starts_with("field"))
        .map(|node| {
            node.tag_name()
                .name()
                .strip_prefix("field")
                .and_then(|suffix| suffix.parse::<usize>().ok())
                .map(|index| (index, node))
        })
        .collect::<Option<Vec<_>>>();
    let Some(mut declarations) = declarations.take() else {
        warnings.push(format!(
            "fixed-length component `{name}` has a field declaration without a numeric suffix; skipped"
        ));
        return None;
    };
    declarations.sort_by_key(|(index, _)| *index);
    if declarations.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        warnings.push(format!(
            "fixed-length component `{name}` has duplicate field declaration indexes; skipped"
        ));
        return None;
    }

    let mut schema_fields = Vec::new();
    let mut widths = Vec::new();
    let mut field_names = BTreeSet::new();
    for (_, field) in declarations {
        let field_name = field.attribute("name").unwrap_or_default();
        if field_name.is_empty() || !field_names.insert(field_name.to_string()) {
            warnings.push(format!(
                "fixed-length component `{name}` has an empty or duplicate field name; skipped"
            ));
            return None;
        }
        let Some(width) = field
            .attribute("length")
            .and_then(|length| length.parse::<u32>().ok())
            .and_then(FixedFieldWidth::new)
        else {
            warnings.push(format!(
                "fixed-length component `{name}` field `{field_name}` has a missing or invalid positive length; skipped"
            ));
            return None;
        };
        widths.push(width);
        schema_fields.push(SchemaNode::scalar(
            field_name,
            scalar_type(field.attribute("type")),
        ));
    }
    if schema_fields.is_empty() {
        warnings.push(format!(
            "fixed-length component `{name}` declares no fields; skipped"
        ));
        return None;
    }

    let fill_char = match settings.attribute("fillchar") {
        Some(value) => {
            let mut characters = value.chars();
            let Some(character) = characters.next() else {
                warnings.push(format!(
                    "fixed-length component `{name}` declares an empty fill character; skipped"
                ));
                return None;
            };
            if characters.next().is_some() {
                warnings.push(format!(
                    "fixed-length component `{name}` declares a multi-character fill value `{value}`; skipped"
                ));
                return None;
            }
            character
        }
        None => ' ',
    };
    let record_delimiters = bool_setting(settings.attribute("delimiter"), true, &name, warnings);
    let treat_empty_as_absent =
        bool_setting(settings.attribute("removeempty"), true, &name, warnings);
    let layout =
        match FixedWidthLayout::new(widths, fill_char, record_delimiters, treat_empty_as_absent) {
            Ok(layout) => layout,
            Err(error) => {
                warnings.push(format!(
                    "fixed-length component `{name}` has an invalid layout ({error}); skipped"
                ));
                return None;
            }
        };

    if text
        .attribute("encoding")
        .is_some_and(|encoding| encoding != "1000")
    {
        warnings.push(format!(
            "fixed-length component `{name}` declares a non-UTF-8 encoding; ferrule fixed-width I/O assumes UTF-8"
        ));
    }

    let mut ports = BTreeMap::new();
    let mut output_count = 0_usize;
    let mut input_count = 0_usize;
    record_port(&block, &[], &mut ports, &mut output_count, &mut input_count);
    let entry_fields = block
        .children()
        .filter(|node| node.has_tag_name("entry"))
        .collect::<Vec<_>>();
    let declared_names = schema_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<BTreeSet<_>>();
    let entry_names = entry_fields
        .iter()
        .map(|field| field.attribute("name").unwrap_or_default())
        .collect::<BTreeSet<_>>();
    if entry_fields.len() != schema_fields.len() || entry_names != declared_names {
        warnings.push(format!(
            "fixed-length component `{name}` entry fields do not match its inline declarations; skipped"
        ));
        return None;
    }
    for field_name in schema_fields.iter().map(|field| field.name.as_str()) {
        let field = entry_fields
            .iter()
            .find(|entry| entry.attribute("name") == Some(field_name))?;
        record_port(
            field,
            &[field_name.to_string()],
            &mut ports,
            &mut output_count,
            &mut input_count,
        );
    }
    if output_count == 0 && input_count == 0 {
        warnings.push(format!("component `{name}` has no connected ports"));
    }
    let (input_keys, output_keys) = entry_key_sets(&root);
    let root_name = names
        .attribute("root")
        .filter(|root| !root.is_empty())
        .unwrap_or(&name)
        .to_string();

    Some(SchemaComponent {
        name,
        format: ComponentFormat::Csv,
        schema: SchemaNode::group(root_name, schema_fields),
        input_instance: text.attribute("inputinstance").map(str::to_string),
        output_instance: text.attribute("outputinstance").map(str::to_string),
        options: FormatOptions {
            fixed_width: Some(layout),
            ..FormatOptions::default()
        },
        is_source: output_count >= input_count,
        is_default_output: is_default_output(component),
        is_variable: false,
        is_pass_through: false,
        compute_when_key: None,
        ports,
        input_ancestors: BTreeMap::new(),
        input_keys,
        output_keys,
        db_queries: Vec::new(),
        dynamic_json: None,
    })
}

fn scalar_type(value: Option<&str>) -> ScalarType {
    match value {
        Some("number") | Some("decimal") | Some("double") | Some("float") => ScalarType::Float,
        Some("integer") | Some("int") => ScalarType::Int,
        Some("boolean") => ScalarType::Bool,
        _ => ScalarType::String,
    }
}

fn bool_setting(
    value: Option<&str>,
    default: bool,
    component_name: &str,
    warnings: &mut Vec<String>,
) -> bool {
    match value {
        None => default,
        Some("true" | "1") => true,
        Some("false" | "0") => false,
        Some(value) => {
            warnings.push(format!(
                "fixed-length component `{component_name}` uses unrecognized boolean setting `{value}`; defaulted to `{default}`"
            ));
            default
        }
    }
}

fn record_port(
    entry: &roxmltree::Node<'_, '_>,
    path: &[String],
    ports: &mut BTreeMap<u32, Vec<String>>,
    output_count: &mut usize,
    input_count: &mut usize,
) {
    if let Some(key) = parse_u32(entry.attribute("outkey")) {
        *output_count += 1;
        ports.insert(key, path.to_vec());
    }
    if let Some(key) = parse_u32(entry.attribute("inpkey")) {
        *input_count += 1;
        ports.insert(key, path.to_vec());
    }
}
