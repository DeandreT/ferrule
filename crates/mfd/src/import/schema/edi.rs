use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use ir::SchemaNode;
use mapping::FormatOptions;

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
    let root = data.children().find(|node| node.has_tag_name("root"))?;
    let mut entry = root.children().find(|node| node.has_tag_name("entry"))?;
    while matches!(
        entry.attribute("name"),
        Some("FileInstance") | Some("document")
    ) {
        entry = entry.children().find(|node| node.has_tag_name("entry"))?;
    }

    let mut fallback_schema = entry_tree_schema(&entry, true, false)?;
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

    let selected_messages = text
        .children()
        .find(|node| node.has_tag_name("messages"))
        .into_iter()
        .flat_map(|messages| messages.children())
        .filter(|node| node.has_tag_name("message"))
        .filter_map(|node| node.attribute("type"))
        .map(str::to_string)
        .collect::<Vec<_>>();
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
    let (schema, idoc, swift_mt) = compiled.unwrap_or((fallback_schema, None, None));

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

    if runtime_boundary && config.is_none() {
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

    Some(SchemaComponent {
        name,
        format: ComponentFormat::Edi,
        schema,
        input_instance: text.attribute("inputinstance").map(str::to_string),
        output_instance: text.attribute("outputinstance").map(str::to_string),
        options: FormatOptions {
            lenient_segments: true,
            idoc,
            swift_mt,
            ..FormatOptions::default()
        },
        is_source: out_count >= in_count,
        is_default_output: super::is_default_output(component),
        is_variable: false,
        compute_when_key: None,
        ports,
        input_keys,
        output_keys,
        input_ancestors: BTreeMap::new(),
        db_queries: Vec::new(),
        dynamic_json: None,
    })
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
