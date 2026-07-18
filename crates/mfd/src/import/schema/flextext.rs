use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use ir::{ScalarType, SchemaNode};
use mapping::{
    DelimitedDialect, DelimitedRecordField, FixedWidthRecordField, FlexCommand, FlexLineEnding,
    FlexTextLayout, FormatOptions, MAX_FLEXTEXT_LAYOUT_DEPTH, MAX_FLEXTEXT_LAYOUT_NODES,
    ManySplitter, OnceSplitter, StoreTrim, SwitchArm, TrimSide,
};

use super::{
    ComponentFormat, SchemaComponent, entry_key_sets, is_default_output, parse_u32, schema_node_at,
};

const MAX_MFT_BYTES: usize = 4 * 1024 * 1024;
const MAX_MFT_ELEMENTS: usize = 16_384;

pub(super) fn read(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
) -> Result<SchemaComponent, String> {
    let name = component.attribute("name").unwrap_or_default().to_string();
    let data = child(component, "data").ok_or_else(|| "component has no data".to_string())?;
    let text = child(&data, "text")
        .filter(|text| text.attribute("type") == Some("txt"))
        .ok_or_else(|| "component has no FlexText metadata".to_string())?;
    let config = text
        .attribute("config")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "component has no external configuration path".to_string())?;
    let config_path = resolve_config(mfd_path, config)?;
    let bytes = std::fs::read(&config_path)
        .map_err(|error| format!("could not read configuration `{config}` ({error})"))?;
    if bytes.len() > MAX_MFT_BYTES {
        return Err(format!(
            "configuration `{config}` exceeds the {MAX_MFT_BYTES}-byte limit"
        ));
    }
    let source = String::from_utf8(bytes)
        .map_err(|_| format!("configuration `{config}` is not valid UTF-8"))?;
    if source.contains("<!DOCTYPE") {
        return Err(format!(
            "configuration `{config}` uses a document type declaration"
        ));
    }
    let document = roxmltree::Document::parse(&source)
        .map_err(|error| format!("could not parse configuration `{config}` ({error})"))?;
    if document
        .descendants()
        .filter(|node| node.is_element())
        .count()
        > MAX_MFT_ELEMENTS
    {
        return Err(format!(
            "configuration `{config}` exceeds the {MAX_MFT_ELEMENTS}-element limit"
        ));
    }
    let parsed = parse_project(&document)?;
    let schema = parsed.layout.schema();
    let root =
        child(&data, "root").ok_or_else(|| "component has no visible entry tree".to_string())?;
    let visible = visible_root(&root, parsed.layout.root_name())?;

    let mut ports = BTreeMap::new();
    let mut output_count = 0_usize;
    let mut input_count = 0_usize;
    record_port(
        &visible,
        &[],
        &schema,
        &mut ports,
        &mut output_count,
        &mut input_count,
    )?;
    collect_ports(
        &visible,
        &mut Vec::new(),
        &schema,
        &mut ports,
        &mut output_count,
        &mut input_count,
    )?;
    let is_source = output_count >= input_count;
    let fallback = parsed
        .file_name
        .filter(|path| !path.trim().is_empty())
        .map(|path| rebase_instance_path(mfd_path, &config_path, path));
    let input_instance = text
        .attribute("inputinstance")
        .map(decode_value)
        .transpose()?
        .or_else(|| is_source.then(|| fallback.clone()).flatten());
    let output_instance = text
        .attribute("outputinstance")
        .map(decode_value)
        .transpose()?
        .or_else(|| (!is_source).then_some(fallback).flatten());
    let (input_keys, output_keys) = entry_key_sets(&root);

    Ok(SchemaComponent {
        name,
        format: ComponentFormat::FlexText,
        schema,
        input_instance,
        output_instance,
        options: FormatOptions {
            flextext: Some(parsed.layout),
            ..FormatOptions::default()
        },
        is_source,
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

struct ParsedProject {
    layout: FlexTextLayout,
    file_name: Option<String>,
}

fn parse_project(document: &roxmltree::Document<'_>) -> Result<ParsedProject, String> {
    let root = document.root_element();
    if !root.has_tag_name("FlexText") {
        return Err("configuration root is not <FlexText>".to_string());
    }
    let project = child(&root, "Commands")
        .and_then(|commands| only_element_child(&commands).ok())
        .filter(|project| project.has_tag_name("Project"))
        .ok_or_else(|| "configuration has no single <Commands>/<Project>".to_string())?;
    let root_name = value_child(&project, "RootName")?;
    let command = only_connected_command(&project)?;
    let mut state = ParseState::default();
    let command = parse_command(&command, 1, &mut state)?;
    let line_ending = match project.attribute("LineEnding") {
        None | Some("CRLF" | "crlf") => FlexLineEnding::Crlf,
        Some("LF" | "lf") => FlexLineEnding::Lf,
        Some(value) => return Err(format!("unsupported project line ending `{value}`")),
    };
    let write_bom = match project.attribute("ByteOrderMark") {
        None | Some("0") => false,
        Some("1") => true,
        Some(value) => return Err(format!("invalid project ByteOrderMark `{value}`")),
    };
    let layout = FlexTextLayout::new(root_name, command, line_ending, write_bom)
        .map_err(|error| format!("invalid command layout ({error})"))?;
    let file_name = project
        .attribute("FileName")
        .map(decode_value)
        .transpose()?;
    Ok(ParsedProject { layout, file_name })
}

#[derive(Default)]
struct ParseState {
    nodes: usize,
}

fn parse_command(
    node: &roxmltree::Node<'_, '_>,
    depth: usize,
    state: &mut ParseState,
) -> Result<FlexCommand, String> {
    if depth > MAX_FLEXTEXT_LAYOUT_DEPTH {
        return Err(format!(
            "command layout exceeds the {MAX_FLEXTEXT_LAYOUT_DEPTH}-level depth limit"
        ));
    }
    state.nodes = state
        .nodes
        .checked_add(1)
        .ok_or_else(|| "command node count overflowed".to_string())?;
    if state.nodes > MAX_FLEXTEXT_LAYOUT_NODES {
        return Err(format!(
            "command layout exceeds the {MAX_FLEXTEXT_LAYOUT_NODES}-node limit"
        ));
    }
    match node.tag_name().name() {
        "SplitSingle" => parse_split_once(node, depth, state),
        "SplitMultiple" => parse_split_many(node, depth, state),
        "Store" => parse_store(node),
        "Ignore" => Ok(FlexCommand::Ignore),
        "Switch" => parse_switch(node, depth, state),
        "FLF" => parse_fixed_records(node),
        "CSV" => parse_delimited_records(node),
        name => Err(format!("unsupported FlexText command <{name}>")),
    }
}

fn parse_split_once(
    node: &roxmltree::Node<'_, '_>,
    depth: usize,
    state: &mut ParseState,
) -> Result<FlexCommand, String> {
    let splitter = if node.attribute("Orientation") == Some("Vertical") {
        OnceSplitter::FixedColumns(column_width(node)?)
    } else {
        match (node.attribute("Mode"), node.attribute("Behavior")) {
            (Some("DynF"), _) => OnceSplitter::Delimiter(value_child(node, "Separator")?),
            (Some("DynL"), Some("LineBased")) => {
                OnceSplitter::LineContaining(value_child(node, "Separator")?)
            }
            (None | Some("Fix"), _) => OnceSplitter::FixedLines(nonzero_offset(node, "Upper")?),
            (mode, behavior) => {
                return Err(format!(
                    "unsupported SplitSingle mode `{}` behavior `{}`",
                    mode.unwrap_or(""),
                    behavior.unwrap_or("")
                ));
            }
        }
    };
    let commands = connected_commands(node)?;
    if commands.len() != 2 {
        return Err("SplitSingle must have exactly two connected commands".to_string());
    }
    Ok(FlexCommand::SplitOnce {
        name: value_child(node, "Name")?,
        splitter,
        first: Box::new(parse_command(&commands[0], depth + 1, state)?),
        second: Box::new(parse_command(&commands[1], depth + 1, state)?),
    })
}

fn parse_split_many(
    node: &roxmltree::Node<'_, '_>,
    depth: usize,
    state: &mut ParseState,
) -> Result<FlexCommand, String> {
    let splitter = match (node.attribute("Mode"), node.attribute("Behavior")) {
        (Some("Fix"), _) => ManySplitter::FixedLines(nonzero_attribute(node, "Offset")?),
        (Some("DynLS"), Some("LineStartsWith")) => {
            ManySplitter::LinesStartingWith(value_child(node, "Separator")?)
        }
        (mode, behavior) => {
            return Err(format!(
                "unsupported SplitMultiple mode `{}` behavior `{}`",
                mode.unwrap_or(""),
                behavior.unwrap_or("")
            ));
        }
    };
    let command = only_connected_command(node)?;
    Ok(FlexCommand::SplitMany {
        name: value_child(node, "Name")?,
        splitter,
        child: Box::new(parse_command(&command, depth + 1, state)?),
    })
}

fn parse_store(node: &roxmltree::Node<'_, '_>) -> Result<FlexCommand, String> {
    let trim = match node.attribute("TrimCharSet") {
        None | Some("") => None,
        Some(characters) => {
            let side = match node.attribute("TrimSide") {
                None | Some("Both") => TrimSide::Both,
                Some("Left") => TrimSide::Left,
                Some("Right") => TrimSide::Right,
                Some(value) => return Err(format!("unsupported Store trim side `{value}`")),
            };
            Some(
                StoreTrim::new(side, decode_value(characters)?)
                    .map_err(|error| format!("invalid Store trim characters ({error})"))?,
            )
        }
    };
    Ok(FlexCommand::store(
        value_child(node, "Name")?,
        scalar_type(node.attribute("Type"))?,
        trim,
    ))
}

fn parse_switch(
    node: &roxmltree::Node<'_, '_>,
    depth: usize,
    state: &mut ParseState,
) -> Result<FlexCommand, String> {
    if node.attribute("Mode") != Some("AllPossible") {
        return Err(format!(
            "unsupported Switch mode `{}`",
            node.attribute("Mode").unwrap_or("")
        ));
    }
    let conditions =
        child(node, "Conditions").ok_or_else(|| "Switch has no conditions".to_string())?;
    let mut arms = Vec::new();
    let mut default = None;
    for condition in conditions
        .children()
        .filter(|condition| condition.has_tag_name("Condition"))
    {
        let command = parse_command(&only_connected_command(&condition)?, depth + 1, state)?;
        match condition.attribute("Mode") {
            Some("Default") => {
                if default.replace(Box::new(command)).is_some() {
                    return Err("Switch has more than one default condition".to_string());
                }
            }
            None | Some("ContentStartsWith") => {
                let prefix = value_child(&condition, "Value")?;
                arms.push(
                    SwitchArm::new(prefix, command)
                        .map_err(|error| format!("invalid Switch condition ({error})"))?,
                );
            }
            Some(mode) => return Err(format!("unsupported Switch condition mode `{mode}`")),
        }
    }
    Ok(FlexCommand::Switch {
        name: value_child(node, "Name")?,
        arms,
        default,
    })
}

fn parse_fixed_records(node: &roxmltree::Node<'_, '_>) -> Result<FlexCommand, String> {
    let fields = record_fields(node, |field, name, ty| {
        let width = match field.attribute("Size") {
            None => NonZeroU32::new(1),
            Some(value) => value.parse::<u32>().ok().and_then(NonZeroU32::new),
        }
        .ok_or_else(|| format!("FLF field `{name}` has an invalid size"))?;
        FixedWidthRecordField::new(name, ty, width)
            .map_err(|error| format!("invalid FLF field ({error})"))
    })?;
    Ok(FlexCommand::FixedWidthRecords {
        name: value_child(node, "RecordName")?,
        fields,
    })
}

fn parse_delimited_records(node: &roxmltree::Node<'_, '_>) -> Result<FlexCommand, String> {
    let fields = record_fields(node, |_field, name, ty| {
        DelimitedRecordField::new(name, ty).map_err(|error| format!("invalid CSV field ({error})"))
    })?;
    let field_separator = value_child(node, "FieldSeparator")?;
    let record_separator = value_child(node, "RecordSeparator")?;
    let quote = single_character(
        node.attribute("QuoteCharacter").unwrap_or("\""),
        "CSV quote",
    )?;
    let escape = single_character(
        node.attribute("EscapeCharacter").unwrap_or("\""),
        "CSV escape",
    )?;
    let dialect = DelimitedDialect::new_with_field_separator(
        field_separator,
        record_separator,
        quote,
        escape,
    )
    .map_err(|error| format!("invalid CSV dialect ({error})"))?;
    Ok(FlexCommand::DelimitedRecords {
        name: value_child(node, "RecordName")?,
        dialect,
        fields,
    })
}

fn record_fields<T>(
    node: &roxmltree::Node<'_, '_>,
    mut build: impl FnMut(&roxmltree::Node<'_, '_>, String, ScalarType) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    let fields = child(node, "Fields").ok_or_else(|| "record has no fields".to_string())?;
    fields
        .children()
        .filter(|field| field.has_tag_name("Field"))
        .map(|field| {
            let name = value_child(&field, "Name")?;
            let ty = scalar_type(field.attribute("Type"))?;
            build(&field, name, ty)
        })
        .collect()
}

fn visible_root<'a, 'input>(
    root: &roxmltree::Node<'a, 'input>,
    expected: &str,
) -> Result<roxmltree::Node<'a, 'input>, String> {
    let mut entry = root
        .children()
        .find(|node| node.has_tag_name("entry"))
        .ok_or_else(|| "visible entry tree is empty".to_string())?;
    while matches!(entry.attribute("name"), Some("FileInstance" | "document")) {
        entry = entry
            .children()
            .find(|node| node.has_tag_name("entry"))
            .ok_or_else(|| "visible entry wrapper has no payload".to_string())?;
    }
    if entry.attribute("name") != Some(expected) {
        return Err(format!(
            "configuration root `{expected}` does not match visible root `{}`",
            entry.attribute("name").unwrap_or("")
        ));
    }
    Ok(entry)
}

fn collect_ports(
    entry: &roxmltree::Node<'_, '_>,
    path: &mut Vec<String>,
    schema: &SchemaNode,
    ports: &mut BTreeMap<u32, Vec<String>>,
    output_count: &mut usize,
    input_count: &mut usize,
) -> Result<(), String> {
    for child in entry.children().filter(|node| node.has_tag_name("entry")) {
        let name = child.attribute("name").unwrap_or_default().to_string();
        path.push(name);
        record_port(&child, path, schema, ports, output_count, input_count)?;
        collect_ports(&child, path, schema, ports, output_count, input_count)?;
        path.pop();
    }
    Ok(())
}

fn record_port(
    entry: &roxmltree::Node<'_, '_>,
    path: &[String],
    schema: &SchemaNode,
    ports: &mut BTreeMap<u32, Vec<String>>,
    output_count: &mut usize,
    input_count: &mut usize,
) -> Result<(), String> {
    if (entry.attribute("outkey").is_some() || entry.attribute("inpkey").is_some())
        && schema_node_at(schema, path).is_none()
    {
        return Err(format!(
            "visible port `{}` does not exist in the external command layout",
            path.join("/")
        ));
    }
    if let Some(key) = parse_u32(entry.attribute("outkey")) {
        ports.insert(key, path.to_vec());
        *output_count += 1;
    }
    if let Some(key) = parse_u32(entry.attribute("inpkey")) {
        ports.insert(key, path.to_vec());
        *input_count += 1;
    }
    Ok(())
}

fn resolve_config(mfd_path: &Path, relative: &str) -> Result<PathBuf, String> {
    let portable = relative.replace('\\', "/");
    let base = mfd_path.parent().unwrap_or_else(|| Path::new("."));
    let direct = base.join(&portable);
    if direct.is_file() {
        return Ok(direct);
    }
    let file_name = direct
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("configuration path `{relative}` has no file name"))?;
    let directory = direct.parent().unwrap_or(base);
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("could not resolve configuration `{relative}` ({error})"))?;
    let mut matches = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("could not resolve configuration `{relative}` ({error})"))?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(file_name))
            && entry
                .file_type()
                .map_err(|error| format!("could not inspect configuration `{relative}` ({error})"))?
                .is_file()
        {
            matches.push(entry.path());
        }
    }
    match matches.as_slice() {
        [path] => Ok(path.clone()),
        [] => Err(format!("configuration `{relative}` was not found")),
        _ => Err(format!(
            "configuration `{relative}` has multiple case-insensitive sibling matches"
        )),
    }
}

