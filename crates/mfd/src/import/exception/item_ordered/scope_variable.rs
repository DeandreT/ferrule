//! One schema-identical XML carrier; no general variable or fanout admission.

use super::super::super::function::FnComponent;
use super::*;

pub(super) fn target_owner(
    builder: &GraphBuilder<'_>,
    target: &SchemaComponent,
    edges: &[(u32, u32)],
    keep: u32,
    filter: &FnComponent,
    recipe: &Recipe,
) -> Result<u32, &'static str> {
    let [variable] = builder.intermediates else {
        return Err("scope-sequence requires exactly one typed XML variable");
    };
    let source = builder.sources[0];
    let raw_input = filter.inputs[0].ok_or("filter node input is missing")?;
    let raw = unique_feed(edges, raw_input)?;
    let collection = source
        .ports
        .get(&raw)
        .filter(|_| source.output_keys.contains(&raw))
        .ok_or("scope-sequence raw feed is not the primary collection")?;
    // The first extension proves a single immediate collection. A parent-root
    // trigger cannot be guessed for deeper or independently iterated contexts.
    if collection.len() != 1 || !plain_collection(&source.schema, collection) {
        return Err("scope-sequence requires one immediate primary repeated collection");
    }
    let item = schema_node_at(&source.schema, collection)
        .ok_or("scope-sequence owner schema is missing")?;
    if variable.format != ComponentFormat::Xml
        || !variable.is_variable
        || variable.is_pass_through
        || variable.input_instance.is_some()
        || variable.output_instance.is_some()
        || !local_options(&variable.options)
        || !bounded_schema(&variable.schema)
        || &variable.schema != item
    {
        return Err("scope-sequence variable must retain the exact local owner XML schema");
    }
    let roots = variable
        .ports
        .iter()
        .filter(|(key, path)| path.is_empty() && variable.input_keys.contains(key))
        .map(|(key, _)| *key)
        .collect::<Vec<_>>();
    let [root_input] = roots.as_slice() else {
        return Err("scope-sequence variable has no unique root input");
    };
    let roots = variable
        .ports
        .iter()
        .filter(|(key, path)| path.is_empty() && variable.output_keys.contains(key))
        .map(|(key, _)| *key)
        .collect::<Vec<_>>();
    let [root_output] = roots.as_slice() else {
        return Err("scope-sequence variable has no unique root output");
    };
    let compute = variable
        .compute_when_key
        .ok_or("scope-sequence variable has no parent-root trigger")?;
    if variable
        .input_keys
        .iter()
        .any(|key| *key != *root_input && *key != compute)
        || unique_feed(edges, *root_input)? != keep
        || consumers(edges, keep) != vec![*root_input]
    {
        return Err("scope-sequence keep branch must exclusively feed its unmodified root");
    }
    let parent = unique_feed(edges, compute)?;
    if !source.output_keys.contains(&parent)
        || source
            .ports
            .get(&parent)
            .is_none_or(|path| !path.is_empty())
        || consumers(edges, parent) != vec![compute]
    {
        return Err("scope-sequence compute trigger must exclusively read the primary parent root");
    }
    let owner_inputs = consumers(edges, *root_output)
        .into_iter()
        .filter(|key| target.input_keys.contains(key))
        .collect::<Vec<_>>();
    let [owner_input] = owner_inputs.as_slice() else {
        return Err("scope-sequence root must drive exactly one primary target owner");
    };
    let owner_path = target
        .ports
        .get(owner_input)
        .ok_or("scope-sequence target owner has no schema path")?;
    if owner_path.is_empty() {
        return Err("scope-sequence target owner cannot be the document root");
    }
    let mut target_trace = Trace::default();
    for key in &target.input_keys {
        let Some(path) = target.ports.get(key) else {
            continue;
        };
        if path.len() <= owner_path.len()
            || !path.starts_with(owner_path)
            || !edges.iter().any(|(_, sink)| sink == key)
        {
            continue;
        }
        if !schema_node_at(&target.schema, path)
            .is_some_and(|node| matches!(node.kind, SchemaKind::Scalar { .. }))
        {
            return Err("scope-sequence target descendants must be ordinary scalar bindings");
        }
        target_trace.input(*key, builder, variable, edges, *root_output)?;
    }
    let mut raw_trace = Trace::default();
    raw_trace.input(
        filter.inputs[1].ok_or("filter predicate input is missing")?,
        builder,
        variable,
        edges,
        raw,
    )?;
    if let Some(message) = recipe
        .message_input
        .filter(|input| edges.iter().any(|(_, sink)| sink == input))
    {
        raw_trace.input(message, builder, variable, edges, raw)?;
    }
    if !raw_trace.positions.is_disjoint(&target_trace.positions)
        || raw_trace
            .feeds
            .iter()
            .any(|key| variable.output_keys.contains(key))
    {
        return Err(
            "scope-sequence raw predicate/message and target positions must remain distinct",
        );
    }
    if raw_trace.positions.iter().any(|key| {
        consumers(edges, *key)
            .iter()
            .any(|sink| !raw_trace.sinks.contains(sink))
    }) || target_trace.positions.iter().any(|key| {
        consumers(edges, *key)
            .iter()
            .any(|sink| !target_trace.sinks.contains(sink))
    }) {
        return Err("scope-sequence position outputs have unrelated or cross-context consumers");
    }
    for sink in consumers(edges, raw) {
        if sink != raw_input && !raw_trace.position_inputs.contains(&sink) {
            return Err("scope-sequence raw collection has an unrelated structural consumer");
        }
    }
    for sink in consumers(edges, *root_output) {
        if sink != *owner_input && !target_trace.position_inputs.contains(&sink) {
            return Err("scope-sequence root has a shared or unsupported structural consumer");
        }
    }
    for key in &variable.output_keys {
        if key == root_output {
            continue;
        }
        let path = variable
            .ports
            .get(key)
            .ok_or("scope-sequence scalar output has no schema path")?;
        if !owner_scalar_path(item, path)
            || consumers(edges, *key)
                .iter()
                .any(|sink| !target_trace.sinks.contains(sink))
        {
            return Err(
                "scope-sequence scalar outputs must exclusively feed owner target bindings",
            );
        }
    }
    Ok(*owner_input)
}

