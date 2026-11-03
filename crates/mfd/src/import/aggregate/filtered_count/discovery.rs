//! Refusal-only connectivity, independent of optional positive construction.
//! All claims are unions: a source alias never hides an opaque/function route.
use super::*;

struct Seed {
    registration: Option<usize>,
    owner: Option<RawOwner>,
    inputs: Vec<u32>,
    outputs: Vec<u32>,
    unknown_inputs: bool,
}
#[derive(Default)]
struct Header {
    inputs: Vec<u32>,
    outputs: BTreeSet<u32>,
    fn_inputs: Option<usize>,
    filter: bool,
    schema: bool,
}
struct Trace<'a, 'n> {
    functions: &'a BTreeMap<u32, Vec<usize>>,
    links: &'a BTreeMap<u32, Vec<u32>>,
    filters: &'a BTreeSet<u32>,
    opaque: &'a BTreeSet<u32>,
    n: &'a Normalized<'n>,
    edges: &'a EdgeInputs,
    budget: &'a CountConstruction,
}
impl Trace<'_, '_> {
    fn affected(
        &self,
        feed: u32,
        active: &mut BTreeSet<u32>,
        done: &mut BTreeSet<u32>,
        depth: usize,
    ) -> Result<bool, String> {
        self.budget.claim(2, 0, depth)?;
        if active.contains(&feed) {
            return Ok(!self.filters.is_empty());
        }
        if done.contains(&feed) {
            return Ok(false);
        }
        active.insert(feed);
        let mut registered_filter = false;
        let mut result = self.opaque.contains(&feed) && !self.filters.is_empty();
        if let Some(indices) = self.functions.get(&feed) {
            for &index in indices {
                let function = &self.n.functions[index];
                self.budget.claim(1, function.name.len(), depth)?;
                if function.name == "filter" {
                    registered_filter = true;
                    result |= filter_may_select_scalar(
                        function,
                        self.functions,
                        self.n,
                        self.edges,
                        self.budget,
                    )?;
                }
            }
        }
        result |= self.filters.contains(&feed) && !registered_filter;
        if let Some(dependencies) = self.links.get(&feed) {
            for &dependency in dependencies {
                result |= self.affected(dependency, active, done, depth + 1)?;
            }
        } else {
            result |= !self.filters.is_empty();
        }
        active.remove(&feed);
        done.insert(feed);
        Ok(result)
    }
}
fn filter_may_select_scalar(
    filter: &FnComponent,
    functions: &BTreeMap<u32, Vec<usize>>,
    n: &Normalized<'_>,
    edges: &EdgeInputs,
    budget: &CountConstruction,
) -> Result<bool, String> {
    let Some(input) = filter.inputs.first().copied().flatten() else {
        return Ok(true);
    };
    let Some(feeds) = edges.claims.get(&input) else {
        return Ok(true);
    };
    if feeds.len() != 1 {
        return Ok(true);
    }
    let Some(&feed) = feeds.first() else {
        return Ok(true);
    };
    budget.claim(1, 0, 0)?;
    if functions.contains_key(&feed) {
        return Ok(true);
    }
    let mut ordinary_group = false;
    for source in n.schemas {
        budget.claim(1, 0, 0)?;
        if let Some(path) = source.ports.get(&feed) {
            let node = target_node(&source.schema, path, budget);
            if node.is_err() {
                budget.claim(0, 0, 0)?;
                return Ok(true);
            }
            if (source.format == ComponentFormat::Json || source.is_variable)
                && source.unambiguous_declared_output(feed)
                && node.is_ok_and(|node| matches!(node.kind, SchemaKind::Group { .. }))
            {
                ordinary_group = true;
            } else {
                return Ok(true);
            }
        }
    }
    Ok(!ordinary_group)
}
fn add_links(
    links: &mut BTreeMap<u32, Vec<u32>>,
    output: u32,
    inputs: impl Iterator<Item = u32>,
    edges: &EdgeInputs,
    budget: &CountConstruction,
) -> Result<(), String> {
    budget.claim(1, 0, 0)?;
    let dependencies = links.entry(output).or_default();
    for input in inputs {
        budget.claim(1, 0, 0)?;
        if let Some(feeds) = edges.claims.get(&input) {
            budget.claim(feeds.len(), 0, 0)?;
            dependencies.extend(feeds);
        }
    }
    Ok(())
}

