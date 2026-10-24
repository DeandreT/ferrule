//! Native source grammar is independent of repeated execution-view containers.
use serde::{Deserialize, Serialize};

/// One original X12 source particle. `maximum == None` denotes unbounded.
/// Bounds describe native parser ownership, not runtime collection cardinality.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct X12SourceStructure {
    pub name: String,
    pub segment: Option<String>,
    pub minimum: u32,
    pub maximum: Option<u32>,
    pub respect_maximum: bool,
    pub merged_entries: u32,
    pub children: Vec<X12SourceStructure>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub selection: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub known_segments: Vec<String>,
}

fn is_false(value: &bool) -> bool {
    !value
}

impl X12SourceStructure {
    /// Refuse a parser plan that does not cover this exact physical schema.
    pub fn validate(&self, schema: &ir::SchemaNode) -> Result<(), &'static str> {
        fn walk(
            plan: &X12SourceStructure,
            node: &ir::SchemaNode,
            depth: usize,
            count: &mut usize,
        ) -> Result<(), &'static str> {
            *count += 1;
            if depth > 64 || *count > 10_000 {
                return Err("native X12 structure exceeds its limit");
            }
            if plan.name != node.name
                || plan.minimum > 1024
                || plan.merged_entries == 0
                || plan.merged_entries > 1024
                || plan.maximum.is_some_and(|max| max < plan.minimum)
            {
                return Err("invalid native X12 structure binding");
            }
            if (depth != 0 && !plan.known_segments.is_empty())
                || plan.known_segments.len() > 10_000
                || plan.known_segments.iter().any(|name| {
                    !(2..=3).contains(&name.len())
                        || !name
                            .bytes()
                            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
                })
                || plan
                    .known_segments
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    != plan.known_segments.len()
            {
                return Err("invalid native X12 segment catalog");
            }
            let ir::SchemaKind::Group { children, .. } = &node.kind else {
                return Err("native X12 particle is not a group");
            };
            if let Some(segment) = &plan.segment {
                if plan.selection
                    || !plan.children.is_empty()
                    || !(2..=3).contains(&segment.len())
                    || !segment
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
                {
                    return Err("invalid native X12 segment");
                }
            } else {
                if children.len() != plan.children.len() {
                    return Err("native X12 structure does not cover its schema");
                }
                for (child, node) in plan.children.iter().zip(children) {
                    walk(child, node, depth + 1, count)?;
                }
            }
            Ok(())
        }
        walk(self, schema, 0, &mut 0)?;
        fn selected(plan: &X12SourceStructure) -> bool {
            plan.selection || plan.children.iter().any(selected)
        }
        if selected(self) && self.known_segments.is_empty() {
            return Err("native X12 selection requires a segment catalog");
        }
        Ok(())
    }
}
