//! Compare XML occurrences at the mapping's selected input collection with
//! serialized CSV rows. A value match is not an execution trace.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

use ir::{SchemaKind, SchemaNode};
use mapping::{Node, Project, Scope, ScopeIteration, SourceOwner};
use roxmltree::{Document, Node as XmlNode, NodeId};

use crate::preview_table::{MAX_CELLS, MAX_COLUMNS, MAX_ROWS, PreviewTable};

#[derive(Debug)]
pub(super) struct InputItem {
    pub excerpt: Range<usize>,
    pub line: u32,
    values: Vec<Option<usize>>,
}

#[derive(Debug)]
pub(super) struct InputGroup {
    pub items: Range<usize>,
    values: Vec<Option<usize>>,
}

#[derive(Debug)]
pub(super) struct RowScope {
    pub path: String,
    pub group_name: String,
    pub columns: Vec<String>,
    pub groups: Vec<InputGroup>,
    pub items: Vec<InputItem>,
    values: Vec<String>,
}

struct InputField<'a> {
    name: &'a str,
    /// Depth in the root-to-item path of the frame that owns the scalar.
    owner: usize,
    path: &'a [String],
}

fn schema_child<'a>(node: &'a SchemaNode, name: &str) -> Option<&'a SchemaNode> {
    let SchemaKind::Group { children, .. } = &node.kind else {
        return None;
    };
    let mut fields = children.iter().filter(|child| child.name == name);
    let field = fields.next()?;
    fields.next().is_none().then_some(field)
}

fn scalar_at(schema: &SchemaNode, path: &[String]) -> bool {
    !path.is_empty()
        && path
            .iter()
            .try_fold(schema, |owner, name| {
                schema_child(owner, name).filter(|node| !node.repeating)
            })
            .is_some_and(|node| matches!(&node.kind, SchemaKind::Scalar { .. }))
}

fn input_fields<'a>(
    project: &'a Project,
    root: &'a Scope,
    schemas: &[&SchemaNode],
    path: &[String],
) -> Vec<InputField<'a>> {
    root.bindings
        .iter()
        .filter_map(|binding| {
            let Node::SourceField { path: field, frame } =
                project.graph.nodes.get(&binding.node)?
            else {
                return None;
            };
            let mut owners = schemas.iter().enumerate().filter(|(depth, schema)| {
                (*depth == 0 || schema.repeating)
                    && frame
                        .as_ref()
                        .is_none_or(|wanted| wanted.as_slice() == &path[..*depth])
                    && scalar_at(schema, field)
            });
            let (owner, _) = owners.next()?;
            // A fallback across two possible frames isn't an explicit input key.
            owners.next().is_none().then_some(InputField {
                name: &binding.target_field,
                owner,
                path: field,
            })
        })
        .collect()
}

fn plain_child<'a, 'input>(node: XmlNode<'a, 'input>, name: &str) -> Option<XmlNode<'a, 'input>> {
    let mut matches = node.children().filter(|child| child.has_tag_name(name));
    let found = matches.next()?;
    matches.next().is_none().then_some(found)
}

/// Values from a repeated ancestor are stored once, even when many inner
/// items use that ancestor as one component of their compound key.
#[derive(Default)]
struct Values {
    by_node: HashMap<NodeId, usize>,
    by_field: HashMap<(NodeId, usize), Option<usize>>,
    text: Vec<String>,
    cells: usize,
}