fn rebase_instance_path(mfd_path: &Path, config_path: &Path, file_name: String) -> String {
    let portable = file_name.replace('\\', "/");
    let file_path = Path::new(&portable);
    if file_path.is_absolute() {
        return portable;
    }
    let mfd_parent = mfd_path.parent().unwrap_or_else(|| Path::new("."));
    let config_parent = config_path.parent().unwrap_or(mfd_parent);
    let resolved = config_parent.join(file_path);
    resolved
        .strip_prefix(mfd_parent)
        .unwrap_or(&resolved)
        .to_string_lossy()
        .replace('\\', "/")
}

fn connected_commands<'a, 'input>(
    node: &roxmltree::Node<'a, 'input>,
) -> Result<Vec<roxmltree::Node<'a, 'input>>, String> {
    let Some(connections) = child(node, "Connections") else {
        return Ok(Vec::new());
    };
    let mut commands = Vec::new();
    for connection in connections
        .children()
        .filter(|node| node.has_tag_name("Connection"))
    {
        if connection.children().any(|child| child.is_element()) {
            commands.push(only_element_child(&connection)?);
        }
    }
    Ok(commands)
}

fn only_connected_command<'a, 'input>(
    node: &roxmltree::Node<'a, 'input>,
) -> Result<roxmltree::Node<'a, 'input>, String> {
    let commands = connected_commands(node)?;
    match commands.as_slice() {
        [command] => Ok(*command),
        _ => Err(format!(
            "{} must have exactly one connected command",
            node.tag_name().name()
        )),
    }
}

