use ir::Instance;

use super::{
    MAX_TRACE_ROW_NAME_CHARS, MAX_TRACE_ROW_TEXT_BYTES, TraceOutputKind, TraceValue,
    bounded_row_value, truncate_bytes, truncate_chars,
};

const MAX_TREE_NODES: usize = 64;
const MAX_TREE_DEPTH: usize = 8;
const MAX_TREE_CHILDREN: usize = 8;

/// A bounded structural source snapshot. Field names and portable document
/// paths label children; collection items have no name and retain their order.
/// All names and scalar previews share one 512-byte UTF-8 budget for the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceSourceTree {
    pub name: Option<String>,
    pub name_truncated: bool,
    pub kind: TraceOutputKind,
    pub value: Option<TraceValue>,
    pub children: Vec<Self>,
    pub omitted_children: usize,
    pub depth_limited: bool,
}

impl TraceSourceTree {
    pub(super) fn new(instance: &Instance) -> Self {
        let mut remaining_nodes = MAX_TREE_NODES;
        Self::capture(
            instance,
            None,
            0,
            &mut remaining_nodes,
            MAX_TRACE_ROW_TEXT_BYTES,
        )
        .0
    }

    fn capture(
        instance: &Instance,
        name: Option<&str>,
        depth: usize,
        remaining_nodes: &mut usize,
        text_limit: usize,
    ) -> (Self, usize) {
        *remaining_nodes -= 1;
        let (name, name_truncated) = match name {
            Some(name) => {
                let (name, chars_truncated) = truncate_chars(name, MAX_TRACE_ROW_NAME_CHARS);
                let (name, bytes_truncated) = truncate_bytes(&name, text_limit / 2);
                (Some(name), chars_truncated || bytes_truncated)
            }
            None => (None, false),
        };
        let mut remaining_text = text_limit - name.as_ref().map_or(0, String::len);
        let mut tree = Self {
            name,
            name_truncated,
            kind: TraceOutputKind::of(instance),
            value: None,
            children: Vec::new(),
            omitted_children: 0,
            depth_limited: false,
        };
        if let Instance::Scalar(value) = instance {
            let value = bounded_row_value(value, remaining_text);
            remaining_text -= value.preview.len();
            tree.value = Some(value);
            return (tree, text_limit - remaining_text);
        }
        let child_count = match instance {
            Instance::Group(fields) => fields.len(),
            Instance::Repeated(items) | Instance::MappedSequence(items) => items.len(),
            Instance::DocumentSet(documents) => documents.len(),
            Instance::Scalar(_) => 0,
        };
        tree.depth_limited = depth == MAX_TREE_DEPTH && child_count > 0;
        if !tree.depth_limited {
            let visible = child_count.min(MAX_TREE_CHILDREN);
            for index in 0..visible {
                if *remaining_nodes == 0 {
                    break;
                }
                let (name, instance) = match instance {
                    Instance::Group(fields) => {
                        let (name, value) = &fields[index];
                        (Some(name.as_str()), value)
                    }
                    Instance::Repeated(items) | Instance::MappedSequence(items) => {
                        (None, &items[index])
                    }
                    Instance::DocumentSet(documents) => {
                        let member = &documents[index];
                        (Some(member.path()), member.value())
                    }
                    Instance::Scalar(_) => break,
                };
                // Give later siblings a share even when the first subtree has
                // long text. The node and depth caps bound traversal separately.
                let share = remaining_text / (visible - index);
                let (child, used) =
                    Self::capture(instance, name, depth + 1, remaining_nodes, share);
                remaining_text -= used;
                tree.children.push(child);
            }
        }
        tree.omitted_children = child_count - tree.children.len();
        (tree, text_limit - remaining_text)
    }
}