// This walk reads attribute NAMES to find additional pins, not literal values
// or schema names. Numeric values are charged separately before parsing. Reuse
// already-parsed, noncollapsing reader facts instead of rescanning padded keys.
fn header_names(node: roxmltree::Node<'_, '_>, budget: &CountConstruction) -> Result<(), String> {
    let mut bytes = node
        .tag_name()
        .name()
        .len()
        .checked_add(node.tag_name().namespace().map_or(0, str::len))
        .ok_or("Count header name overflow")?;
    for attribute in node.attributes() {
        bytes = bytes
            .checked_add(attribute.name().len())
            .and_then(|n| n.checked_add(attribute.namespace().map_or(0, str::len)))
            .ok_or("Count header name overflow")?;
    }
    budget.claim(1 + 3 * node.attributes().len(), bytes, 0)
}
fn pin(value: Option<&str>, budget: &CountConstruction) -> Result<Option<u32>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    budget.claim(1, value.len(), 0)?;
    count_visit(Counter::RefusalPin);
    Ok(crate::import::schema::parse_u32(Some(value)))
}

pub(super) fn discover(
    table: &mut CountProofTable,
    structure: roxmltree::Node<'_, '_>,
    edges: &EdgeInputs,
    n: &Normalized<'_>,
    budget: &CountConstruction,
) -> Result<(), String> {
    let owners = || {
        structure
            .children()
            .filter(|c| c.has_tag_name("children"))
            .flat_map(|c| c.children())
            .filter(|c| c.has_tag_name("component"))
    };
    if !n.functions.iter().any(|f| f.kind == 5 && f.name == "count")
        && !owners()
            .any(|owner| owner.attribute("name") == Some("count") && !non_count_header(owner))
    {
        return Ok(());
    }
    let mut seeds = Vec::new();
    let mut functions = BTreeMap::<u32, Vec<usize>>::new();
    let mut filters = BTreeSet::new();
    let mut opaque = BTreeSet::new();
    let mut links = BTreeMap::<u32, Vec<u32>>::new();
    let mut headers = BTreeMap::<RawOwner, Header>::new();
    let mut connected = BTreeSet::<u32>::new();
    for feeds in edges.claims.values() {
        budget.claim(1 + feeds.len(), 0, 0)?;
        connected.extend(feeds);
    }
    for (index, function) in n.functions.iter().enumerate() {
        budget.claim(
            1 + function.declared_output_keys.len(),
            function
                .name
                .len()
                .checked_add(function.library.len())
                .ok_or("Count discovery metadata overflow")?,
            0,
        )?;
        if let Origin::ActiveMain(owner) = n.functions.count_refusal_origin_at(index) {
            budget.claim(
                1 + function.inputs.len() + function.declared_output_keys.len(),
                0,
                0,
            )?;
            let header = headers.entry(owner).or_default();
            header.fn_inputs =
                Some(header.fn_inputs.unwrap_or(0) + function.inputs.iter().flatten().count());
            header
                .inputs
                .extend(function.inputs.iter().flatten().copied());
            header.outputs.extend(&function.declared_output_keys);
            header.filter |= function.name == "filter";
        }
        for &output in &function.declared_output_keys {
            functions.entry(output).or_default().push(index);
            add_links(
                &mut links,
                output,
                function.inputs.iter().flatten().copied(),
                edges,
                budget,
            )?;
            if function.name == "filter" {
                budget.claim(1, 0, 0)?;
                filters.insert(output);
            }
        }
        if function.kind == 5 && function.name == "count" {
            budget.claim(
                1 + function.inputs.len() + function.declared_output_keys.len(),
                0,
                0,
            )?;
            seeds.push(Seed {
                registration: Some(index),
                owner: match n.functions.count_refusal_origin_at(index) {
                    Origin::ActiveMain(owner) => Some(owner),
                    _ => None,
                },
                inputs: function.inputs.iter().flatten().copied().collect(),
                outputs: function.declared_output_keys.iter().copied().collect(),
                unknown_inputs: false,
            });
        }
    }
    for (index, schema) in n.schemas.iter().enumerate() {
        budget.claim(1, 0, 0)?;
        if let Origin::ActiveMain(owner) = n.schemas.count_refusal_origin_at(index) {
            budget.claim(
                1 + schema.input_keys.len() + schema.ports.len() + schema.output_keys.len(),
                0,
                0,
            )?;
            let header = headers.entry(owner).or_default();
            header.schema = true;
            header.inputs.extend(&schema.input_keys);
            header
                .outputs
                .extend(schema.ports.keys().chain(&schema.output_keys));
        }
        for &output in schema.ports.keys().chain(
            schema
                .output_keys
                .iter()
                .filter(|key| !schema.ports.contains_key(key)),
        ) {
            add_links(
                &mut links,
                output,
                schema.input_keys.iter().copied(),
                edges,
                budget,
            )?;
        }
    }
    for call in n.calls {
        for &output in call.outputs.keys() {
            add_links(&mut links, output, call.input_pins(), edges, budget)?;
        }
    }
    for recipe in n.external_scalar {
        add_links(
            &mut links,
            recipe.output,
            recipe.input_pins(),
            edges,
            budget,
        )?;
    }
    for recipe in n.external_xslt {
        add_links(
            &mut links,
            recipe.output,
            recipe.input_pins(),
            edges,
            budget,
        )?;
    }
    for recipe in n.json_serializers {
        add_links(
            &mut links,
            recipe.output,
            recipe.input_pins(),
            edges,
            budget,
        )?;
    }
    for recipe in n.xml_serializers {
        add_links(
            &mut links,
            recipe.output,
            recipe.input_pins(),
            edges,
            budget,
        )?;
    }
    for recipe in n.json_parsers {
        for &output in recipe.outputs.keys().chain(recipe.object_outputs.keys()) {
            add_links(&mut links, output, recipe.input_pins(), edges, budget)?;
        }
    }
    for recipe in n.flextext_parsers {
        for &output in recipe.outputs.keys() {
            add_links(&mut links, output, recipe.input_pins(), edges, budget)?;
        }
    }

    // Component subtrees are disjoint: this is ONE dependency-header walk, not
    // a per-function/per-Count ownership census. Extra pins never grant proof.
    for owner in owners() {
        budget.claim(1, 0, 0)?;
        let identity = RawOwner::of(owner);
        let known = headers.get(&identity);
        let fn_known = known.is_some_and(|header| header.fn_inputs.is_some());
        let schema_root = if known.is_some_and(|header| header.schema) {
            owner
                .children()
                .find(|n| n.has_tag_name("data"))
                .and_then(|data| data.children().find(|n| n.has_tag_name("root")))
        } else {
            None
        };
        let mut inputs = Vec::new();
        let mut outputs = BTreeSet::new();
        let mut keyed_inputs = 0usize;
        for node in owner.descendants().filter(roxmltree::Node::is_element) {
            count_visit(Counter::RefusalElement);
            header_names(node, budget)?;
            for (depth, _) in node.ancestors().take_while(|n| *n != owner).enumerate() {
                budget.claim(1, 0, depth)?;
            }
            if node.ancestors().find(|n| n.has_tag_name("component")) != Some(owner)
                || node.ancestors().find(|n| n.has_tag_name("structure")) != Some(structure)
            {
                continue;
            }
            let saved_entry = node.has_tag_name("entry")
                && schema_root.is_some()
                && node.ancestors().find(|n| n.has_tag_name("root")) == schema_root;
            if !saved_entry {
                for (attribute, output) in [("inpkey", false), ("outkey", true)] {
                    if let Some(key) = pin(node.attribute(attribute), budget)? {
                        budget.claim(1, 0, 0)?;
                        if output {
                            outputs.insert(key);
                        } else {
                            inputs.push(key);
                        }
                    }
                }
            }
            if node.has_tag_name("datapoint")
                && let Some(parent) = node.parent()
                && (parent.has_tag_name("sources") || parent.has_tag_name("targets"))
            {
                if fn_known && parent.parent() == Some(owner) {
                    if parent.has_tag_name("sources") && node.attribute("key").is_some() {
                        keyed_inputs += 1;
                    }
                    continue;
                }
                if let Some(key) = pin(node.attribute("key"), budget)? {
                    budget.claim(1, 0, 0)?;
                    if parent.has_tag_name("targets") {
                        outputs.insert(key);
                    } else {
                        inputs.push(key);
                    }
                }
            }
        }
        let extras = !inputs.is_empty() || !outputs.is_empty();
        let unknown_inputs = known
            .and_then(|header| header.fn_inputs)
            .is_some_and(|count| count != keyed_inputs);
        if let Some(header) = known {
            budget.claim(header.inputs.len() + header.outputs.len(), 0, 0)?;
            inputs.extend(&header.inputs);
            outputs.extend(&header.outputs);
        }
        let name = owner.attribute("name").unwrap_or_default();
        budget.claim(1, name.len(), 0)?;
        let filter = if fn_known {
            known.is_some_and(|header| header.filter)
        } else {
            name == "filter" && !known.is_some_and(|header| header.schema)
        };
        for &output in &outputs {
            if extras || known.is_none() {
                add_links(&mut links, output, inputs.iter().copied(), edges, budget)?;
            }
            if unknown_inputs {
                budget.claim(1, 0, 0)?;
                opaque.insert(output);
            }
            if filter {
                budget.claim(1, 0, 0)?;
                filters.insert(output);
            }
        }
        budget.claim(seeds.len(), 0, 0)?;
        if let Some(seed) = seeds.iter_mut().find(|seed| seed.owner == Some(identity)) {
            budget.claim(inputs.len() + outputs.len() + 1, 0, 0)?;
            seed.inputs = inputs;
            seed.outputs = outputs.into_iter().collect();
            seed.unknown_inputs = unknown_inputs;
        } else if name == "count" && !known.is_some_and(|header| header.schema) {
            let bytes = owner
                .attributes()
                .try_fold(0usize, |sum, attribute| {
                    sum.checked_add(attribute.value().len())
                })
                .ok_or("Count header value overflow")?;
            budget.claim(1, bytes, 0)?;
            if !non_count_header(owner) {
                budget.claim(1 + outputs.len(), 0, 0)?;
                seeds.push(Seed {
                    registration: None,
                    owner: Some(identity),
                    inputs,
                    outputs: outputs.into_iter().collect(),
                    unknown_inputs,
                });
            }
        }
    }
    for seed in seeds {
        budget.claim(1 + seed.outputs.len(), 0, 0)?;
        if !seed.outputs.iter().any(|output| connected.contains(output)) {
            continue;
        }
        let trace = Trace {
            functions: &functions,
            links: &links,
            filters: &filters,
            opaque: &opaque,
            n,
            edges,
            budget,
        };
        let mut affected = seed.unknown_inputs && !filters.is_empty();
        for input in seed.inputs {
            budget.claim(1, 0, 0)?;
            if let Some(feeds) = edges.claims.get(&input) {
                for &feed in feeds {
                    affected |=
                        trace.affected(feed, &mut BTreeSet::new(), &mut BTreeSet::new(), 0)?;
                }
            } else if !edges.complete {
                affected |= !filters.is_empty();
            }
        }
        if !affected {
            continue;
        }
        budget.claim(1 + seed.outputs.len(), 0, 0)?;
        let id = table.candidates.len();
        for &output in &seed.outputs {
            table.candidate_outputs.entry(output).or_default().push(id);
        }
        table.candidates.push(Candidate {
            registration: seed.registration,
            raw_owner: seed.owner,
            outputs: seed.outputs,
            connected: true,
            plan: Err("unreconciled Count".into()),
        });
    }
    Ok(())
}
