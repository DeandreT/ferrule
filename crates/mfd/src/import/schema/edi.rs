use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use ir::{SchemaKind, SchemaNode};
use mapping::{EdiBoundaryKind, FormatOptions, X12Separators};

use super::{ComponentFormat, SchemaComponent};

pub(super) fn read(
    component: &roxmltree::Node,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
    runtime_boundary: bool,
) -> Option<SchemaComponent> {
    let name = component.attribute("name").unwrap_or_default().to_string();
    let data = component
        .children()
        .find(|node| node.has_tag_name("data"))?;
    let text = data
        .children()
        .find(|node| node.has_tag_name("text") && node.attribute("type") == Some("edi"))?;
    let kind = text.attribute("kind").unwrap_or_default();
    let x12_separators = (kind == "EDIX12")
        .then(|| read_x12_separators(&text, &name, warnings))
        .flatten();
    let x12_interchange_version = (kind == "EDIX12")
        .then(|| read_x12_interchange_version(&text, &name, warnings))
        .flatten();
    let root = data.children().find(|node| node.has_tag_name("root"))?;
    let mut entry = root.children().find(|node| node.has_tag_name("entry"))?;
    while matches!(
        entry.attribute("name"),
        Some("FileInstance") | Some("document")
    ) {
        entry = entry.children().find(|node| node.has_tag_name("entry"))?;
    }

    let embedded_schema = typed_entry_tree_schema(&entry, true);
    let has_embedded_schema = embedded_schema.is_some();
    let mut fallback_schema = embedded_schema.or_else(|| entry_tree_schema(&entry, true, false))?;
    if !has_embedded_schema {
        fallback_schema.name = match kind {
            "EDIX12" => "MFD-X12",
            "EDIFACT" => "MFD-EDIFACT",
            "EDIHL7" => "HL7",
            "EDIFIXED" => "IDOC",
            "SWIFTMT" => "SWIFT",
            "EDITRADACOMS" => "TRADACOMS",
            _ if kind.is_empty() => "EDI",
            other => other,
        }
        .to_string();
    }

    let selected_messages = text
        .children()
        .find(|node| node.has_tag_name("messages"))
        .into_iter()
        .flat_map(|messages| messages.children())
        .filter(|node| node.has_tag_name("message"))
        .filter_map(|node| node.attribute("type"))
        .map(str::to_string)
        .collect::<Vec<_>>();
    let (embedded_idoc, embedded_swift_mt) = embedded_runtime_layout(&text, kind, &name, warnings);
    let config = text.attribute("config");
    let compiled = if runtime_boundary
        && matches!(
            kind,
            "EDIX12" | "EDIFACT" | "EDIHL7" | "EDITRADACOMS" | "EDIFIXED" | "SWIFTMT"
        ) {
        config.and_then(|declared| {
            match resolve_config(mfd_path, declared).and_then(|path| match kind {
                "EDIFIXED" => format_edi::config::idoc::import_config(&path)
                    .map(|compiled| (compiled.schema, Some(compiled.layout), None))
                    .map_err(|error| error.to_string()),
                "SWIFTMT" => format_edi::config::swift::import_config(&path, &selected_messages)
                    .map(|compiled| (compiled.schema, None, Some(compiled.layout)))
                    .map_err(|error| error.to_string()),
                _ => format_edi::config::import_config(&path, &selected_messages)
                    .map(|schema| (schema, None, None))
                    .map_err(|error| error.to_string()),
            }) {
                Ok(compiled) => Some(compiled),
                Err(error) => {
                    if runtime_boundary {
                        warnings.push(format!(
                            "EDI component `{name}` could not compile external configuration \
                         `{declared}` ({error}); its mapping graph was imported, but execution is \
                         disabled until a complete EDI schema is supplied"
                        ));
                    }
                    None
                }
            }
        })
    } else {
        None
    };
    let has_compiled_schema = compiled.is_some();
    let (mut schema, idoc, swift_mt) =
        compiled.unwrap_or((fallback_schema, embedded_idoc, embedded_swift_mt));
    if has_compiled_schema && kind == "EDIX12" {
        merge_parser_error_entries(&entry, &mut schema);
    }

    let mut ports = BTreeMap::new();
    let mut out_count = 0usize;
    let mut in_count = 0usize;
    let (input_keys, output_keys) = super::entry_key_sets(&root);
    super::record_entry_keys(&entry, &[], &mut ports, &mut out_count, &mut in_count);
    super::collect_entry_ports(
        &entry,
        &mut Vec::new(),
        &mut ports,
        &mut out_count,
        &mut in_count,
    );
    if out_count == 0 && in_count == 0 {
        warnings.push(format!("component `{name}` has no connected ports"));
    }

    if runtime_boundary && config.is_none() && !has_embedded_schema {
        warnings.push(format!(
            "EDI component `{name}` uses an entry-tree schema inferred without an external `{kind}` \
             configuration; its mapping graph was imported, but execution is disabled until a schema \
             with element positions, scalar types, fixed qualifiers, and cardinalities is supplied"
        ));
    }
    if runtime_boundary
        && !matches!(
            kind,
            "EDIX12" | "EDIFACT" | "EDIHL7" | "EDITRADACOMS" | "EDIFIXED" | "SWIFTMT"
        )
    {
        warnings.push(format!(
            "EDI component `{name}` uses runtime dialect `{kind}`; its mapping graph was imported, \
             but ferrule currently executes only EDIX12, EDIFACT, EDIHL7, EDITRADACOMS, EDIFIXED, and SWIFTMT instances"
        ));
    }

    let nested_instance = |role| {
        root.descendants()
            .find(|node| node.has_tag_name("file") && node.attribute("role") == Some(role))
            .and_then(|node| node.attribute("name"))
            .map(str::to_string)
    };

    Some(SchemaComponent {
        name,
        format: ComponentFormat::Edi,
        schema,
        input_instance: text
            .attribute("inputinstance")
            .map(str::to_string)
            .or_else(|| nested_instance("inputinstance")),
        output_instance: text
            .attribute("outputinstance")
            .map(str::to_string)
            .or_else(|| nested_instance("outputinstance")),
        options: FormatOptions {
            lenient_segments: true,
            edi_kind: match kind {
                "EDIX12" => Some(EdiBoundaryKind::X12),
                "EDIFACT" => Some(EdiBoundaryKind::Edifact),
                "EDIHL7" => Some(EdiBoundaryKind::Hl7),
                "EDITRADACOMS" => Some(EdiBoundaryKind::Tradacoms),
                "EDIFIXED" => Some(EdiBoundaryKind::Idoc),
                "SWIFTMT" => Some(EdiBoundaryKind::SwiftMt),
                _ => None,
            },
            x12_separators,
            x12_interchange_version,
            idoc,
            swift_mt,
            ..FormatOptions::default()
        },
        is_source: out_count >= in_count,
        is_default_output: super::is_default_output(component),
        is_variable: false,
        is_pass_through: false,
        compute_when_key: None,
        ports,
        input_keys,
        output_keys,
        input_ancestors: BTreeMap::new(),
        db_queries: Vec::new(),
        dynamic_json: None,
    })
}