impl Values {
    fn collect(
        &mut self,
        node: XmlNode<'_, '_>,
        depth: usize,
        fields: &[InputField<'_>],
    ) -> Result<Vec<Option<usize>>, String> {
        self.cells += fields.len();
        if self.cells > MAX_CELLS {
            return Err("Input key cells exceed the row-scope inspection limit.".into());
        }
        Ok(fields
            .iter()
            .enumerate()
            .map(|(column, field)| {
                let distance = depth.checked_sub(field.owner)?;
                let frame = node.ancestors().nth(distance)?;
                let slot = (frame.id(), column);
                if let Some(index) = self.by_field.get(&slot) {
                    return *index;
                }
                let index = field
                    .path
                    .iter()
                    .try_fold(frame, |owner, name| plain_child(owner, name))
                    .filter(|leaf| !leaf.children().any(|child| child.is_element()))
                    .map(|leaf| {
                        *self.by_node.entry(leaf.id()).or_insert_with(|| {
                            let index = self.text.len();
                            self.text.push(
                                leaf.children()
                                    .filter(|child| child.is_text())
                                    .filter_map(|child| child.text())
                                    .collect(),
                            );
                            index
                        })
                    });
                self.by_field.insert(slot, index);
                index
            })
            .collect())
    }
}

fn descend<'a, 'input>(
    start: XmlNode<'a, 'input>,
    path: &[String],
) -> Result<Vec<XmlNode<'a, 'input>>, String> {
    let mut nodes = vec![start];
    for name in path {
        let mut next = Vec::new();
        for node in nodes {
            for child in node
                .children()
                .filter(|child| child.has_tag_name(name.as_str()))
            {
                if next.len() == MAX_ROWS {
                    return Err("Input occurrences exceed the row-scope inspection limit.".into());
                }
                next.push(child);
            }
        }
        nodes = next;
    }
    Ok(nodes)
}

impl RowScope {
    pub(super) fn capture(text: &str, project: &Project, root: &Scope) -> Result<Self, String> {
        let path = match &root.iteration {
            ScopeIteration::Source(path)
            | ScopeIteration::DynamicDocuments { source: path, .. } => path,
            ScopeIteration::OwnedSource { source, .. } if source.owner == SourceOwner::Primary => {
                &source.path
            }
            _ => {
                return Err(
                    "This output has no simple primary input collection to compare.".into(),
                );
            }
        };
        if path.len() > 128 {
            return Err("Input scope is too deep for inspection.".into());
        }
        let mut schemas = vec![&project.source];
        for name in path {
            let next = schema_child(schemas.last().unwrap(), name)
                .ok_or("Input scope does not resolve to a unique schema path.")?;
            schemas.push(next);
        }
        if !schemas.last().is_some_and(|schema| schema.repeating) {
            return Err("The selected path does not declare repeating input items.".into());
        }
        if text.len() > crate::run_report::MAX_PREVIEW_BYTES {
            return Err("Input exceeds the 1 MiB row-scope inspection limit.".into());
        }
        let doc = Document::parse_with_options(
            text,
            roxmltree::ParsingOptions {
                nodes_limit: MAX_CELLS as u32,
                ..Default::default()
            },
        )
        .map_err(|error| format!("Input XML inspection unavailable: {error}"))?;
        let xml_root = doc.root_element();
        if !xml_root.has_tag_name(project.source.name.as_str())
            || doc
                .descendants()
                .any(|node| node.is_element() && node.tag_name().namespace().is_some())
        {
            return Err("Row-scope inspection requires unqualified XML elements matching the primary schema.".into());
        }
        if root.bindings.len() > MAX_COLUMNS {
            return Err("Too many bindings for row-scope inspection.".into());
        }
        let fields = input_fields(project, root, &schemas, path);
        let parent_depth = schemas
            .iter()
            .enumerate()
            .take(path.len())
            .rfind(|(_, node)| node.repeating)
            .map(|(depth, _)| depth)
            .unwrap_or(0);
        let mut values = Values::default();
        let mut groups = Vec::new();
        let mut items = Vec::new();
        for parent in descend(xml_root, &path[..parent_depth])? {
            let start = items.len();
            for node in descend(parent, &path[parent_depth..])? {
                if items.len() == MAX_ROWS {
                    return Err("Input occurrences exceed the row-scope inspection limit.".into());
                }
                let excerpt = node.range();
                // text_pos_at walks the source; line numbers are indexed below instead.
                items.push(InputItem {
                    excerpt,
                    line: 0,
                    values: values.collect(node, path.len(), &fields)?,
                });
            }
            groups.push(InputGroup {
                items: start..items.len(),
                values: values.collect(parent, parent_depth, &fields)?,
            });
        }
        let (mut previous, mut line) = (0, 1u32);
        for item in &mut items {
            line += text.as_bytes()[previous..item.excerpt.start]
                .iter()
                .filter(|&&byte| byte == b'\n')
                .count() as u32;
            item.line = line;
            previous = item.excerpt.start;
        }
        Ok(Self {
            path: format!(
                "/{}",
                schemas
                    .iter()
                    .map(|schema| schema.name.as_str())
                    .collect::<Vec<_>>()
                    .join("/")
            ),
            group_name: schemas[parent_depth].name.clone(),
            columns: fields.iter().map(|field| field.name.to_owned()).collect(),
            groups,
            items,
            values: values.text,
        })
    }