#[derive(Default)]
struct Trace {
    depths: BTreeMap<u32, usize>,
    active: BTreeSet<u32>,
    feeds: BTreeSet<u32>,
    sinks: BTreeSet<u32>,
    positions: BTreeSet<u32>,
    position_inputs: BTreeSet<u32>,
    dependencies: usize,
}
impl Trace {
    fn input(
        &mut self,
        sink: u32,
        builder: &GraphBuilder<'_>,
        variable: &SchemaComponent,
        edges: &[(u32, u32)],
        position_feed: u32,
    ) -> Result<usize, &'static str> {
        self.dependencies = self
            .dependencies
            .checked_add(1)
            .filter(|count| *count <= MAX_NODES * 4)
            .ok_or("scope-sequence scalar dependency census exceeded")?;
        self.sinks.insert(sink);
        self.feed(
            unique_feed(edges, sink)?,
            builder,
            variable,
            edges,
            position_feed,
        )
    }
    fn feed(
        &mut self,
        key: u32,
        builder: &GraphBuilder<'_>,
        variable: &SchemaComponent,
        edges: &[(u32, u32)],
        position_feed: u32,
    ) -> Result<usize, &'static str> {
        if let Some(depth) = self.depths.get(&key) {
            return Ok(*depth);
        }
        if self.depths.len() + self.active.len() >= MAX_NODES
            || self.active.len() >= MAX_DEPTH
            || !self.active.insert(key)
        {
            return Err("scope-sequence scalar graph exceeds bounded acyclic shape");
        }
        self.feeds.insert(key);
        let mut depth = 1usize;
        if let Some(path) = builder.sources[0]
            .ports
            .get(&key)
            .filter(|_| builder.sources[0].output_keys.contains(&key))
        {
            if !schema_node_at(&builder.sources[0].schema, path)
                .is_some_and(|node| matches!(node.kind, SchemaKind::Scalar { .. }))
            {
                return Err(
                    "scope-sequence scalar expressions cannot read structural source ports",
                );
            }
        } else if variable.output_keys.contains(&key) {
            let path = variable
                .ports
                .get(&key)
                .ok_or("scope-sequence output has no path")?;
            if !owner_scalar_path(&variable.schema, path) {
                return Err("scope-sequence scalar expression reads a non-scalar carrier output");
            }
        } else {
            let index = *builder
                .fn_by_output
                .get(&key)
                .ok_or("scope-sequence scalar dependency is unresolved")?;
            let component = builder
                .fn_components
                .get(index)
                .ok_or("scope-sequence scalar component is missing")?;
            if component.library != "core"
                || !matches!(component.kind, 2 | 5)
                || component.inputs.len() > MAX_DEPTH
                || component.outputs.as_slice() != [key]
                || component.output_pins.as_slice() != [Some(key)]
            {
                return Err(
                    "scope-sequence dependencies must be ordinary single-output scalar functions",
                );
            }
            if component.name == "position" && component.kind == 5 {
                let [Some(input)] = component.inputs.as_slice() else {
                    return Err("scope-sequence position must have one exact sequence input");
                };
                if unique_feed(edges, *input)? != position_feed {
                    return Err("scope-sequence position reads the wrong raw or filtered sequence");
                }
                self.positions.insert(key);
                self.position_inputs.insert(*input);
                self.sinks.insert(*input);
            } else {
                for input in component.inputs.iter().flatten() {
                    if edges.iter().any(|(_, sink)| sink == input) {
                        depth = depth.max(
                            self.input(*input, builder, variable, edges, position_feed)?
                                .checked_add(1)
                                .ok_or("scope-sequence dependency depth overflow")?,
                        );
                    }
                }
            }
        }
        self.active.remove(&key);
        if depth > MAX_DEPTH {
            return Err("scope-sequence scalar dependency depth exceeded");
        }
        self.depths.insert(key, depth);
        Ok(depth)
    }
}