fn only_element_child<'a, 'input>(
    node: &roxmltree::Node<'a, 'input>,
) -> Result<roxmltree::Node<'a, 'input>, String> {
    let mut children = node.children().filter(|child| {
        child.is_element() && !matches!(child.tag_name().name(), "Version" | "XData" | "Functions")
    });
    let first = children
        .next()
        .ok_or_else(|| format!("<{}> has no command", node.tag_name().name()))?;
    if children.next().is_some() {
        return Err(format!(
            "<{}> has more than one command",
            node.tag_name().name()
        ));
    }
    Ok(first)
}

fn nonzero_offset(node: &roxmltree::Node<'_, '_>, child_name: &str) -> Result<NonZeroU32, String> {
    let offset = child(node, child_name)
        .and_then(|child| child.attribute("Offset"))
        .unwrap_or("1");
    offset
        .parse::<u32>()
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(|| {
            format!(
                "{} has an invalid {child_name} offset `{offset}`",
                node.tag_name().name()
            )
        })
}

fn column_width(node: &roxmltree::Node<'_, '_>) -> Result<NonZeroU32, String> {
    let upper = child(node, "Upper")
        .and_then(|child| child.attribute("Offset"))
        .unwrap_or("1")
        .parse::<u32>()
        .map_err(|_| "SplitSingle has an invalid Upper column offset".to_string())?;
    let lower = child(node, "Lower")
        .and_then(|child| child.attribute("Offset"))
        .ok_or_else(|| "vertical SplitSingle has no Lower column offset".to_string())?
        .parse::<u32>()
        .map_err(|_| "SplitSingle has an invalid Lower column offset".to_string())?;
    lower
        .checked_sub(upper)
        .and_then(|width| width.checked_add(1))
        .and_then(NonZeroU32::new)
        .ok_or_else(|| {
            "vertical SplitSingle Lower offset must not precede Upper offset".to_string()
        })
}

