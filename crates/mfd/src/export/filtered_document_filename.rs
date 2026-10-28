use std::fmt::Write as _;

use mapping::{AggregateOp, Node, NodeId, Project, Scope, ScopeConstruction};

use super::schema::{KeyAlloc, PortMatch, SideFormat, side_format};
use super::source::SourceExports;
use crate::MfdError;

fn qualified(
    project: &Project,
    root: &Scope,
    schema: &ir::SchemaNode,
    filter: NodeId,
    filename: NodeId,
) -> bool {
    let Ok(Some(source)) = root.source() else {
        return false;
    };
    if source.len() != 1
        || side_format(&project.source_path, &project.source_options) != SideFormat::Xml
        || project.source.repeating
        || project.source_options.local_xml_file_set
        || root.construction != ScopeConstruction::Constructed
        || root.has_sort()
        || !root.windows.is_empty()
        || root.grouping_nodes().next().is_some()
        || root.post_group_filter.is_some()
    {
        return false;
    }
    let Some(Node::Call { function, args }) = project.graph.nodes.get(&filter) else {
        return false;
    };
    if function != "greater_than"
        || args.len() != 2
        || !matches!(
            project.graph.nodes.get(&args[1]),
            Some(Node::Const {
                value: ir::Value::Int(0)
            })
        )
    {
        return false;
    }
    let Some(Node::Aggregate {
        function: AggregateOp::Count,
        collection,
        value,
        expression: None,
        arg: None,
    }) = project.graph.nodes.get(&args[0])
    else {
        return false;
    };
    if !value.is_empty() || collection.len() != 1 {
        return false;
    }
    let Some(parent) = project.source.child(&source[0]) else {
        return false;
    };
    let Some(member) = collection.last() else {
        return false;
    };
    if !parent.repeating
        || project.source.child(member).is_some()
        || !parent
            .child(member)
            .is_some_and(|node| node.repeating && matches!(node.kind, ir::SchemaKind::Group { .. }))
    {
        return false;
    }
    if !project.extra_sources.is_empty()
        || !root.bindings.is_empty()
        || !root.dynamic_bindings.is_empty()
        || !root.dynamic_children.is_empty()
        || root.merge_dynamic_fields
    {
        return false;
    }
    let [body] = root.children.as_slice() else {
        return false;
    };
    if !schema
        .child(&body.target_field)
        .is_some_and(|node| node.repeating && matches!(node.kind, ir::SchemaKind::Group { .. }))
    {
        return false;
    }
    if !matches!(&body.iteration, mapping::ScopeIteration::Source(path) if path.as_slice() == std::slice::from_ref(member))
        || body.construction != ScopeConstruction::Constructed
        || body.bindings.is_empty()
        || !body.children.is_empty()
        || !body.dynamic_bindings.is_empty()
        || !body.dynamic_children.is_empty()
        || body.filter.is_some()
        || body.post_group_filter.is_some()
        || body.has_sort()
        || !body.windows.is_empty()
        || body.grouping_nodes().next().is_some()
        || body.merge_dynamic_fields
    {
        return false;
    }
    let mut frame = source.to_vec();
    frame.push(member.clone());
    let Some(member_schema) = parent.child(member) else {
        return false;
    };
    if body
        .bindings
        .iter()
        .any(|binding| match project.graph.nodes.get(&binding.node) {
            Some(Node::SourceField {
                frame: Some(owner),
                path,
            }) if owner == &frame && !path.is_empty() => {
                let mut node = member_schema;
                for part in path {
                    let Some(child) = node.child(part) else {
                        return true;
                    };
                    if child.repeating {
                        return true;
                    }
                    node = child;
                }
                !node.is_scalar()
            }
            _ => true,
        })
    {
        return false;
    }
    let Some(Node::Call { function, args }) = project.graph.nodes.get(&filename) else {
        return false;
    };
    function == "concat"
        && args.len() >= 2
        && args
            .iter()
            .any(|id| matches!(project.graph.nodes.get(id), Some(Node::SourceField { .. })))
        && args.iter().all(|id| match project.graph.nodes.get(id) {
            Some(Node::Const {
                value: ir::Value::String(_),
            }) => true,
            Some(Node::SourceField {
                frame: Some(frame),
                path,
            }) if frame == source && !path.is_empty() => {
                let mut node = parent;
                for segment in path {
                    let Some(child) = node.child(segment) else {
                        return false;
                    };
                    if child.repeating {
                        return false;
                    }
                    node = child;
                }
                matches!(
                    node.kind,
                    ir::SchemaKind::Scalar {
                        ty: ir::ScalarType::String
                    }
                )
            }
            _ => false,
        })
}

pub(super) struct RenderArgs<'a> {
    pub project: &'a Project,
    pub root: &'a Scope,
    pub schema: &'a ir::SchemaNode,
    pub from: u32,
    pub keys: &'a mut KeyAlloc,
    pub uid: &'a mut u32,
    pub components: &'a mut String,
    pub edges: &'a mut Vec<(u32, u32)>,
    pub sources: &'a SourceExports<'a>,
}

pub(super) fn append(args: RenderArgs<'_>) -> Result<u32, MfdError> {
    let RenderArgs {
        project,
        root,
        schema,
        from,
        keys,
        uid,
        components,
        edges,
        sources,
    } = args;
    let Some(filter) = root.filter else {
        return Ok(from);
    };
    let Some(filename) = root.output_path() else {
        return Ok(from);
    };
    if !qualified(project, root, schema, filter, filename) {
        return Err(MfdError::Unsupported(
            "filtered dynamic filenames require a qualified outer-record existence guard".into(),
        ));
    }
    let member_path = (|| {
        let mut path = root.source().ok()??.to_vec();
        let Node::Call { args, .. } = project.graph.nodes.get(&filter)? else {
            return None;
        };
        let Node::Aggregate { collection, .. } = project.graph.nodes.get(args.first()?)? else {
            return None;
        };
        path.push(collection.last()?.clone());
        Some(path)
    })()
    .ok_or_else(|| {
        MfdError::Unsupported("filtered dynamic filename has no member collection".into())
    })?;
    let PortMatch::Unique(member) = sources.match_sequence(&member_path) else {
        return Err(MfdError::Unsupported(
            "filtered dynamic filename member collection is ambiguous".into(),
        ));
    };
    let exists_input = keys.next();
    let predicate = keys.next();
    *uid += 1;
    let _ = write!(
        components,
        "\t\t\t\t<component name=\"exists\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
         \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{exists_input}\"/></sources>\n\
         \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{predicate}\"/></targets>\n\
         \t\t\t\t</component>\n"
    );
    edges.push((member, exists_input));
    let value = keys.next();
    let condition = keys.next();
    let output = keys.next();
    *uid += 1;
    let _ = write!(
        components,
        "\t\t\t\t<component name=\"filter\" library=\"core\" uid=\"{uid}\" kind=\"3\">\n\
         \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{value}\"/><datapoint pos=\"1\" key=\"{condition}\"/></sources>\n\
         \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{output}\"/><datapoint/></targets>\n\
         \t\t\t\t</component>\n"
    );
    edges.extend([(from, value), (predicate, condition)]);
    Ok(output)
}