    fn value(&self, values: &[Option<usize>], column: usize) -> Option<&str> {
        values
            .get(column)?
            .and_then(|index| self.values.get(index).map(String::as_str))
    }

    pub(super) fn label(&self, group: usize, item: Option<usize>, keys: &[String]) -> String {
        let values = item.map_or(&self.groups[group].values, |i| &self.items[i].values);
        let parts: Vec<_> = keys
            .iter()
            .filter_map(|key| {
                let col = self.columns.iter().position(|field| field == key)?;
                self.value(values, col).filter(|v| !v.is_empty())
            })
            .map(|value| &value[..value.floor_char_boundary(value.len().min(96))])
            .collect();
        if !parts.is_empty() {
            return parts.join(" / ");
        }
        if keys.is_empty() && item.is_none() {
            let examples = self
                .columns
                .iter()
                .enumerate()
                .filter_map(|(index, name)| {
                    let value = self.value(values, index).filter(|v| !v.is_empty())?;
                    Some(format!(
                        "{name}: {}",
                        &value[..value.floor_char_boundary(value.len().min(80))]
                    ))
                })
                .take(2)
                .collect::<Vec<_>>();
            if !examples.is_empty() {
                return examples.join(" · ");
            }
        }
        match item {
            Some(index) => format!("Item {}", index + 1),
            None => format!("{} {}", self.group_name, group + 1),
        }
    }

    pub(super) fn match_rows(
        &self,
        output: &PreviewTable,
        keys: &[String],
    ) -> Result<RowMatches, String> {
        if keys.is_empty() {
            return Err("Choose direct input key fields to match occurrences to data rows.".into());
        }
        let mut source_cols = Vec::new();
        let mut target_cols = Vec::new();
        for key in keys {
            let mut columns = self
                .columns
                .iter()
                .enumerate()
                .filter(|(_, name)| *name == key);
            let column = columns
                .next()
                .filter(|_| columns.next().is_none())
                .ok_or_else(|| format!("`{key}` has no unambiguous direct input field."))?;
            source_cols.push(column.0);
            target_cols.push(output.column(key)?);
        }
        let mut output_keys = BTreeMap::new();
        let mut missing_output_keys = Vec::new();
        for (index, row) in output.rows.iter().enumerate() {
            let key: Vec<_> = target_cols.iter().map(|&col| row[col].as_str()).collect();
            if key.iter().all(|v| !v.is_empty()) {
                output_keys
                    .entry(key)
                    .and_modify(|found| *found = RowMatch::Ambiguous)
                    .or_insert(RowMatch::Row(index));
            } else {
                missing_output_keys.push(index);
            }
        }
        let input_keys: Vec<Option<Vec<_>>> = self
            .items
            .iter()
            .map(|item| {
                source_cols
                    .iter()
                    .map(|&col| self.value(&item.values, col).filter(|v| !v.is_empty()))
                    .collect()
            })
            .collect();
        let mut counts = BTreeMap::new();
        for key in input_keys.iter().flatten() {
            *counts.entry(key.as_slice()).or_insert(0usize) += 1;
        }
        let items: Vec<_> = input_keys
            .iter()
            .map(|key| match key {
                None => RowMatch::MissingKey,
                Some(key) if counts[key.as_slice()] != 1 => RowMatch::Ambiguous,
                Some(key) => output_keys.get(key).copied().unwrap_or(RowMatch::NoOutput),
            })
            .collect();
        let matched = items
            .iter()
            .filter(|row| matches!(row, RowMatch::Row(_)))
            .count();
        let duplicate_output_keys = output_keys
            .values()
            .filter(|row| **row == RowMatch::Ambiguous)
            .count();
        Ok(RowMatches {
            items,
            missing_output_keys,
            duplicate_output_keys,
            unmatched_output_rows: output.rows.len() - matched,
        })
    }
}

#[derive(Debug)]
pub(super) struct RowMatches {
    pub items: Vec<RowMatch>,
    pub missing_output_keys: Vec<usize>,
    pub duplicate_output_keys: usize,
    pub unmatched_output_rows: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RowMatch {
    Row(usize),
    MissingKey,
    Ambiguous,
    NoOutput,
}
