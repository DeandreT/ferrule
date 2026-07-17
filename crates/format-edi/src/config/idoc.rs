//! Bounded compiler for the line-oriented SAP IDoc parser configuration.

use std::io::Read;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use ir::{ScalarType, SchemaNode};
use mapping::{IdocFieldLayout, IdocLayout, IdocSegmentLayout};
use thiserror::Error;

const MAX_CONFIG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_LINES: usize = 200_000;
const MAX_DEPTH: usize = 128;

#[derive(Debug)]
pub struct CompiledIdoc {
    pub schema: SchemaNode,
    pub layout: IdocLayout,
}

#[derive(Debug, Error)]
pub enum IdocConfigError {
    #[error("could not read IDoc configuration `{path}`: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid IDoc configuration: {0}")]
    Invalid(String),
    #[error("IDoc configuration exceeds the {0} limit")]
    Limit(&'static str),
}

pub fn import_config(path: &Path) -> Result<CompiledIdoc, IdocConfigError> {
    let file = std::fs::File::open(path).map_err(|source| IdocConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut text = String::new();
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|source| IdocConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if text.len() as u64 > MAX_CONFIG_BYTES {
        return Err(IdocConfigError::Limit("file size"));
    }
    let lines = text.lines().map(str::trim).collect::<Vec<_>>();
    if lines.len() > MAX_LINES {
        return Err(IdocConfigError::Limit("line count"));
    }

    let begin = lines
        .iter()
        .position(|line| keyword(line) == "BEGIN_IDOC")
        .ok_or_else(|| IdocConfigError::Invalid("missing BEGIN_IDOC".into()))?;
    let mut cursor = begin + 1;
    let nodes = parse_nodes(&lines, &mut cursor, "END_IDOC", 0)?;
    if nodes.is_empty() {
        return Err(IdocConfigError::Invalid(
            "IDoc contains no segment definitions".into(),
        ));
    }

    let mut segment_layouts = Vec::new();
    let children = nodes
        .into_iter()
        .map(|node| node.into_schema(&mut segment_layouts))
        .collect::<Result<Vec<_>, _>>()?;
    let layout = IdocLayout::new(segment_layouts)
        .map_err(|error| IdocConfigError::Invalid(error.to_string()))?;
    Ok(CompiledIdoc {
        schema: SchemaNode::group("IDOC", children),
        layout,
    })
}

#[derive(Debug)]
enum ConfigNode {
    Segment {
        name: String,
        repeating: bool,
        fields: Vec<Field>,
    },
    Group {
        name: String,
        repeating: bool,
        children: Vec<ConfigNode>,
    },
}

impl ConfigNode {
    fn into_schema(
        self,
        layouts: &mut Vec<IdocSegmentLayout>,
    ) -> Result<SchemaNode, IdocConfigError> {
        match self {
            Self::Segment {
                name,
                repeating,
                fields,
            } => {
                let schema_fields = fields
                    .iter()
                    .map(|field| SchemaNode::scalar(&field.name, ScalarType::String))
                    .collect();
                let layout_fields = fields
                    .into_iter()
                    .map(|field| {
                        IdocFieldLayout::new(field.name, field.first_byte, field.last_byte)
                            .map_err(|error| IdocConfigError::Invalid(error.to_string()))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                layouts.push(
                    IdocSegmentLayout::new(&name, layout_fields)
                        .map_err(|error| IdocConfigError::Invalid(error.to_string()))?,
                );
                let mut node = SchemaNode::group(&name, schema_fields);
                node.repeating = repeating;
                Ok(node)
            }
            Self::Group {
                name,
                repeating,
                children,
            } => {
                let children = children
                    .into_iter()
                    .map(|child| child.into_schema(layouts))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut node = SchemaNode::group(&name, children);
                node.repeating = repeating;
                Ok(node)
            }
        }
    }
}

#[derive(Debug)]
struct Field {
    name: String,
    first_byte: NonZeroU32,
    last_byte: NonZeroU32,
}

fn parse_nodes(
    lines: &[&str],
    cursor: &mut usize,
    end: &str,
    depth: usize,
) -> Result<Vec<ConfigNode>, IdocConfigError> {
    if depth > MAX_DEPTH {
        return Err(IdocConfigError::Limit("nesting depth"));
    }
    let mut nodes = Vec::new();
    while let Some(line) = lines.get(*cursor) {
        let key = keyword(line);
        if key == end {
            *cursor += 1;
            return Ok(nodes);
        }
        match key {
            "BEGIN_SEGMENT" => nodes.push(parse_segment(lines, cursor)?),
            "BEGIN_GROUP" => nodes.push(parse_group(lines, cursor, depth + 1)?),
            _ => *cursor += 1,
        }
    }
    Err(IdocConfigError::Invalid(format!("missing {end}")))
}

fn parse_group(
    lines: &[&str],
    cursor: &mut usize,
    depth: usize,
) -> Result<ConfigNode, IdocConfigError> {
    let line = lines
        .get(*cursor)
        .ok_or_else(|| IdocConfigError::Invalid("missing group declaration".into()))?;
    let declared = argument(line)
        .ok_or_else(|| IdocConfigError::Invalid("BEGIN_GROUP has no number".into()))?;
    let name = if declared.starts_with("SG") {
        declared.to_string()
    } else {
        format!("SG{declared}")
    };
    *cursor += 1;
    let start = *cursor;
    let children = parse_nodes(lines, cursor, "END_GROUP", depth)?;
    let repeating = occurrence_is_repeating(&lines[start..*cursor]);
    if children.is_empty() {
        return Err(IdocConfigError::Invalid(format!(
            "group `{name}` contains no segments"
        )));
    }
    Ok(ConfigNode::Group {
        name,
        repeating,
        children,
    })
}

fn parse_segment(lines: &[&str], cursor: &mut usize) -> Result<ConfigNode, IdocConfigError> {
    let line = lines
        .get(*cursor)
        .ok_or_else(|| IdocConfigError::Invalid("missing segment declaration".into()))?;
    let name = argument(line)
        .ok_or_else(|| IdocConfigError::Invalid("BEGIN_SEGMENT has no name".into()))?
        .to_string();
    *cursor += 1;
    let start = *cursor;
    let mut fields = Vec::new();
    while let Some(line) = lines.get(*cursor) {
        match keyword(line) {
            "END_SEGMENT" => {
                *cursor += 1;
                if fields.is_empty() {
                    return Err(IdocConfigError::Invalid(format!(
                        "segment `{name}` contains no fields"
                    )));
                }
                return Ok(ConfigNode::Segment {
                    name,
                    repeating: occurrence_is_repeating(&lines[start..*cursor]),
                    fields,
                });
            }
            "BEGIN_FIELDS" => {
                *cursor += 1;
                fields = parse_fields(lines, cursor)?;
            }
            _ => *cursor += 1,
        }
    }
    Err(IdocConfigError::Invalid(format!(
        "segment `{name}` has no END_SEGMENT"
    )))
}

fn parse_fields(lines: &[&str], cursor: &mut usize) -> Result<Vec<Field>, IdocConfigError> {
    let mut fields = Vec::new();
    let mut name = None;
    let mut first = None;
    let mut last = None;
    while let Some(line) = lines.get(*cursor) {
        match keyword(line) {
            "END_FIELDS" => {
                push_field(&mut fields, &mut name, &mut first, &mut last)?;
                *cursor += 1;
                return Ok(fields);
            }
            "NAME" => {
                push_field(&mut fields, &mut name, &mut first, &mut last)?;
                name = argument(line).map(str::to_string);
            }
            "BYTE_FIRST" => first = parse_nonzero(argument(line), "BYTE_FIRST")?,
            "BYTE_LAST" => last = parse_nonzero(argument(line), "BYTE_LAST")?,
            _ => {}
        }
        *cursor += 1;
    }
    Err(IdocConfigError::Invalid("missing END_FIELDS".into()))
}

fn push_field(
    fields: &mut Vec<Field>,
    name: &mut Option<String>,
    first: &mut Option<NonZeroU32>,
    last: &mut Option<NonZeroU32>,
) -> Result<(), IdocConfigError> {
    let Some(name) = name.take() else {
        return Ok(());
    };
    let first_byte = first
        .take()
        .ok_or_else(|| IdocConfigError::Invalid(format!("field `{name}` has no BYTE_FIRST")))?;
    let last_byte = last
        .take()
        .ok_or_else(|| IdocConfigError::Invalid(format!("field `{name}` has no BYTE_LAST")))?;
    fields.push(Field {
        name,
        first_byte,
        last_byte,
    });
    Ok(())
}

fn parse_nonzero(
    value: Option<&str>,
    label: &'static str,
) -> Result<Option<NonZeroU32>, IdocConfigError> {
    let raw = value.ok_or_else(|| IdocConfigError::Invalid(format!("{label} has no value")))?;
    let value = raw
        .parse::<u32>()
        .map_err(|_| IdocConfigError::Invalid(format!("invalid {label} `{raw}`")))?;
    NonZeroU32::new(value)
        .map(Some)
        .ok_or_else(|| IdocConfigError::Invalid(format!("{label} must be one-based")))
}

fn occurrence_is_repeating(lines: &[&str]) -> bool {
    lines.iter().any(|line| {
        (keyword(line) == "STATUS"
            && argument(line).is_some_and(|value| !value.eq_ignore_ascii_case("MANDATORY")))
            || (keyword(line) == "LOOPMAX"
                && argument(line)
                    .and_then(|value| value.parse::<u64>().ok())
                    .is_some_and(|value| value > 1))
    })
}

fn keyword(line: &str) -> &str {
    line.split_whitespace().next().unwrap_or_default()
}

fn argument(line: &str) -> Option<&str> {
    line.split_whitespace().nth(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::SchemaKind;

    #[test]
    fn compiles_nested_groups_occurrences_and_absolute_fields() {
        let path = std::env::temp_dir().join(format!(
            "ferrule_idoc_config_{}_{}.txt",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::write(
            &path,
            "BEGIN_SEGMENT_SECTION\nBEGIN_IDOC TEST\nBEGIN_SEGMENT HEADER\nSTATUS MANDATORY\nLOOPMAX 1\nBEGIN_FIELDS\nNAME DOCNO\nTYPE CHARACTER\nBYTE_FIRST 11\nBYTE_LAST 18\nEND_FIELDS\nEND_SEGMENT\nBEGIN_GROUP 1\nSTATUS OPTIONAL\nLOOPMAX 99\nBEGIN_SEGMENT ITEM\nSTATUS MANDATORY\nLOOPMAX 1\nBEGIN_FIELDS\nNAME CODE\nBYTE_FIRST 11\nBYTE_LAST 14\nNAME COUNT\nBYTE_FIRST 15\nBYTE_LAST 17\nEND_FIELDS\nEND_SEGMENT\nEND_GROUP\nEND_IDOC\nEND_SEGMENT_SECTION\n",
        )
        .unwrap();

        let compiled = import_config(&path).unwrap();
        let SchemaKind::Group { children, .. } = &compiled.schema.kind else {
            panic!("root should be a group");
        };
        assert_eq!(children[0].name, "HEADER");
        assert!(!children[0].repeating);
        assert_eq!(children[1].name, "SG1");
        assert!(children[1].repeating);
        assert_eq!(compiled.layout.segment("ITEM").unwrap().fields().len(), 2);
        assert_eq!(
            compiled.layout.segment("ITEM").unwrap().fields()[1]
                .first_byte()
                .get(),
            15
        );
        std::fs::remove_file(path).unwrap();
    }
}
