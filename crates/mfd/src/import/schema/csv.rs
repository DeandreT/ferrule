use std::collections::BTreeSet;

use ir::{ScalarType, SchemaNode};
use mapping::{CsvTextRepairCause, CsvTextRepairDependency, FormatOptions};

use super::parse_u32;

const CSV_SINGLETON_BEFORE: &str = "\u{1f}ferrule-csv-singleton-before";
const CSV_SINGLETON_AFTER: &str = "\u{1f}ferrule-csv-singleton-after";

fn diagnostic_component_name(name: &str) -> &str {
    if name.len() <= 32 {
        return name;
    }
    let end = name
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= 32)
        .last()
        .unwrap_or(0);
    &name[..end]
}

pub(super) fn read_text_settings(
    text: &roxmltree::Node<'_, '_>,
    component_name: &str,
    warnings: &mut Vec<String>,
) -> (bool, Option<CsvTextRepairDependency>) {
    let component_name = diagnostic_component_name(component_name);
    let mut repair = None;
    if text
        .attribute("encoding")
        .is_some_and(|encoding| encoding != "1000")
    {
        warnings.push(format!(
            "csv component `{component_name}` declares an unsupported text encoding; \
             only UTF-8 encoding code 1000 is supported; retained as a repair draft"
        ));
        add_cause(&mut repair, CsvTextRepairCause::Encoding);
    }
    if text
        .attribute("byteorder")
        .is_some_and(|byteorder| byteorder != "1")
    {
        warnings.push(format!(
            "csv component `{component_name}` declares an unsupported byte order code; \
             only absent or code 1 is supported; retained as a repair draft"
        ));
        add_cause(&mut repair, CsvTextRepairCause::ByteOrder);
    }
    let utf8_bom = match text.attribute("byteordermark") {
        None | Some("0") => false,
        Some("1") => true,
        Some(_) => {
            warnings.push(format!(
                "csv component `{component_name}` declares an unsupported byte order mark code; \
                 expected absent, 0, or 1; retained as a repair draft"
            ));
            add_cause(&mut repair, CsvTextRepairCause::ByteOrderMark);
            false
        }
    };
    (utf8_bom, repair)
}

fn add_cause(repair: &mut Option<CsvTextRepairDependency>, cause: CsvTextRepairCause) {
    *repair = Some(match *repair {
        Some(dependency) => dependency.with_cause(cause),
        None => CsvTextRepairDependency::new(cause),
    });
}

fn empty_text_policy(
    settings: &roxmltree::Node<'_, '_>,
    component_name: &str,
    repair: &mut Option<CsvTextRepairDependency>,
    warnings: &mut Vec<String>,
) -> bool {
    let component_name = diagnostic_component_name(component_name);
    match settings.attribute("removeempty") {
        None | Some("true" | "1") => false,
        Some("false" | "0") => true,
        Some(_) => {
            warnings.push(format!(
                "csv component `{component_name}` declares an invalid removeempty flag; \
                 expected true, false, 1, or 0; retained as a repair draft treating empty fields as absent"
            ));
            add_cause(repair, CsvTextRepairCause::EmptyPolicy);
            false
        }
    }
}

