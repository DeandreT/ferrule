//! Complete, input-accounted negative routing. This is not positive ownership.
//! The original edge claims are borrowed only during the existing reader call.
use super::*;
use crate::import::graph::EdgeClaimView;
use std::{cell::RefCell, rc::Rc};
pub(in crate::import) mod budget;
mod registered;
#[cfg(test)]
mod tests;
use budget::{Buffer, Dimensions, Ledger};
pub(in crate::import) use registered::{Event, Fact};

#[derive(Clone, Copy, Debug, Default)]
pub(in crate::import) struct Headers {
    count: bool,
    dimensions: Dimensions,
    work: usize,
}
impl Headers {
    pub(in crate::import) fn read(structure: roxmltree::Node<'_, '_>) -> Result<Self, String> {
        let mut result = Self::default();
        let parent_records = structure.parent().into_iter().flat_map(|parent| {
            parent
                .children()
                .filter(saved_container)
                .flat_map(|container| container.descendants())
        });
        // Only saved edge containers expand the scope. Wrapper components are
        // not main producers; the active-structure identity check stays below.
        for node in structure.descendants().chain(parent_records) {
            count_visit(Counter::NegativeHeader);
            result.dimensions.walk_nodes = result
                .dimensions
                .walk_nodes
                .checked_add(1)
                .ok_or("Count routing traversal dimensions overflow")?;
            result.work = result
                .work
                .checked_add(1)
                .ok_or("Count routing header work overflow")?;
            if !node.is_element() {
                continue;
            }
            result.dimensions.nodes += 1;
            result.dimensions.decoded = result
                .dimensions
                .decoded
                .checked_add(node.tag_name().name().len())
                .and_then(|n| n.checked_add(node.tag_name().namespace().map_or(0, str::len)))
                .ok_or("Count routing decoded size overflow")?;
            result.work = result
                .work
                .checked_add(
                    8 + node.tag_name().name().len()
                        + node.tag_name().namespace().map_or(0, str::len),
                )
                .ok_or("Count routing header work overflow")?;
            let component = node.has_tag_name("component")
                && node.parent().is_some_and(|parent| {
                    parent.has_tag_name("children") && parent.parent() == Some(structure)
                });
            if component {
                count_visit(Counter::Header);
            }
            for attribute in node.attributes() {
                result.dimensions.attributes += 1;
                result.dimensions.decoded = result
                    .dimensions
                    .decoded
                    .checked_add(attribute.name().len())
                    .and_then(|n| n.checked_add(attribute.value().len()))
                    .and_then(|n| n.checked_add(attribute.namespace().map_or(0, str::len)))
                    .ok_or("Count routing decoded size overflow")?;
                // Attribute names are inspected; unrelated literal values are
                // only sized, never scanned or retained by this header pass.
                result.work = result
                    .work
                    .checked_add(
                        2 + attribute.name().len() + attribute.namespace().map_or(0, str::len),
                    )
                    .ok_or("Count routing header work overflow")?;
                if matches!(attribute.name(), "key" | "inpkey" | "outkey" | "vertexkey") {
                    result.dimensions.pins += 1;
                }
                if component && attribute.name() == "name" {
                    result.work = result
                        .work
                        .checked_add(attribute.value().len())
                        .ok_or("Count routing header work overflow")?;
                    result.count |= attribute.value() == "count";
                }
            }
            result.dimensions.edges += usize::from(node.has_tag_name("edge"));
        }
        // The saved graph grammar associates a descendant edge with every
        // enclosing vertex, including malformed nested vertices. Size those
        // actual claims as well as XML records before the legacy reader parses
        // or lends them; the negative buffers retain that same complete union.
        for parent in std::iter::once(structure).chain(structure.parent()) {
            for container in parent.children() {
                // Also size direct selector visits, including wrapper siblings
                // outside saved containers. They grant no retained identities.
                result.dimensions.walk_nodes = result
                    .dimensions
                    .walk_nodes
                    .checked_add(1)
                    .ok_or("Count routing traversal dimensions overflow")?;
                result.work = result
                    .work
                    .checked_add(2)
                    .ok_or("Count routing header work overflow")?;
                if !container.has_tag_name("graph") {
                    continue;
                }
                for vertex in container.descendants() {
                    result.work = result
                        .work
                        .checked_add(1)
                        .ok_or("Count routing header work overflow")?;
                    if !vertex.has_tag_name("vertex") {
                        continue;
                    }
                    for edge in vertex.descendants() {
                        result.work = result
                            .work
                            .checked_add(1)
                            .ok_or("Count routing header work overflow")?;
                        if edge.has_tag_name("edge") {
                            result.dimensions.edges = result
                                .dimensions
                                .edges
                                .checked_add(1)
                                .ok_or("Count routing edge dimensions overflow")?;
                        }
                    }
                }
            }
        }
        publish_header(result);
        Ok(result)
    }
}