fn read_x12_interchange_version(
    text: &roxmltree::Node<'_, '_>,
    component_name: &str,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let value = text
        .children()
        .find(|node| node.has_tag_name("settings"))?
        .attribute("interchangecontrolversionnumber")?;
    if value.len() == 5 && value.bytes().all(|byte| byte.is_ascii_digit()) {
        Some(value.to_string())
    } else {
        warnings.push(format!(
            "EDI component `{component_name}` has invalid X12 interchange version `{value}`; \
             ISA12 must be mapped explicitly for runtime output"
        ));
        None
    }
}

fn read_x12_separators(
    text: &roxmltree::Node<'_, '_>,
    component_name: &str,
    warnings: &mut Vec<String>,
) -> Option<X12Separators> {
    let separators = text
        .children()
        .find(|node| node.has_tag_name("settings"))?
        .children()
        .find(|node| node.has_tag_name("separators"))?;
    let required = |attribute: &'static str| -> Result<char, String> {
        one_separator(separators.attribute(attribute))
            .ok_or_else(|| format!("`{attribute}` must contain exactly one visible character"))
    };
    let parsed = (|| {
        let syntax = X12Separators {
            element: required("dataelement")?,
            component: required("component")?,
            segment: required("segment")?,
            repetition: optional_separator(separators.attribute("repetition"))?,
            release: optional_separator(separators.attribute("escape"))?,
        };
        let mut characters = vec![syntax.element, syntax.component, syntax.segment];
        characters.extend(syntax.repetition);
        characters.extend(syntax.release);
        if characters.iter().any(|character| {
            character.is_alphanumeric() || character.is_control() || character.is_whitespace()
        }) {
            return Err("separator characters must be non-alphanumeric and visible".to_string());
        }
        characters.sort_unstable();
        if characters.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err("separator characters must be distinct".to_string());
        }
        Ok(syntax)
    })();
    match parsed {
        Ok(syntax) => Some(syntax),
        Err(reason) => {
            warnings.push(format!(
                "EDI component `{component_name}` has invalid X12 separator settings ({reason}); \
                 the mapping was retained with standard runtime separators"
            ));
            None
        }
    }
}