pub(super) fn read_settings(
    settings: &roxmltree::Node<'_, '_>,
    component_name: &str,
    options: &mut FormatOptions,
    warnings: &mut Vec<String>,
) {
    let component_name = diagnostic_component_name(component_name);
    options.csv_preserve_empty_strings = empty_text_policy(
        settings,
        component_name,
        &mut options.csv_text_repair_dependency,
        warnings,
    );
    if let Some(separator) = settings.attribute("separator") {
        let mut characters = separator.chars();
        options.delimiter = characters.next();
        if options.delimiter.is_none() {
            warnings.push(format!(
                "csv component `{component_name}` declares an empty separator; \
                 only one character is representable; retained as a repair draft using comma"
            ));
            add_cause(
                &mut options.csv_text_repair_dependency,
                CsvTextRepairCause::Separator,
            );
        } else if characters.next().is_some() {
            warnings.push(format!(
                "csv component `{component_name}` declares a multi-character separator; \
                 only one character is representable; retained as a repair draft using a supported separator"
            ));
            add_cause(
                &mut options.csv_text_repair_dependency,
                CsvTextRepairCause::Separator,
            );
        }
    }
    options.has_header_row = Some(match settings.attribute("firstrownames") {
        None | Some("false") => false,
        Some("true") => true,
        Some(_) => {
            warnings.push(format!(
                "csv component `{component_name}` declares an invalid firstrownames flag; \
                 expected absent, true, or false; retained as a repair draft without header rows"
            ));
            add_cause(
                &mut options.csv_text_repair_dependency,
                CsvTextRepairCause::HeaderRow,
            );
            false
        }
    });
    if let Some(raw_quote) = settings.attribute("quote") {
        if raw_quote.is_empty() {
            options.csv_quote_disabled = true;
        } else {
            let mut characters = raw_quote.chars();
            match (characters.next(), characters.next()) {
                (Some(quote), None)
                    if quote.is_ascii_graphic() && quote != options.delimiter.unwrap_or(',') =>
                {
                    options.csv_quote = (quote != '"').then_some(quote);
                }
                _ => {
                    warnings.push(format!(
                        "csv component `{component_name}`: quote setting must be one printable \
                         ASCII character distinct from the separator; retained as a repair draft using double quote"
                    ));
                    add_cause(
                        &mut options.csv_text_repair_dependency,
                        CsvTextRepairCause::Quote,
                    );
                }
            }
        }
    }
    let delimiter = options.delimiter.unwrap_or(',');
    let quote = options.csv_quote.unwrap_or('"');
    if !delimiter.is_ascii()
        || matches!(delimiter, '\0' | '\r' | '\n')
        || (!options.csv_quote_disabled && delimiter == quote)
    {
        let already_marked = options
            .csv_text_repair_dependency
            .is_some_and(|dependency| {
                dependency
                    .causes()
                    .any(|cause| cause == CsvTextRepairCause::Separator)
            });
        if !already_marked {
            warnings.push(format!(
                "csv component `{component_name}` declares an unsupported separator; \
                 expected one non-NUL, non-newline ASCII byte distinct from the active quote; retained as a repair draft"
            ));
        }
        add_cause(
            &mut options.csv_text_repair_dependency,
            CsvTextRepairCause::Separator,
        );
        options.delimiter = Some(if !options.csv_quote_disabled && quote == ',' {
            '\t'
        } else {
            ','
        });
    }
}

pub(super) fn warn_typed_empty_cells(
    schema: &SchemaNode,
    component_name: &str,
    options: &mut FormatOptions,
    warnings: &mut Vec<String>,
) {
    let component_name = diagnostic_component_name(component_name);
    let all_text = matches!(&schema.kind, ir::SchemaKind::Group { children, .. }
        if children.iter().all(|field| matches!(field.kind, ir::SchemaKind::Scalar { ty: ScalarType::String })));
    if !all_text {
        warnings.push(format!(
            "csv source component `{component_name}` keeps empty fields with non-text columns; \
             native typed-empty-cell behavior is unsupported; retained as a repair draft"
        ));
        add_cause(
            &mut options.csv_text_repair_dependency,
            CsvTextRepairCause::TypedEmptyCells,
        );
    }
}

