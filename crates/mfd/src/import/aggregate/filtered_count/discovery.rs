use super::*;

struct Trace<'a, 'n> {
    functions: &'a BTreeMap<u32, Vec<usize>>,
    links: &'a BTreeMap<u32, Vec<u32>>,
    raw_filters: &'a BTreeSet<u32>,
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
        let Self {
            functions,
            links,
            raw_filters,
            n,
            edges,
            budget,
        } = self;
        budget.claim(2, 0, depth)?;
        if active.contains(&feed) {
            return Ok(!raw_filters.is_empty());
        }
        if done.contains(&feed) {
            return Ok(false);
        }
        active.insert(feed);
        let registered_filter = functions
            .get(&feed)
            .is_some_and(|indices| indices.iter().any(|&i| n.functions[i].name == "filter"));
        let mut result = raw_filters.contains(&feed) && !registered_filter;
        if let Some(indices) = functions.get(&feed) {
            for &index in indices {
                let f = &n.functions[index];
                if f.name == "filter" {
                    result |= filter_may_select_scalar(f, functions, n, edges, budget)?;
                } else {
                    for key in f.inputs.iter().flatten() {
                        budget.claim(1, 0, depth)?;
                        if let Some(&feed) = edges.feeds.get(key) {
                            result |= self.affected(feed, active, done, depth + 1)?;
                        } else if !edges.complete && !raw_filters.is_empty() {
                            result = true;
                        }
                    }
                }
            }
        }
        if let Some(dependencies) = links.get(&feed) {
            for &dependency in dependencies {
                result |= self.affected(dependency, active, done, depth + 1)?;
            }
            if dependencies.is_empty() && !edges.complete && !raw_filters.is_empty() {
                let mut ordinary_source = false;
                for source in n.sources {
                    budget.claim(1, 0, depth)?;
                    ordinary_source |=
                        source.input_keys.is_empty() && source.ports.contains_key(&feed);
                }
                result |= !ordinary_source;
            }
        } else if !functions.contains_key(&feed) && !raw_filters.is_empty() {
            let mut registered = false;
            for schema in n.schemas {
                budget.claim(1, 0, depth)?;
                registered |= schema.ports.contains_key(&feed);
            }
            result |= !registered;
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
    let Some(feed) = filter
        .inputs
        .first()
        .copied()
        .flatten()
        .and_then(|key| edges.feeds.get(&key).copied())
    else {
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
            checked_path(path, budget)?;
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
    // A proved ordinary group sequence belongs to the existing group/forward
    // Count adapters, not this scalar filter theorem. Unknown scalar origins
    // remain candidates and will be refused by the ownership census.
    Ok(!ordinary_group)
}

pub(super) fn discover(
    table: &mut CountProofTable,
    structure: roxmltree::Node<'_, '_>,
    edges: &EdgeInputs,
    n: &Normalized<'_>,
    budget: &CountConstruction,
) -> Result<(), String> {
    let mut functions = BTreeMap::<u32, Vec<usize>>::new();
    let mut links = BTreeMap::<u32, Vec<u32>>::new();
    let mut raw_filters = BTreeSet::new();
    for owner in structure
        .children()
        .filter(|c| c.has_tag_name("children"))
        .flat_map(|c| c.children())
        .filter(|c| c.has_tag_name("component") && c.attribute("name") == Some("filter"))
    {
        for node in owner.descendants().filter(roxmltree::Node::is_element) {
            budget.claim(1 + node.attributes().len(), 0, 0)?;
            if node.ancestors().find(|n| n.has_tag_name("component")) == Some(owner)
                && let Some(key) = crate::import::schema::parse_u32(node.attribute("outkey"))
            {
                budget.claim(1, 0, 0)?;
                raw_filters.insert(key);
            }
        }
        for container in owner.children().filter(|c| c.has_tag_name("targets")) {
            for (index, point) in container
                .children()
                .filter(roxmltree::Node::is_element)
                .enumerate()
            {
                budget.claim(2, 0, 0)?;
                if index >= 64 {
                    return Err("filter discovery pin bound exceeded".into());
                }
                if let Some(key) = crate::import::schema::parse_u32(point.attribute("key")) {
                    raw_filters.insert(key);
                }
            }
        }
    }
    for (i, f) in n.functions.iter().enumerate() {
        budget.claim(f.declared_output_keys.len() + 1, 0, 0)?;
        for &key in &f.declared_output_keys {
            functions.entry(key).or_default().push(i);
            if f.name == "filter" {
                budget.claim(1, 0, 0)?;
                raw_filters.insert(key);
            }
        }
    }
    // Discovery links are not ownership claims. Keep every registered
    // intermediate/call route and opaque local function header so a wrapper
    // cannot hide a physical filter before the one demanded authority census.
    for schema in n.schemas {
        for &output in &schema.output_keys {
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
    for owner in structure
        .children()
        .filter(|c| c.has_tag_name("children"))
        .flat_map(|c| c.children())
        .filter(|c| c.has_tag_name("component"))
    {
        budget.claim(n.functions.len() + 1, 0, 0)?;
        if n.functions
            .iter()
            .enumerate()
            .any(|(i, _)| n.functions.count_origin_at(i) == Origin::ActiveMain(RawOwner::of(owner)))
        {
            continue;
        }
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        for container in owner
            .children()
            .filter(|c| c.has_tag_name("sources") || c.has_tag_name("targets"))
        {
            for (position, point) in container
                .children()
                .filter(roxmltree::Node::is_element)
                .enumerate()
            {
                budget.claim(2, 0, 0)?;
                if position >= 64 {
                    return Err("opaque discovery header pin bound exceeded".into());
                }
                if let Some(key) = crate::import::schema::parse_u32(point.attribute("key")) {
                    if container.has_tag_name("sources") {
                        inputs.push(key);
                    } else {
                        outputs.push(key);
                    }
                }
            }
        }
        for output in outputs {
            add_links(&mut links, output, inputs.iter().copied(), edges, budget)?;
        }
    }
    for owner in structure
        .children()
        .filter(|c| c.has_tag_name("children"))
        .flat_map(|c| c.children())
        .filter(|c| c.has_tag_name("component") && c.attribute("name") == Some("count"))
    {
        if non_count_header(owner) {
            continue;
        }
        budget.claim(n.functions.len() + 1, 0, 0)?;
        let registration = n.functions.iter().enumerate().position(|(i, _)| {
            n.functions.count_origin_at(i) == Origin::ActiveMain(RawOwner::of(owner))
        });
        if registration.is_none() && known_other_owner(RawOwner::of(owner), n, budget)? {
            continue;
        }
        let mut inputs = Vec::new();
        let mut outputs = BTreeSet::new();
        for node in owner.descendants().filter(roxmltree::Node::is_element) {
            budget.claim(1 + node.attributes().len(), 0, 0)?;
            if node.ancestors().find(|n| n.has_tag_name("component")) != Some(owner)
                || node.ancestors().find(|n| n.has_tag_name("structure")) != Some(structure)
            {
                continue;
            }
            if let Some(key) = crate::import::schema::parse_u32(node.attribute("outkey")) {
                budget.claim(1, 0, 0)?;
                outputs.insert(key);
            }
            if let Some(key) = crate::import::schema::parse_u32(node.attribute("inpkey")) {
                budget.claim(1, 0, 0)?;
                inputs.push(key);
            }
        }
        for container in owner
            .children()
            .filter(|c| c.has_tag_name("sources") || c.has_tag_name("targets"))
        {
            for (position, pin) in container
                .children()
                .filter(roxmltree::Node::is_element)
                .enumerate()
            {
                budget.claim(1 + pin.attributes().len(), 0, 0)?;
                if position >= 64 {
                    return Err("Count discovery pin bound exceeded".into());
                }
                if let Some(key) = crate::import::schema::parse_u32(pin.attribute("key")) {
                    budget.claim(1, 0, 0)?;
                    if container.has_tag_name("sources") {
                        inputs.push(key);
                    } else {
                        outputs.insert(key);
                    }
                }
            }
        }
        let mut demand = false;
        for input in inputs {
            budget.claim(1, 0, 0)?;
            if let Some(&feed) = edges.feeds.get(&input) {
                demand |= raw_filters.contains(&feed) && !functions.contains_key(&feed);
                demand |= Trace {
                    functions: &functions,
                    links: &links,
                    raw_filters: &raw_filters,
                    n,
                    edges,
                    budget,
                }
                .affected(feed, &mut BTreeSet::new(), &mut BTreeSet::new(), 0)?;
            } else if !edges.complete {
                demand |= !raw_filters.is_empty();
            }
        }
        if !demand {
            continue;
        }
        let mut connected = !edges.complete;
        for &feed in edges.feeds.values() {
            budget.claim(1, 0, 0)?;
            connected |= outputs.contains(&feed);
        }
        // A disconnected Count cannot claim the physical route. Its optional
        // capture work never creates a raw/final authority census.
        if !connected {
            continue;
        }
        budget.claim(2 + outputs.len(), 0, 0)?;
        let outputs = outputs.into_iter().collect::<Vec<_>>();
        let id = table.candidates.len();
        for &key in &outputs {
            table.candidate_outputs.entry(key).or_default().push(id);
        }
        table.candidates.push(Candidate {
            registration,
            raw_owner: Some(RawOwner::of(owner)),
            outputs,
            connected,
            plan: Err("unreconciled Count".into()),
        });
    }
    // A promoted/synthetic Count cannot acquire active-main authority by
    // appearing only after registration. Keep its outputs explicitly refused.
    for (index, f) in n
        .functions
        .iter()
        .enumerate()
        .filter(|(_, f)| f.name == "count")
    {
        budget.claim(1, 0, 0)?;
        if table
            .candidates
            .iter()
            .any(|c| c.registration == Some(index))
            || matches!(n.functions.count_origin_at(index), Origin::ActiveMain(_))
        {
            continue;
        }
        let connected = edges
            .feeds
            .values()
            .any(|feed| f.declared_output_keys.contains(feed));
        if !connected {
            continue;
        }
        let mut demand = false;
        for input in f.inputs.iter().flatten() {
            budget.claim(1, 0, 0)?;
            if let Some(&feed) = edges.feeds.get(input) {
                demand |= Trace {
                    functions: &functions,
                    links: &links,
                    raw_filters: &raw_filters,
                    n,
                    edges,
                    budget,
                }
                .affected(feed, &mut BTreeSet::new(), &mut BTreeSet::new(), 0)?;
            }
        }
        if !demand {
            continue;
        }
        budget.claim(f.declared_output_keys.len() + 2, 0, 0)?;
        let outputs = f.declared_output_keys.iter().copied().collect::<Vec<_>>();
        let id = table.candidates.len();
        for &key in &outputs {
            table.candidate_outputs.entry(key).or_default().push(id);
        }
        table.candidates.push(Candidate {
            registration: Some(index),
            raw_owner: None,
            outputs,
            connected,
            plan: Err("non-active Count origin".into()),
        });
    }
    Ok(())
}

fn add_links(
    links: &mut BTreeMap<u32, Vec<u32>>,
    output: u32,
    inputs: impl Iterator<Item = u32>,
    edges: &EdgeInputs,
    budget: &CountConstruction,
) -> Result<(), String> {
    budget.claim(1, 0, 0)?;
    links.entry(output).or_default();
    for input in inputs {
        budget.claim(2, 0, 0)?;
        if let Some(&feed) = edges.feeds.get(&input) {
            links.entry(output).or_default().push(feed);
        }
    }
    Ok(())
}