fn one_separator(value: Option<&str>) -> Option<char> {
    let value = value?;
    if let Some(encoded) = value.strip_prefix('%')
        && encoded.len() == 2
        && let Ok(byte) = u8::from_str_radix(encoded, 16)
    {
        return Some(char::from(byte));
    }
    let mut characters = value.chars();
    let character = characters.next()?;
    characters.next().is_none().then_some(character)
}

fn optional_separator(value: Option<&str>) -> Result<Option<char>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() || value == "%20" || value == " " {
        return Ok(None);
    }
    one_separator(Some(value))
        .map(Some)
        .ok_or_else(|| "optional separator must contain zero or one visible character".to_string())
}

fn embedded_runtime_layout(
    text: &roxmltree::Node<'_, '_>,
    component_kind: &str,
    component_name: &str,
    warnings: &mut Vec<String>,
) -> (Option<mapping::IdocLayout>, Option<mapping::SwiftMtLayout>) {
    let Some(layout) = text
        .children()
        .find(|node| node.has_tag_name("ferrule-layout"))
    else {
        return (None, None);
    };
    let declared_kind = layout.attribute("kind").unwrap_or_default();
    let contents = layout.text().unwrap_or_default();
    match (component_kind, declared_kind) {
        ("EDIFIXED", "idoc") => match serde_json::from_str(contents) {
            Ok(layout) => (Some(layout), None),
            Err(error) => {
                warnings.push(format!(
                    "EDI component `{component_name}` has invalid embedded IDoc layout metadata \
                     ({error}); its typed entry tree was retained, but IDoc execution is disabled"
                ));
                (None, None)
            }
        },
        ("SWIFTMT", "swift_mt") => match serde_json::from_str(contents) {
            Ok(layout) => (None, Some(layout)),
            Err(error) => {
                warnings.push(format!(
                    "EDI component `{component_name}` has invalid embedded SWIFT MT layout metadata \
                     ({error}); its typed entry tree was retained, but SWIFT execution is disabled"
                ));
                (None, None)
            }
        },
        _ => {
            warnings.push(format!(
                "EDI component `{component_name}` has embedded `{declared_kind}` layout metadata \
                 that does not match component dialect `{component_kind}`; the layout was ignored"
            ));
            (None, None)
        }
    }
}

/// Reads ferrule's portable, self-describing EDI entry metadata. The typed
/// form is accepted only when every node declares cardinality and every leaf
/// has a supported scalar type, so ordinary vendor entry-tree fallbacks keep
/// their existing warning and conservative string shape.
fn typed_entry_tree_schema(entry: &roxmltree::Node<'_, '_>, is_root: bool) -> Option<SchemaNode> {
    let name = entry.attribute("name")?;
    if name.is_empty() {
        return None;
    }
    let repeating = match entry.attribute("ferrule-repeating")? {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    if is_root && repeating {
        return None;
    }
    let entries = entry
        .children()
        .filter(|node| node.has_tag_name("entry"))
        .collect::<Vec<_>>();
    let declared_kind = entry.attribute("ferrule-kind");
    let mut node = match (declared_kind, entries.is_empty()) {
        (Some("group"), _) | (None, false) => {
            if entry.attribute("datatype").is_some() {
                return None;
            }
            SchemaNode::group(
                name,
                entries
                    .iter()
                    .map(|child| typed_entry_tree_schema(child, false))
                    .collect::<Option<Vec<_>>>()?,
            )
        }
        (Some("scalar") | None, true) => {
            let ty = match entry.attribute("datatype")? {
                "string" => ir::ScalarType::String,
                "integer" => ir::ScalarType::Int,
                "decimal" => ir::ScalarType::Float,
                "boolean" => ir::ScalarType::Bool,
                _ => return None,
            };
            SchemaNode::scalar(name, ty)
        }
        _ => return None,
    };
    node.repeating = repeating;
    node.fixed = entry.attribute("ferrule-fixed").map(str::to_string);
    Some(node)
}

/// MapForce exposes parser-generated acknowledgement details as virtual X12
/// entry branches. They are not part of the message configuration, but their
/// connected scalar ports still need schema identities for graph validation.
fn merge_parser_error_entries(entry: &roxmltree::Node, schema: &mut SchemaNode) {
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        return;
    };
    let parent_is_segment = entry.attribute("name").is_some_and(is_inferred_segment);
    for child_entry in entry.children().filter(|node| node.has_tag_name("entry")) {
        let name = child_entry.attribute("name").unwrap_or_default();
        if name.starts_with("ParserErrors_") {
            if children.iter().all(|child| child.name != name)
                && let Some(parser_errors) =
                    entry_tree_schema(&child_entry, false, parent_is_segment)
            {
                children.push(parser_errors);
            }
            continue;
        }
        if let Some(child_schema) = children.iter_mut().find(|child| child.name == name) {
            merge_parser_error_entries(&child_entry, child_schema);
        }
    }
}