fn nonzero_attribute(
    node: &roxmltree::Node<'_, '_>,
    attribute: &str,
) -> Result<NonZeroU32, String> {
    let value = node.attribute(attribute).unwrap_or("1");
    value
        .parse::<u32>()
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(|| {
            format!(
                "{} has an invalid {attribute} `{value}`",
                node.tag_name().name()
            )
        })
}

fn scalar_type(value: Option<&str>) -> Result<ScalarType, String> {
    match value.unwrap_or("string") {
        "string" => Ok(ScalarType::String),
        "integer" | "int" => Ok(ScalarType::Int),
        "number" | "decimal" | "double" | "float" => Ok(ScalarType::Float),
        "boolean" => Ok(ScalarType::Bool),
        value => Err(format!("unsupported scalar type `{value}`")),
    }
}

fn single_character(value: &str, kind: &str) -> Result<char, String> {
    let mut characters = value.chars();
    match (characters.next(), characters.next()) {
        (Some(character), None) => Ok(character),
        _ => Err(format!("{kind} separator must be one character")),
    }
}

fn value_child(node: &roxmltree::Node<'_, '_>, name: &str) -> Result<String, String> {
    let value = child(node, name)
        .and_then(|child| child.attribute("Value"))
        .ok_or_else(|| format!("{} has no {name} value", node.tag_name().name()))?;
    decode_value(value)
}