pub(super) fn field_declarations(
    names: &roxmltree::Node<'_, '_>,
) -> Result<Vec<SchemaNode>, &'static str> {
    let mut declarations = names
        .children()
        .filter(|node| node.is_element() && node.tag_name().name().starts_with("field"))
        .map(|node| {
            node.tag_name()
                .name()
                .strip_prefix("field")
                .and_then(|suffix| suffix.parse::<usize>().ok())
                .map(|index| (index, node))
                .ok_or("has a field declaration without a valid numeric suffix")
        })
        .collect::<Result<Vec<_>, _>>()?;
    declarations.sort_by_key(|(index, _)| *index);
    if declarations.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err("has duplicate field declaration indexes");
    }
    let mut field_names = BTreeSet::new();
    declarations
        .into_iter()
        .map(|(_, field)| {
            let name = field.attribute("name").unwrap_or_default();
            if name.is_empty() || !field_names.insert(name) {
                return Err("has an empty or duplicate field name");
            }
            let ty = match field.attribute("type") {
                Some("number" | "decimal" | "double" | "float") => ScalarType::Float,
                Some("integer" | "int") => ScalarType::Int,
                Some("boolean") => ScalarType::Bool,
                _ => ScalarType::String,
            };
            Ok(SchemaNode::scalar(name, ty))
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SingletonPosition {
    Before(usize),
    After(usize),
}

pub(super) struct SingletonRow<'a, 'input> {
    pub(super) position: SingletonPosition,
    pub(super) entry: roxmltree::Node<'a, 'input>,
}

pub(super) fn singleton_port_path(position: SingletonPosition, field: &str) -> Vec<String> {
    let (marker, index) = match position {
        SingletonPosition::Before(index) => (CSV_SINGLETON_BEFORE, index),
        SingletonPosition::After(index) => (CSV_SINGLETON_AFTER, index),
    };
    vec![marker.to_string(), index.to_string(), field.to_string()]
}

pub(crate) fn split_singleton_port(path: &[String]) -> Option<(SingletonPosition, &str)> {
    let [marker, index, field] = path else {
        return None;
    };
    let index = index.parse().ok()?;
    let position = match marker.as_str() {
        CSV_SINGLETON_BEFORE => SingletonPosition::Before(index),
        CSV_SINGLETON_AFTER => SingletonPosition::After(index),
        _ => return None,
    };
    Some((position, field))
}

pub(super) fn select_block<'a, 'input>(
    root: roxmltree::Node<'a, 'input>,
    configured_block: Option<&str>,
    component_name: &str,
    format_name: &str,
    warnings: &mut Vec<String>,
) -> Option<roxmltree::Node<'a, 'input>> {
    let document = root
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("document"));
    let blocks = document
        .map(|document| {
            document
                .children()
                .filter(|node| {
                    node.has_tag_name("entry")
                        && configured_block.is_none_or(|name| node.attribute("name") == Some(name))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let fallback = || {
        let mut entry = root.children().find(|node| node.has_tag_name("entry"))?;
        while matches!(
            entry.attribute("name"),
            Some("FileInstance") | Some("document")
        ) {
            entry = entry.children().find(|node| node.has_tag_name("entry"))?;
        }
        Some(entry)
    };
    let first = blocks.first().copied().or_else(fallback)?;
    let Some(repeated) = blocks.iter().copied().find(|candidate| {
        candidate.attribute("clone") == Some("1")
            && parse_u32(candidate.attribute("inpkey")).is_some()
    }) else {
        return Some(first);
    };
    if blocks.len() > 1 && format_name != "csv" {
        let target_label = if format_name == "csv" {
            "CSV"
        } else {
            format_name
        };
        warnings.push(format!(
            "{format_name} target component `{component_name}` contains singleton rows alongside repeated block `{}`; singleton rows were skipped because ferrule {target_label} targets represent one repeated row shape",
            repeated.attribute("name").unwrap_or_default()
        ));
    }
    Some(repeated)
}

pub(super) fn singleton_rows<'a, 'input>(
    root: roxmltree::Node<'a, 'input>,
    configured_block: Option<&str>,
    selected: roxmltree::Node<'a, 'input>,
) -> Vec<SingletonRow<'a, 'input>> {
    let Some(document) = root
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("document"))
    else {
        return Vec::new();
    };
    let blocks = document
        .children()
        .filter(|node| {
            node.has_tag_name("entry")
                && configured_block.is_none_or(|name| node.attribute("name") == Some(name))
        })
        .collect::<Vec<_>>();
    let Some(selected_index) = blocks.iter().position(|block| block.id() == selected.id()) else {
        return Vec::new();
    };
    if selected.attribute("clone") != Some("1") || parse_u32(selected.attribute("inpkey")).is_none()
    {
        return Vec::new();
    }
    blocks
        .into_iter()
        .enumerate()
        .filter(|(_, block)| block.id() != selected.id())
        .map(|(index, entry)| SingletonRow {
            position: if index < selected_index {
                SingletonPosition::Before(index)
            } else {
                SingletonPosition::After(index)
            },
            entry,
        })
        .collect()
}