fn resolve_config(mfd_path: &Path, declared: &str) -> Result<PathBuf, String> {
    let portable = declared
        .strip_prefix("altova://edi_config/")
        .unwrap_or(declared)
        .replace('\\', "/");
    let relative = Path::new(&portable);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(format!(
            "configuration path `{declared}` is not a bounded relative path"
        ));
    }

    let unresolved_base = mfd_path.parent().unwrap_or_else(|| Path::new("."));
    let base = std::fs::canonicalize(unresolved_base)
        .map_err(|error| format!("could not resolve mapping directory ({error})"))?;
    let mut roots = vec![base.to_path_buf()];
    if let Some(root) = std::env::var_os("FERRULE_EDI_CONFIG_DIR") {
        roots.push(PathBuf::from(root));
    }
    for ancestor in base.ancestors().take(12) {
        roots.push(ancestor.to_path_buf());
        roots.push(ancestor.join("MapForceEDI"));
        if let Ok(entries) = std::fs::read_dir(ancestor) {
            roots.extend(
                entries
                    .take(128)
                    .filter_map(Result::ok)
                    .filter_map(|entry| {
                        entry
                            .file_type()
                            .ok()
                            .filter(|file_type| file_type.is_dir())
                            .map(|_| entry)
                    })
                    .map(|entry| entry.path().join("MapForceEDI")),
            );
        }
    }
    let mut matches = roots
        .into_iter()
        .filter_map(|root| resolve_case_insensitive(&root, relative))
        .filter_map(|path| std::fs::canonicalize(path).ok())
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();
    match matches.as_slice() {
        [path] => Ok(path.clone()),
        [] => Err(format!("configuration `{declared}` was not found")),
        _ => Err(format!(
            "configuration `{declared}` resolves to multiple nearby installations"
        )),
    }
}

fn resolve_case_insensitive(base: &Path, relative: &Path) -> Option<PathBuf> {
    let mut current = base.to_path_buf();
    for component in relative.components() {
        let Component::Normal(expected) = component else {
            continue;
        };
        let direct = current.join(expected);
        if direct.exists() {
            current = direct;
            continue;
        }
        let expected = expected.to_str()?;
        let mut matches = std::fs::read_dir(&current)
            .ok()?
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case(expected))
            })
            .map(|entry| entry.path());
        let found = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        current = found;
    }
    current.is_file().then_some(current)
}

fn entry_tree_schema(
    entry: &roxmltree::Node,
    is_root: bool,
    parent_is_segment: bool,
) -> Option<SchemaNode> {
    let name = entry.attribute("name").unwrap_or("EDI");
    let is_segment = is_inferred_segment(name);
    let child_entries = entry
        .children()
        .filter(|node| node.has_tag_name("entry"))
        .collect::<Vec<_>>();
    let children = child_entries
        .iter()
        .filter_map(|child| entry_tree_schema(child, false, is_segment))
        .collect::<Vec<_>>();
    let connected = entry.attribute("inpkey").is_some() || entry.attribute("outkey").is_some();

    if child_entries.is_empty() {
        return connected.then(|| SchemaNode::scalar(name, ir::ScalarType::String));
    }
    if children.is_empty() && !connected && !is_root {
        return None;
    }

    let node = SchemaNode::group(name, children);
    if !is_root && !parent_is_segment && (connected || !is_segment) {
        Some(node.repeating())
    } else {
        Some(node)
    }
}

fn is_inferred_segment(name: &str) -> bool {
    let name = name.strip_prefix("MF_").unwrap_or(name);
    (2..=3).contains(&name.len())
        && name
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_uppercase())
        && name
            .chars()
            .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit())
        && !name.strip_prefix("SG").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        })
}