fn saved_container(node: &roxmltree::Node<'_, '_>) -> bool {
    node.has_tag_name("connections") || node.has_tag_name("graph")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Key {
    Port(u32),
    Raw(usize),
    Registered(u8, usize),
}
#[derive(Clone, Copy, Debug)]
struct Owner {
    raw: RawOwner,
    count: bool,
    filter: bool,
    uncertain: bool,
    wanted: bool,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct CountRegistration {
    slot: usize,
    raw: Option<RawOwner>,
    vertex: Key,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct FilterRegistration {
    raw: Option<RawOwner>,
    slot: usize,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct GroupClaim {
    slot: usize,
    key: u32,
    raw: Option<RawOwner>,
}

#[derive(Debug)]
pub(in crate::import) struct Routing {
    ledger: Ledger,
    owners: Buffer<Owner>,
    links: Buffer<(Key, Key)>,
    counts: Buffer<CountRegistration>,
    filters: Buffer<FilterRegistration>,
    groups: Buffer<GroupClaim>,
    uncertain_ports: Buffer<u32>,
    unknown_edge: bool,
    frozen: bool,
}
pub(in crate::import) type Handle = Rc<RefCell<Routing>>;

pub(super) struct DemandSeed {
    pub registration: Option<usize>,
    pub owner: Option<RawOwner>,
    pub outputs: Vec<u32>,
}
/// Only this complete raw/registered union may distinguish Ordinary from an
/// affected seed. Optional positive failures never participate in this choice.
pub(in crate::import) struct CompleteDemand {
    seeds: Vec<DemandSeed>,
    state: Option<Handle>,
}

/// Compact noncollapsing route receipts. Both the sorted key vector and every
/// multiplicity vector use the graph's preallocation policy.
pub(super) struct OutputIndex {
    entries: Buffer<(u32, Buffer<usize>)>,
}
impl OutputIndex {
    pub(super) const EMPTY: Self = Self {
        entries: Buffer(Vec::new()),
    };
    pub(super) fn get(&self, key: &u32) -> Option<&Vec<usize>> {
        self.entries
            .0
            .binary_search_by_key(key, |(key, _)| *key)
            .ok()
            .map(|i| &self.entries.0[i].1.0)
    }
    #[cfg(test)]
    pub(super) fn contains_key(&self, key: &u32) -> bool {
        self.get(key).is_some()
    }
    fn insert(&mut self, key: u32, id: usize, ledger: &mut Ledger) -> Result<(), String> {
        ledger.lookup(self.entries.0.len())?;
        let index = match self.entries.0.binary_search_by_key(&key, |(key, _)| *key) {
            Ok(index) => index,
            Err(index) => {
                ledger.work(self.entries.0.len() - index + 1)?;
                self.entries.reserve(1, ledger)?;
                self.entries.0.insert(index, (key, Buffer::new()));
                index
            }
        };
        self.entries.0[index].1.push(id, ledger)
    }
}

impl CompleteDemand {
    pub(super) fn install(self, table: &mut CountProofTable) -> Result<(), String> {
        let Some(state) = self.state else {
            return Ok(());
        };
        let mut state = state.borrow_mut();
        let bytes = self
            .seeds
            .len()
            .checked_mul(std::mem::size_of::<Candidate>())
            .ok_or("Count demand receipt overflow")?;
        state.ledger.reserve(self.seeds.len(), bytes)?;
        table
            .candidates
            .try_reserve_exact(self.seeds.len())
            .map_err(|_| "Count demand receipt allocation failed")?;
        for seed in self.seeds {
            let id = table.candidates.len();
            for &output in &seed.outputs {
                table
                    .candidate_outputs
                    .insert(output, id, &mut state.ledger)?;
            }
            state.ledger.reserve(1, 32)?;
            table.candidates.push(Candidate {
                registration: seed.registration,
                raw_owner: seed.owner,
                outputs: seed.outputs,
                connected: true,
                plan: Err("unreconciled Count".into()),
            });
        }
        state.publish();
        Ok(())
    }
}

impl Routing {
    pub(in crate::import) fn register<T: Fact>(
        &mut self,
        value: &T,
        origin: Origin,
        slot: usize,
        replay: bool,
    ) -> Result<(), String> {
        if self.frozen {
            return Err("Count routing registration after freeze".into());
        }
        self.ledger.extend_registered(1, 0)?;
        if T::DOMAIN != 1
            && let Origin::ActiveMain(owner) = origin
            && let Some(index) = self.owner_index(owner)?
        {
            self.owners.0[index].count = false;
            self.owners.0[index].filter = false;
        }
        value.routing(&mut Event {
            state: self,
            vertex: Key::Registered(T::DOMAIN, slot),
            origin,
            slot,
            replay,
        })
    }
    pub(in crate::import) fn record<T: Fact>(&mut self, value: &T, origin: Origin, slot: usize) {
        if self.register(value, origin, slot, false).is_err() {
            self.ledger.failed = true;
        }
    }
    pub(in crate::import) fn prepare(
        headers: Headers,
        structure: roxmltree::Node<'_, '_>,
        edges: EdgeClaimView<'_>,
    ) -> Result<Option<Handle>, String> {
        if !headers.count {
            return Ok(None);
        }
        count_visit(Counter::NegativeBuild);
        let mut state = Self {
            ledger: Ledger::new(headers.dimensions),
            owners: Buffer::new(),
            links: Buffer::new(),
            counts: Buffer::new(),
            filters: Buffer::new(),
            groups: Buffer::new(),
            uncertain_ports: Buffer::new(),
            unknown_edge: false,
            frozen: false,
        };
        for (&input, feeds) in edges.sources {
            state.ledger.work(1)?;
            for &feed in feeds {
                state.link(Key::Port(feed), Key::Port(input))?;
            }
        }
        for children in structure.children() {
            state.ledger.work(1)?;
            if !children.has_tag_name("children") {
                continue;
            }
            for owner in children.children() {
                state.ledger.work(1)?;
                if !owner.has_tag_name("component") {
                    continue;
                }
                let id = state.owners.0.len();
                state.metadata(owner)?;
                let name = owner.attribute("name").unwrap_or_default();
                let mut info = Owner {
                    raw: RawOwner::of(owner),
                    count: name == "count",
                    filter: name == "filter",
                    uncertain: false,
                    wanted: false,
                };
                // The existing non-Count header grammar is used only after its
                // complete inspected subtree has been charged, never as proof.
                if info.count || info.filter {
                    for node in owner.descendants() {
                        state.ledger.work(1)?;
                        if !node.is_element() {
                            continue;
                        }
                        state.metadata(node)?;
                    }
                    if non_count_header(owner) {
                        info.count = false;
                        info.filter = false;
                    }
                }
                state.owners.push(info, &mut state.ledger)?;
                for node in owner.descendants() {
                    state.ledger.work(1)?;
                    if !node.is_element() {
                        continue;
                    }
                    count_visit(Counter::RefusalElement);
                    state.metadata_names(node)?;
                    if !state.belongs(node, owner, structure)? {
                        continue;
                    }
                    for attribute in node.attributes() {
                        let output = match attribute.name() {
                            "inpkey" => false,
                            "outkey" => true,
                            "key" if node.has_tag_name("datapoint") => {
                                let Some(parent) = node.parent() else {
                                    continue;
                                };
                                if parent.has_tag_name("targets") {
                                    true
                                } else if parent.has_tag_name("sources") {
                                    false
                                } else {
                                    continue;
                                }
                            }
                            _ => continue,
                        };
                        state.ledger.text(attribute.value().len())?;
                        count_visit(Counter::RefusalPin);
                        if let Some(key) = crate::import::schema::parse_u32(Some(attribute.value()))
                        {
                            if output {
                                state.link(Key::Raw(id), Key::Port(key))?;
                            } else {
                                state.link(Key::Port(key), Key::Raw(id))?;
                            }
                        } else {
                            state.owners.0[id].uncertain = true;
                        }
                    }
                }
            }
        }
        // Same saved-container scope as positive raw authority, including ALL
        // wrapper siblings. Those siblings intentionally lack a structure
        // ancestor. This audit is connectivity, never a second feed index.
        for parent in std::iter::once(structure).chain(structure.parent()) {
            for container in parent.children() {
                state.ledger.work(1)?;
                if !saved_container(&container) {
                    continue;
                }
                state.metadata(container)?;
                if container.has_tag_name("connections") {
                    for edge in container.descendants().skip(1) {
                        state.ledger.work(1)?;
                        if !edge.is_element() {
                            continue;
                        }
                        // Nested malformed edge records still carry negative
                        // authority; a bounded outer destination cannot hide an
                        // inner record with a different destination.
                        if !edge.has_tag_name("edge") && edge.parent() != Some(container) {
                            continue;
                        }
                        state.metadata(edge)?;
                        let recognized = state.closed_record(edge, false)?;
                        state.saved_edge(
                            edge.attribute("from"),
                            edge.attribute("to"),
                            recognized,
                        )?;
                    }
                } else {
                    for vertex in container.descendants() {
                        state.ledger.work(1)?;
                        if !vertex.has_tag_name("vertex") {
                            continue;
                        }
                        state.metadata(vertex)?;
                        state
                            .ledger
                            .text(vertex.attribute("vertexkey").map_or(0, str::len))?;
                        let from = crate::import::schema::parse_u32(vertex.attribute("vertexkey"));
                        // Nested vertices follow the existing reader/raw audit:
                        // each enclosing vertex contributes its original claim.
                        for edge in vertex.descendants() {
                            state.ledger.work(1)?;
                            if !edge.has_tag_name("edge") {
                                continue;
                            }
                            state.metadata(edge)?;
                            state
                                .ledger
                                .text(edge.attribute("vertexkey").map_or(0, str::len))?;
                            let to = crate::import::schema::parse_u32(edge.attribute("vertexkey"));
                            let recognized = state.closed_record(edge, true)?;
                            state.edge_fact(from, to, recognized)?;
                        }
                    }
                }
            }
        }
        state.ledger.sort(state.links.0.len())?;
        state.links.0.sort_unstable();
        state.links.0.dedup();
        state.provisional_capture()?;
        state.ledger.reserve(
            1,
            std::mem::size_of::<RefCell<Routing>>() + 2 * std::mem::size_of::<usize>(),
        )?;
        state.publish();
        Ok(Some(Rc::new(RefCell::new(state))))
    }
    fn belongs(
        &mut self,
        node: roxmltree::Node<'_, '_>,
        owner: roxmltree::Node<'_, '_>,
        structure: roxmltree::Node<'_, '_>,
    ) -> Result<bool, String> {
        let mut component = None;
        let mut selected_structure = None;
        for ancestor in node.ancestors() {
            self.ledger.work(2)?;
            if component.is_none() && ancestor.has_tag_name("component") {
                component = Some(ancestor);
            }
            if ancestor.has_tag_name("structure") {
                selected_structure = Some(ancestor);
                break;
            }
        }
        Ok(component == Some(owner) && selected_structure == Some(structure))
    }
    fn metadata_names(&mut self, node: roxmltree::Node<'_, '_>) -> Result<(), String> {
        self.ledger.work(1 + node.attributes().len())?;
        self.ledger.text(node.tag_name().name().len())?;
        self.ledger
            .text(node.tag_name().namespace().map_or(0, str::len))?;
        for attribute in node.attributes() {
            self.ledger.text(attribute.name().len())?;
            self.ledger
                .text(attribute.namespace().map_or(0, str::len))?;
        }
        Ok(())
    }
    fn metadata(&mut self, node: roxmltree::Node<'_, '_>) -> Result<(), String> {
        self.metadata_names(node)?;
        for attribute in node.attributes() {
            self.ledger.text(attribute.value().len())?;
        }
        Ok(())
    }
    pub(super) fn link(&mut self, from: Key, to: Key) -> Result<(), String> {
        self.links.push((from, to), &mut self.ledger)
    }
    fn saved_edge(
        &mut self,
        from: Option<&str>,
        to: Option<&str>,
        recognized: bool,
    ) -> Result<(), String> {
        self.ledger
            .text(from.map_or(0, str::len) + to.map_or(0, str::len))?;
        self.edge_fact(
            crate::import::schema::parse_u32(from),
            crate::import::schema::parse_u32(to),
            recognized,
        )
    }
    fn closed_record(
        &mut self,
        node: roxmltree::Node<'_, '_>,
        graph: bool,
    ) -> Result<bool, String> {
        self.ledger.work(2 + node.tag_name().name().len())?;
        let mut closed = node.has_tag_name("edge");
        for attribute in node.attributes() {
            self.ledger.work(2 + 2 * attribute.name().len())?;
            closed &= if graph {
                matches!(attribute.name(), "vertexkey" | "edgekey")
            } else {
                matches!(attribute.name(), "from" | "to")
            };
        }
        for child in node.children() {
            self.ledger.work(1)?;
            closed &= !child.is_element();
        }
        Ok(closed)
    }
    fn edge_fact(
        &mut self,
        from: Option<u32>,
        to: Option<u32>,
        recognized: bool,
    ) -> Result<(), String> {
        match (from, to) {
            (Some(from), Some(to)) => {
                if !recognized {
                    self.uncertain_ports.push(to, &mut self.ledger)?;
                }
                self.link(Key::Port(from), Key::Port(to))
            }
            (None, Some(to)) => {
                self.uncertain_ports.push(to, &mut self.ledger)?;
                self.link(Key::Port(to), Key::Port(to))
            }
            // In the recognized numeric edge grammar, no numeric endpoint
            // means no possible u32 destination. The legacy reader drops it.
            (_, None) if recognized => Ok(()),
            // An unsupported record shape with no attributable destination
            // cannot certify disjointness. Do not invent a numeric endpoint.
            _ => {
                self.unknown_edge = true;
                Ok(())
            }
        }
    }
    fn owner_index(&mut self, owner: RawOwner) -> Result<Option<usize>, String> {
        self.ledger.lookup(self.owners.0.len())?;
        Ok(self.owners.0.binary_search_by_key(&owner, |o| o.raw).ok())
    }
    pub(in crate::import) fn wants(&mut self, origin: Origin) -> Result<bool, String> {
        match origin {
            Origin::ActiveMain(owner) => Ok(self
                .owner_index(owner)?
                .is_some_and(|i| self.owners.0[i].wanted)),
            _ => Ok(false),
        }
    }
    fn graph(&mut self) -> Result<Graph, String> {
        Graph::build(&self.links.0, &mut self.ledger)
    }
    fn provisional_capture(&mut self) -> Result<(), String> {
        let graph = self.graph()?;
        let mut starts = Buffer::new();
        let has_filter = self.owners.0.iter().any(|o| o.filter);
        self.ledger.work(self.owners.0.len())?;
        if has_filter {
            for &port in &self.uncertain_ports.0 {
                starts.push(Key::Port(port), &mut self.ledger)?;
            }
        }
        for (i, owner) in self.owners.0.iter().enumerate() {
            if owner.filter || has_filter && (owner.uncertain || self.unknown_edge && owner.count) {
                starts.push(Key::Raw(i), &mut self.ledger)?;
            }
        }
        let affected = graph.reach(&starts.0, false, &mut self.ledger)?;
        let mut demanded = Buffer::new();
        for (i, owner) in self.owners.0.iter().enumerate() {
            if owner.count && graph.marked(&affected, Key::Raw(i), &mut self.ledger)? {
                demanded.push(Key::Raw(i), &mut self.ledger)?;
            }
        }
        let consumers = graph.reach(&demanded.0, false, &mut self.ledger)?;
        for (i, key) in graph.keys.0.iter().enumerate() {
            if consumers.0[i] && matches!(key, Key::Raw(_)) {
                demanded.push(*key, &mut self.ledger)?;
            }
        }
        let needed = graph.reach(&demanded.0, true, &mut self.ledger)?;
        for (i, owner) in self.owners.0.iter_mut().enumerate() {
            owner.wanted = graph.marked(&needed, Key::Raw(i), &mut self.ledger)?;
        }
        Ok(())
    }
    fn publish(&self) {
        publish_routing(self);
    }
}

pub(in crate::import) fn finish(
    handle: Option<&Handle>,
    n: &Normalized<'_>,
) -> Result<CompleteDemand, String> {
    let Some(handle) = handle else {
        // A complete conservative header is checked against actual final names;
        // a newly exposed Count never inherits a missing-capture Legacy route.
        for function in n.functions {
            publish_no_count_registration(function.name.len() + 1);
            if function.name == "count" && function.kind == 5 {
                return Err(
                    "physical filtered count: late registered Count has no negative capture".into(),
                );
            }
        }
        publish_no_count_complete();
        return Ok(CompleteDemand {
            seeds: Vec::new(),
            state: None,
        });
    };
    let mut state = handle.borrow_mut();
    state.ledger.check()?;
    fn replay<T: Fact>(state: &mut Routing, values: &RegisteredVec<T>) -> Result<(), String> {
        for (slot, value) in values.iter().enumerate() {
            state.register(value, Origin::SyntheticOrUnproved, slot, true)?;
        }
        Ok(())
    }
    replay(&mut state, n.schemas)?;
    replay(&mut state, n.functions)?;
    replay(&mut state, n.calls)?;
    replay(&mut state, n.external_scalar)?;
    replay(&mut state, n.external_xslt)?;
    replay(&mut state, n.json_serializers)?;
    replay(&mut state, n.xml_serializers)?;
    replay(&mut state, n.json_parsers)?;
    replay(&mut state, n.flextext_parsers)?;
    for key in n.joins.certificate_outputs() {
        state.ledger.extend_registered(1, 0)?;
        state.link(Key::Registered(10, key as usize), Key::Port(key))?;
    }
    for (&key, rules) in n.rules {
        state.ledger.extend_registered(1, 0)?;
        if !rules.is_empty() {
            state.link(Key::Registered(11, key as usize), Key::Port(key))?;
        }
    }
    let link_count = state.links.0.len();
    state.ledger.sort(link_count)?;
    state.links.0.sort_unstable();
    state.links.0.dedup();
    let graph = state.graph()?;
    let mut starts = Buffer::new();
    let mut safe = Buffer::new();
    for i in 0..state.filters.0.len() {
        let filter = state.filters.0[i];
        if state.ordinary_group_filter(filter.slot, n)?
            || state.variable_input_filter(filter.slot, n)?
        {
            safe.push(filter.slot, &mut state.ledger)?;
        } else {
            starts.push(Key::Registered(1, filter.slot), &mut state.ledger)?;
        }
    }
    let has_filter = !state.filters.0.is_empty() || state.owners.0.iter().any(|o| o.filter);
    if has_filter {
        for index in 0..state.uncertain_ports.0.len() {
            let port = state.uncertain_ports.0[index];
            starts.push(Key::Port(port), &mut state.ledger)?;
        }
        for &key in &graph.keys.0 {
            state.ledger.work(1)?;
            if matches!(key, Key::Registered(10 | 11, _)) {
                starts.push(key, &mut state.ledger)?;
            }
        }
    }
    let owner_count = state.owners.0.len();
    state.ledger.work(owner_count)?;
    for i in 0..state.owners.0.len() {
        let owner = state.owners.0[i];
        let work = state.filters.0.len() + safe.0.len();
        state.ledger.work(work)?;
        let safe_filter = state
            .filters
            .0
            .iter()
            .any(|f| f.raw == Some(owner.raw) && safe.0.contains(&f.slot));
        if owner.filter && !safe_filter
            || has_filter && (owner.uncertain || state.unknown_edge && owner.count)
        {
            starts.push(Key::Raw(i), &mut state.ledger)?;
        }
    }
    for i in 0..state.counts.0.len() {
        let count = state.counts.0[i];
        if count.raw.is_none() {
            starts.push(count.vertex, &mut state.ledger)?;
        }
    }
    let affected = graph.reach(&starts.0, false, &mut state.ledger)?;
    let mut seeds = Buffer::new();
    for i in 0..state.owners.0.len() {
        let owner = state.owners.0[i];
        if !owner.count {
            continue;
        }
        let count = state.counts.0.len();
        state.ledger.work(count)?;
        if state.counts.0.iter().any(|c| c.raw == Some(owner.raw)) {
            continue;
        }
        if graph.marked(&affected, Key::Raw(i), &mut state.ledger)? {
            state.seed(&graph, &[Key::Raw(i)], None, Some(owner.raw), &mut seeds)?;
        }
    }
    for i in 0..state.counts.0.len() {
        let count = state.counts.0[i];
        let raw = count
            .raw
            .map(|owner| state.owner_index(owner))
            .transpose()?
            .flatten();
        let mut affected_count = graph.marked(&affected, count.vertex, &mut state.ledger)?;
        if let Some(raw) = raw {
            affected_count |= graph.marked(&affected, Key::Raw(raw), &mut state.ledger)?;
        }
        if affected_count {
            let vertices = [count.vertex, raw.map_or(count.vertex, Key::Raw)];
            state.seed(&graph, &vertices, Some(count.slot), count.raw, &mut seeds)?;
        }
    }
    state.frozen = true;
    // No original edge map escaped its probe; these compact construction facts
    // also die at classification, before positive ownership/materialization.
    state.links.0.clear();
    state.links.0.shrink_to_fit();
    state.owners.0.clear();
    state.owners.0.shrink_to_fit();
    state.counts.0.clear();
    state.counts.0.shrink_to_fit();
    state.filters.0.clear();
    state.filters.0.shrink_to_fit();
    state.groups.0.clear();
    state.groups.0.shrink_to_fit();
    state.uncertain_ports.0.clear();
    state.uncertain_ports.0.shrink_to_fit();
    state.publish();
    Ok(CompleteDemand {
        seeds: seeds.0,
        state: Some(handle.clone()),
    })
}

impl Routing {
    fn seed(
        &mut self,
        graph: &Graph,
        vertices: &[Key],
        registration: Option<usize>,
        owner: Option<RawOwner>,
        seeds: &mut Buffer<DemandSeed>,
    ) -> Result<(), String> {
        let mut outputs = Buffer::new();
        for &(from, to) in &self.links.0 {
            self.ledger.work(vertices.len() + 1)?;
            if vertices.contains(&from)
                && let Key::Port(output) = to
            {
                outputs.push(output, &mut self.ledger)?;
            }
        }
        self.ledger.sort(outputs.0.len())?;
        outputs.0.sort_unstable();
        outputs.0.dedup();
        let mut connected = false;
        for &output in &outputs.0 {
            self.ledger.lookup(graph.forward.0.len())?;
            let key = Key::Port(output);
            let index = graph.forward.0.partition_point(|(from, _)| *from < key);
            connected |= graph
                .forward
                .0
                .get(index)
                .is_some_and(|(from, _)| *from == key);
        }
        if connected {
            seeds.push(
                DemandSeed {
                    registration,
                    owner,
                    outputs: outputs.0,
                },
                &mut self.ledger,
            )?;
        }
        Ok(())
    }
    /// A filter whose every output feeds only inputs of intermediate variable
    /// components selects that variable's items. Ordinary variable lowering
    /// keeps that selection, so Counts over the variable are not filtered-Count
    /// uses of this filter.
    fn variable_input_filter(&mut self, slot: usize, n: &Normalized<'_>) -> Result<bool, String> {
        self.ledger.work(1)?;
        let Some(function) = n.functions.get(slot) else {
            return Ok(false);
        };
        let mut consumed = false;
        for &output in function.output_pins.iter().flatten() {
            for &(from, to) in &self.links.0 {
                self.ledger.work(1)?;
                if from != Key::Port(output) {
                    continue;
                }
                let Key::Port(input) = to else {
                    return Ok(false);
                };
                self.ledger.lookup(n.schemas.len())?;
                if !n
                    .schemas
                    .iter()
                    .any(|schema| schema.is_variable && schema.input_keys.contains(&input))
                {
                    return Ok(false);
                }
                consumed = true;
            }
        }
        Ok(consumed)
    }
    fn ordinary_group_filter(&mut self, slot: usize, n: &Normalized<'_>) -> Result<bool, String> {
        self.ledger.work(4)?;
        let Some(function) = n.functions.get(slot) else {
            return Ok(false);
        };
        let Some(input) = function.inputs.first().copied().flatten() else {
            return Ok(false);
        };
        let mut feed = None;
        for &(from, to) in &self.links.0 {
            self.ledger.work(1)?;
            if to == Key::Port(input)
                && let Key::Port(from) = from
            {
                if feed.is_some_and(|feed| feed != from) {
                    return Ok(false);
                }
                feed = Some(from);
            }
        }
        let Some(feed) = feed else {
            return Ok(false);
        };
        for i in 0..self.groups.0.len() {
            self.ledger.work(1)?;
            let group = self.groups.0[i];
            if group.key != feed {
                continue;
            }
            self.ledger.lookup(self.owners.0.len())?;
            let Some(raw) = group
                .raw
                .and_then(|owner| self.owners.0.binary_search_by_key(&owner, |o| o.raw).ok())
            else {
                continue;
            };
            let source = &n.schemas[group.slot];
            self.ledger
                .lookup(self.ledger.dimensions.pins + self.ledger.dimensions.registered)?;
            self.ledger
                .lookup(self.ledger.dimensions.pins + self.ledger.dimensions.registered)?;
            if !(source.format == ComponentFormat::Json || source.is_variable)
                || !source.unambiguous_declared_output(feed)
            {
                continue;
            }
            self.ledger.lookup(source.ports.len())?;
            let Some(path) = source.ports.get(&feed) else {
                continue;
            };
            let mut node = &source.schema;
            let mut valid = true;
            for segment in path {
                let SchemaKind::Group { children, .. } = &node.kind else {
                    valid = false;
                    break;
                };
                let mut selected = None;
                for child in children {
                    self.ledger
                        .extend_registered(1, child.name.len() + segment.len())?;
                    self.ledger.text(child.name.len() + segment.len())?;
                    if child.name == *segment {
                        if selected.is_some() {
                            valid = false;
                        }
                        selected = Some(child);
                    }
                }
                let Some(child) = selected else {
                    valid = false;
                    break;
                };
                node = child;
            }
            if !valid || !matches!(node.kind, SchemaKind::Group { .. }) {
                continue;
            }
            for &(from, to) in &self.links.0 {
                self.ledger.work(1)?;
                if to == Key::Port(feed)
                    && matches!(from, Key::Raw(_) | Key::Registered(_, _))
                    && from != Key::Raw(raw)
                    && from != Key::Registered(0, group.slot)
                {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        Ok(false)
    }
}

struct Graph {
    keys: Buffer<Key>,
    forward: Buffer<(Key, Key)>,
    reverse: Buffer<(Key, Key)>,
}
impl Graph {
    fn build(links: &[(Key, Key)], ledger: &mut Ledger) -> Result<Self, String> {
        let mut graph = Self {
            keys: Buffer::new(),
            forward: Buffer::new(),
            reverse: Buffer::new(),
        };
        for &(from, to) in links {
            graph.keys.push(from, ledger)?;
            graph.keys.push(to, ledger)?;
            graph.forward.push((from, to), ledger)?;
            graph.reverse.push((to, from), ledger)?;
        }
        ledger.sort(graph.keys.0.len())?;
        graph.keys.0.sort_unstable();
        graph.keys.0.dedup();
        ledger.sort(graph.forward.0.len())?;
        graph.forward.0.sort_unstable();
        graph.forward.0.dedup();
        ledger.sort(graph.reverse.0.len())?;
        graph.reverse.0.sort_unstable();
        graph.reverse.0.dedup();
        Ok(graph)
    }
    fn marked(&self, marks: &Buffer<bool>, key: Key, ledger: &mut Ledger) -> Result<bool, String> {
        ledger.lookup(self.keys.0.len())?;
        Ok(self.keys.0.binary_search(&key).is_ok_and(|i| marks.0[i]))
    }
    fn reach(
        &self,
        starts: &[Key],
        reverse: bool,
        ledger: &mut Ledger,
    ) -> Result<Buffer<bool>, String> {
        let links = if reverse {
            &self.reverse.0
        } else {
            &self.forward.0
        };
        let mut marks = Buffer::new();
        marks.reserve(self.keys.0.len(), ledger)?;
        ledger.work(self.keys.0.len())?;
        marks.0.resize(self.keys.0.len(), false);
        let mut queue = Buffer::new();
        for &key in starts {
            self.enqueue(key, &mut marks, &mut queue, ledger)?;
        }
        let mut cursor = 0;
        while cursor < queue.0.len() {
            let key = queue.0[cursor];
            cursor += 1;
            ledger.lookup(links.len())?;
            let start = links.partition_point(|(from, _)| *from < key);
            for &(from, to) in &links[start..] {
                ledger.work(1)?;
                if from != key {
                    break;
                }
                self.enqueue(to, &mut marks, &mut queue, ledger)?;
            }
        }
        Ok(marks)
    }
    fn enqueue(
        &self,
        key: Key,
        marks: &mut Buffer<bool>,
        queue: &mut Buffer<Key>,
        ledger: &mut Ledger,
    ) -> Result<(), String> {
        ledger.lookup(self.keys.0.len())?;
        if let Ok(i) = self.keys.0.binary_search(&key)
            && !marks.0[i]
        {
            queue.push(key, ledger)?;
            marks.0[i] = true;
        }
        Ok(())
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default)]
pub(in crate::import) struct Report {
    pub header_work: usize,
    pub dimensions: Dimensions,
    pub work: u128,
    pub text_work: u128,
    pub slots: usize,
    pub bytes: usize,
    pub limits: (u128, usize, usize),
    pub frozen: bool,
    pub retained_graph_slots: usize,
}
#[cfg(test)]
thread_local! { static REPORT: std::cell::Cell<Report> = const { std::cell::Cell::new(Report { header_work: 0, dimensions: Dimensions { nodes: 0, walk_nodes: 0, pins: 0, edges: 0, attributes: 0, decoded: 0, registered: 0 }, work: 0, text_work: 0, slots: 0, bytes: 0, limits: (0, 0, 0), frozen: false, retained_graph_slots: 0 }) }; }
fn publish_header(header: Headers) {
    #[cfg(test)]
    REPORT.set(Report {
        header_work: header.work,
        dimensions: header.dimensions,
        limits: Ledger::new(header.dimensions).limits(),
        ..Report::default()
    });
    #[cfg(not(test))]
    let _ = header;
}
fn publish_no_count_registration(work: usize) {
    count_visit(Counter::NegativeWork);
    #[cfg(test)]
    REPORT.with(|report| {
        let mut value = report.get();
        value.work += work as u128;
        value.dimensions.registered += 1;
        value.limits = Ledger::new(value.dimensions).limits();
        report.set(value);
    });
    #[cfg(not(test))]
    let _ = work;
}
fn publish_no_count_complete() {
    #[cfg(test)]
    REPORT.with(|report| {
        let mut value = report.get();
        value.frozen = true;
        report.set(value);
    });
}
fn publish_routing(state: &Routing) {
    #[cfg(test)]
    REPORT.with(|report| {
        report.set(Report {
            header_work: report.get().header_work,
            dimensions: state.ledger.dimensions,
            work: state.ledger.work,
            text_work: state.ledger.text_work,
            slots: state.ledger.slots,
            bytes: state.ledger.bytes,
            limits: state.ledger.limits(),
            frozen: state.frozen,
            retained_graph_slots: state.links.0.capacity()
                + state.owners.0.capacity()
                + state.counts.0.capacity()
                + state.filters.0.capacity()
                + state.groups.0.capacity()
                + state.uncertain_ports.0.capacity(),
        })
    });
    #[cfg(not(test))]
    let _ = state;
}
#[cfg(test)]
pub(in crate::import) fn report() -> Report {
    REPORT.get()
}