fn decode_value(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let Some(pair) = bytes.get(index + 1..index + 3) else {
                return Err(format!("invalid percent escape in `{value}`"));
            };
            let high =
                hex(pair[0]).ok_or_else(|| format!("invalid percent escape in `{value}`"))?;
            let low = hex(pair[1]).ok_or_else(|| format!("invalid percent escape in `{value}`"))?;
            decoded.push(high * 16 + low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| format!("percent-decoded value `{value}` is not UTF-8"))
}

const fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn child<'a, 'input>(
    node: &roxmltree::Node<'a, 'input>,
    name: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    node.children().find(|child| child.has_tag_name(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{Instance, Value};

    #[test]
    fn parses_line_delimiter_and_column_splitters() {
        let xml = r#"<FlexText><Commands><Project FileName="input.txt"><RootName Value="Root"/><Connections><Connection><SplitSingle Mode="DynL" Behavior="LineBased"><Separator Value="ITEM"/><Name Value="document"/><Connections><Connection><Store Type="string"><Name Value="header"/></Store></Connection><Connection><SplitSingle Mode="DynF"><Separator Value=":"/><Name Value="pair"/><Connections><Connection><Ignore/></Connection><Connection><SplitSingle Orientation="Vertical"><Upper Offset="1"/><Lower Offset="2"/><Name Value="columns"/><Connections><Connection><Ignore/></Connection><Connection><Store Type="integer"><Name Value="value"/></Store></Connection></Connections></SplitSingle></Connection></Connections></SplitSingle></Connection></Connections></SplitSingle></Connection></Connections></Project></Commands></FlexText>"#;
        let document = roxmltree::Document::parse(xml).unwrap();
        let parsed = parse_project(&document).unwrap();
        assert_eq!(parsed.layout.root_name(), "Root");
        assert_eq!(parsed.file_name.as_deref(), Some("input.txt"));
        let FlexCommand::SplitOnce {
            splitter, second, ..
        } = parsed.layout.command()
        else {
            panic!("root command should split once");
        };
        assert_eq!(splitter, &OnceSplitter::LineContaining("ITEM".into()));
        let FlexCommand::SplitOnce {
            splitter, second, ..
        } = second.as_ref()
        else {
            panic!("second command should split on a delimiter");
        };
        assert_eq!(splitter, &OnceSplitter::Delimiter(":".into()));
        let FlexCommand::SplitOnce { splitter, .. } = second.as_ref() else {
            panic!("nested command should split columns");
        };
        assert_eq!(
            splitter,
            &OnceSplitter::FixedColumns(NonZeroU32::new(2).unwrap())
        );
    }

    #[test]
    fn vertical_offsets_are_inclusive_one_based_columns() {
        let xml = r#"<FlexText><Commands><Project><RootName Value="Root"/><Connections><Connection>
          <SplitSingle Orientation="Vertical"><Upper Offset="1"/><Lower Offset="17"/><Name Value="CompanyInfo"/><Connections>
            <Connection><Ignore/></Connection>
            <Connection><Store Type="string" TrimSide="Right" TrimCharSet="%0D%0A"><Name Value="Company"/></Store></Connection>
          </Connections></SplitSingle>
        </Connection></Connections></Project></Commands></FlexText>"#;
        let document = roxmltree::Document::parse(xml).unwrap();
        let parsed = parse_project(&document).unwrap();
        let schema = parsed.layout.schema();
        let instance = format_flextext::from_str(
            "Company:         Nanonull Inc.\r\n",
            &schema,
            &parsed.layout,
        )
        .unwrap();

        assert_eq!(
            instance
                .field("CompanyInfo")
                .and_then(|group| group.field("Company"))
                .and_then(Instance::as_scalar),
            Some(&Value::String("Nanonull Inc.".into()))
        );
    }

    #[test]
    fn percent_decoder_rejects_malformed_and_non_utf8_values() {
        assert_eq!(decode_value("A%20B%0D%0A"), Ok("A B\r\n".into()));
        assert!(decode_value("bad%2").is_err());
        assert!(decode_value("%ff").is_err());
    }

    #[test]
    fn visible_ports_must_exist_in_the_layout_schema() {
        let schema = SchemaNode::group(
            "Root",
            vec![SchemaNode::scalar("known", ScalarType::String)],
        );
        let document = roxmltree::Document::parse(
            r#"<root><entry name="Root"><entry name="missing" outkey="1"/></entry></root>"#,
        )
        .unwrap();
        let visible = document.root_element().first_element_child().unwrap();
        let error = record_port(
            &visible.first_element_child().unwrap(),
            &["missing".into()],
            &schema,
            &mut BTreeMap::new(),
            &mut 0,
            &mut 0,
        )
        .unwrap_err();
        assert!(error.contains("does not exist"));
    }
}
