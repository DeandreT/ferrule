//! Reader-backed filtered Count authority. None of this proof survives import;
//! saved ordinary graphs retain the ordinary host-input contract.
use crate::import::{
    function::{self, FnComponent},
    graph::{EdgeInputs, GraphBuilder},
    schema::{ComponentFormat, SchemaComponent},
    scope::ScopeBuilder,
    udf::certificate::{
        CountConstruction, CountProducer, CountProducerView, Normalized, Origin, RawOwner,
        RegisteredVec,
    },
};
use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{AggregateOp, Node, NodeId};
use std::collections::{BTreeMap, BTreeSet};

mod discovery;
mod raw;
mod receiver;
pub(in crate::import) mod routing;
mod seal;
#[cfg(test)]
pub(in crate::import) mod tests;
use receiver::Mode;
pub(in crate::import) use seal::CountSeal;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct UseId {
    target: usize,
    input: u32,
}
struct DirectUse {
    candidate: usize,
    path: Vec<String>,
    drivers: BTreeMap<Vec<String>, Vec<String>>,
}
struct Candidate {
    registration: Option<usize>,
    raw_owner: Option<RawOwner>,
    outputs: Vec<u32>,
    connected: bool,
    plan: Result<Plan, String>,
}
struct Plan {
    owner: Vec<String>,
    suffix: Vec<String>,
    repeated_scalar: bool,
    exclude: bool,
    predicate: Predicate,
}
enum Predicate {
    Field(ScalarType),
    Literal(Value),
    Call {
        name: &'static str,
        args: Vec<Predicate>,
        ty: ScalarType,
    },
}
impl Predicate {
    fn ty(&self) -> ScalarType {
        match self {
            Self::Field(ty) | Self::Call { ty, .. } => *ty,
            Self::Literal(Value::Bool(_)) => ScalarType::Bool,
            _ => ScalarType::String,
        }
    }
}

pub(in crate::import) struct CountProofTable {
    candidates: Vec<Candidate>,
    candidate_outputs: routing::OutputIndex,
    uses: BTreeMap<UseId, DirectUse>,
    targets: BTreeMap<usize, usize>,
    budget: Option<CountConstruction>,
    source: Option<[u8; 32]>,
    source_options: Option<[u8; 32]>,
    target_schemas: BTreeMap<usize, [u8; 32]>,
    target_options: BTreeMap<usize, [u8; 32]>,
    failure: Option<String>,
    discovery_incomplete: bool,
}

pub(in crate::import) enum CountBinding {
    Legacy,
    Handled(NodeId),
    Refused,
}

impl CountProofTable {
    #[cfg(test)]
    pub(in crate::import) fn empty_for_test() -> &'static Self {
        Box::leak(Box::new(Self::EMPTY))
    }
    pub(in crate::import) const EMPTY: Self = Self {
        candidates: Vec::new(),
        candidate_outputs: routing::OutputIndex::EMPTY,
        uses: BTreeMap::new(),
        targets: BTreeMap::new(),
        budget: None,
        source: None,
        source_options: None,
        target_schemas: BTreeMap::new(),
        target_options: BTreeMap::new(),
        failure: None,
        discovery_incomplete: false,
    };
    pub(in crate::import) fn owns_index(&self, index: usize) -> bool {
        self.candidates
            .iter()
            .any(|c| c.registration == Some(index))
    }
    fn candidate(&self, feed: u32) -> Option<Result<usize, String>> {
        self.candidate_outputs
            .get(&feed)
            .map(|ids| match ids.as_slice() {
                [id] => Ok(*id),
                _ => Err("colliding candidate Count outputs".into()),
            })
    }
    pub(in crate::import) fn compile(
        structure: roxmltree::Node<'_, '_>,
        edges: &EdgeInputs,
        n: &Normalized<'_>,
        capture: Option<&CountConstruction>,
        demand: routing::CompleteDemand,
        ordinary_reader: bool,
    ) -> Self {
        let mut table = Self::EMPTY;
        if let Err(reason) = discovery::discover(&mut table, demand) {
            table.discovery_incomplete = true;
            table.failure = Some(reason);
            return table;
        }
        if table.candidates.is_empty() {
            return table;
        }
        let missing_capture;
        let budget = match capture {
            Some(budget) => budget,
            None => {
                // A normalized/promoted Count missed by the optional header
                // trigger gets no fabricated origins. Discovery must retain
                // any affected output as an explicit missing-origin refusal.
                missing_capture = CountConstruction::default();
                &missing_capture
            }
        };
        table.budget = Some(budget.clone());
        if !ordinary_reader {
            table.failure = Some("filtered Count requires an ordinary reader boundary, not a pipeline boundary value".into());
        }
        let reconcile = (|| {
            let raw = raw::Raw::build(structure, edges, budget);
            count_visit(Counter::FinalBuild);
            let view = CountProducerView::build(n, budget);
            let raw = raw?;
            let view = view?;
            for id in 0..table.candidates.len() {
                let result = (|| {
                    let index = table.candidates[id]
                        .registration
                        .ok_or("unregistered Count")?;
                    if table.candidates[id].raw_owner.is_none()
                        || table.candidates[id].raw_owner.map(Origin::ActiveMain)
                            != Some(n.functions.count_origin_at(index))
                    {
                        return Err("Count negative and positive owner identities differ".into());
                    }
                    let compiler = Compiler {
                        n,
                        raw: &raw,
                        view: &view,
                        edges,
                        budget,
                    };
                    let plan = compiler.plan(index)?;
                    for &output in &table.candidates[id].outputs {
                        for &input in raw.outgoing.get(&output).into_iter().flatten() {
                            let pin = raw.pin(input, false, budget)?;
                            let mut selected = None;
                            for (slot, &target) in n.targets.iter().enumerate() {
                                if n.schemas.count_origin_of_for_count(target, budget)?
                                    == Origin::ActiveMain(RawOwner::of(pin.owner))
                                    && selected.replace((slot, target)).is_some()
                                {
                                    return Err("ambiguous receiving target identity".into());
                                }
                            }
                            let (slot, target) =
                                selected.ok_or("indirect or unregistered Count consumer")?;
                            let path = raw::path(pin, budget, &target.schema.name)?;
                            let leaf = target_node(&target.schema, &path, budget)?;
                            if !target.unambiguous_declared_input(input)
                                || target.ports.get(&input) != Some(&path)
                                || !leaf.is_scalar()
                                || leaf.repeating
                                || leaf.attribute != raw::attribute(pin, budget)?
                                || target.is_variable
                                || target.is_pass_through
                                || target.dynamic_json.is_some()
                                || target.db_xml_columns.contains_key(&input)
                            {
                                return Err("unproved direct scalar Count target".into());
                            }
                            let mut drivers = BTreeMap::new();
                            for length in 0..path.len() {
                                let prefix = &path[..length];
                                for (&key, target_path) in &target.ports {
                                    budget.claim(1, 0, 0)?;
                                    if target_path != prefix {
                                        continue;
                                    }
                                    if let Some(&feed) = edges.feeds.get(&key) {
                                        let target_pin = raw.pin(key, false, budget)?;
                                        if target_pin.owner != pin.owner
                                            || raw::path(target_pin, budget, &target.schema.name)?
                                                != prefix
                                            || target_node(&target.schema, prefix, budget)?
                                                .attribute
                                                != raw::attribute(target_pin, budget)?
                                        {
                                            return Err("receiver input identity mismatch".into());
                                        }
                                        let source_path = compiler.source(feed)?;
                                        if drivers
                                            .insert(copy_path(prefix, budget)?, source_path)
                                            .is_some()
                                        {
                                            return Err("duplicate receiver driver".into());
                                        }
                                    }
                                }
                            }
                            budget.claim(3, 0, 0)?;
                            table
                                .targets
                                .insert(std::ptr::from_ref(target) as usize, slot);
                            if let std::collections::btree_map::Entry::Vacant(entry) =
                                table.target_schemas.entry(slot)
                            {
                                // Reserve the bounded schema copy before projection.
                                seal::fingerprint(&target.schema, budget)?;
                                let schema =
                                    crate::import::runtime_target_schema(target, &edges.feeds);
                                entry.insert(seal::fingerprint(&schema, budget)?);
                                seal::fingerprint(&target.options, budget)?;
                                table.target_options.insert(
                                    slot,
                                    seal::fingerprint(
                                        &crate::import::target_format_options(target, &edges.feeds),
                                        budget,
                                    )?,
                                );
                            }
                            let use_id = UseId {
                                target: slot,
                                input,
                            };
                            if table
                                .uses
                                .insert(
                                    use_id,
                                    DirectUse {
                                        candidate: id,
                                        path,
                                        drivers,
                                    },
                                )
                                .is_some()
                            {
                                return Err("duplicate Count use".into());
                            }
                        }
                    }
                    Ok(plan)
                })();
                table.candidates[id].plan = result;
            }
            // Saved scope hints are untrusted before legacy last-wins collapse.
            for metadata in structure
                .children()
                .filter(|n| n.has_tag_name("ferrule-scope-sources"))
            {
                for hint in metadata.children().filter(roxmltree::Node::is_element) {
                    budget.claim(1 + hint.attributes().len(), 0, 0)?;
                    let key = crate::import::schema::parse_u32(hint.attribute("targetkey"))
                        .ok_or("unattributable raw scope hint")?;
                    for (&id, use_) in &table.uses {
                        budget.claim(1, 0, 0)?;
                        if n.targets[id.target].ports.get(&key).is_some_and(|path| {
                            path.len() < use_.path.len() && use_.path.starts_with(path)
                        }) {
                            return Err(
                                "affected saved scope hints require unsupported authority".into()
                            );
                        }
                    }
                }
            }
            if let Some(primary) = n.sources.first() {
                table.source = Some(seal::fingerprint(&primary.schema, budget)?);
                table.source_options = Some(seal::fingerprint(&primary.options, budget)?);
            }
            Ok::<_, String>(())
        })();
        if let Err(reason) = reconcile {
            table.failure = Some(reason);
        }
        table
    }
}

struct Compiler<'a, 'doc, 'xml> {
    n: &'a Normalized<'a>,
    raw: &'a raw::Raw<'doc, 'xml>,
    view: &'a CountProducerView<'a>,
    edges: &'a EdgeInputs,
    budget: &'a CountConstruction,
}
impl Compiler<'_, '_, '_> {
    fn producer(&self, feed: u32) -> Result<CountProducer<'_>, String> {
        let pin = self.raw.pin(feed, true, self.budget)?;
        self.view.get(feed, RawOwner::of(pin.owner), self.budget)
    }
    fn source(&self, feed: u32) -> Result<Vec<String>, String> {
        let CountProducer::Source(source, path) = self.producer(feed)? else {
            return Err("not a direct physical source".into());
        };
        if self.n.sources.len() != 1
            || !std::ptr::eq(source, self.n.sources[0])
            || !matches!(source.format, ComponentFormat::Xml | ComponentFormat::Edi)
            || !source.is_source
            || source.schema.repeating
            || source.is_variable
            || source.is_pass_through
            || source.compute_when_key.is_some()
            || source.dynamic_json.is_some()
            || source.has_target_node_controls
            || !source.input_keys.is_empty()
            || source.options.external_source.is_some()
            || source.options.local_xml_file_set
            || source.options.http_get.is_some()
            || source.options.xml_allow_inactive_root_type_members
            || source.options.xml_root_view_read_policy
            || source.options.wsdl.is_some()
            || !source.db_queries.is_empty()
            || !source.unambiguous_declared_output(feed)
        {
            return Err("requires one ordinary primary reader source".into());
        }
        if source.format == ComponentFormat::Edi
            && !source
                .port_declarations
                .as_ref()
                .is_some_and(|f| f.complete_x12_config())
        {
            return Err("requires configured X12 reader".into());
        }
        if raw::path(
            self.raw.pin(feed, true, self.budget)?,
            self.budget,
            &source.schema.name,
        )? != path
        {
            return Err("raw/final physical path mismatch".into());
        }
        let node = exact_node(&source.schema, path, self.budget)?;
        if node.attribute != raw::attribute(self.raw.pin(feed, true, self.budget)?, self.budget)? {
            return Err("raw/registered scalar or group attribute profile differs".into());
        }
        copy_path(path, self.budget)
    }
    fn function(&self, feed: u32) -> Result<&FnComponent, String> {
        let CountProducer::Function(f) = self.producer(feed)? else {
            return Err("not an ordinary function".into());
        };
        self.budget.claim(
            1,
            f.name
                .len()
                .checked_add(f.library.len())
                .ok_or("Count function metadata overflow")?,
            0,
        )?;
        let origin = Origin::ActiveMain(RawOwner::of(self.raw.pin(feed, true, self.budget)?.owner));
        self.raw.function(f, origin, self.budget)?;
        Ok(f)
    }
    fn input(&self, f: &FnComponent, position: usize) -> Result<u32, String> {
        self.budget.claim(1, 0, 0)?;
        f.inputs
            .get(position)
            .copied()
            .flatten()
            .and_then(|k| self.edges.feeds.get(&k).copied())
            .ok_or("missing required Count closure input".into())
    }
    fn plan(&self, index: usize) -> Result<Plan, String> {
        let count = &self.n.functions[index];
        if !count.raw_filtered_count_signature_valid {
            return Err("malformed Count signature".into());
        }
        self.raw
            .function(count, self.n.functions.count_origin_at(index), self.budget)?;
        for &key in &count.outputs {
            self.function(key)?;
        }
        let sequence = self.input(count, 1)?;
        let filter = self.function(sequence)?;
        if !function::is_filter(filter) || !filter.raw_filtered_count_pins_valid {
            return Err("requires exact direct filter".into());
        }
        let position = filter
            .output_pins
            .iter()
            .position(|k| *k == Some(sequence))
            .filter(|i| *i < 2)
            .ok_or("unproved filter output")?;
        let selected = self.input(filter, 0)?;
        let path = self.source(selected)?;
        let source = self.n.sources[0];
        let leaf = exact_node(&source.schema, &path, self.budget)?;
        if !matches!(
            leaf.kind,
            SchemaKind::Scalar {
                ty: ScalarType::String | ScalarType::Bool
            }
        ) {
            return Err("counted value is not String or Bool".into());
        }
        let mut split = 0;
        for length in 1..=path.len() {
            if exact_node(&source.schema, &path[..length], self.budget)?.repeating {
                split = length;
            }
        }
        if split == 0 || source.format == ComponentFormat::Edi && leaf.repeating {
            return Err("unproved repeated owner or repeating X12 scalar".into());
        }
        let owner = copy_path(&path[..split], self.budget)?;
        let suffix = copy_path(&path[split..], self.budget)?;
        let mut visited = 0;
        let predicate = self.predicate(
            self.input(filter, 1)?,
            &path,
            0,
            &mut BTreeSet::new(),
            &mut visited,
        )?;
        if predicate.ty() != ScalarType::Bool {
            return Err("filter predicate is not Bool".into());
        }
        Ok(Plan {
            owner,
            suffix,
            repeated_scalar: leaf.repeating,
            exclude: position == 1,
            predicate,
        })
    }
    fn predicate(
        &self,
        feed: u32,
        selected: &[String],
        depth: usize,
        active: &mut BTreeSet<u32>,
        visited: &mut usize,
    ) -> Result<Predicate, String> {
        self.budget.claim(1, 0, depth)?;
        count_visit(Counter::Closure);
        *visited += 1;
        if depth > 32 || *visited > 128 || !active.insert(feed) {
            return Err("cyclic or oversized Count predicate".into());
        }
        let result = (|| {
            if let CountProducer::Source(_, _) = self.producer(feed)? {
                let path = self.source(feed)?;
                if path != selected {
                    return Err("independent predicate field".into());
                }
                let leaf = exact_node(&self.n.sources[0].schema, &path, self.budget)?;
                if !leaf.effective_xml_view().is_ok_and(|xml| !xml.nil()) {
                    return Err("nil-reading Count predicate".into());
                }
                let SchemaKind::Scalar { ty } = leaf.kind else {
                    return Err("non-scalar predicate".into());
                };
                return Ok(Predicate::Field(ty));
            }
            let f = self.function(feed)?;
            if f.library == "core" && f.kind == 2 && f.name == "constant" {
                if !f.raw_literal_valid
                    || !f.bounded_scalar_pins_valid
                    || !f.raw_single_output_valid
                {
                    return Err("malformed predicate literal".into());
                }
                let (text, datatype) = f.constant.as_ref().ok_or("missing literal")?;
                self.budget.claim(1, text.len() + datatype.len(), depth)?;
                let value = function::parse_constant(text, datatype);
                return if matches!(value, Value::String(_) | Value::Bool(_)) {
                    Ok(Predicate::Literal(value))
                } else {
                    Err("unsupported literal type".into())
                };
            }
            if f.kind != 5 || !f.raw_filtered_count_pins_valid || !f.raw_single_output_valid {
                return Err("unproved predicate operator".into());
            }
            use ScalarType::{Bool, String as Text};
            let (name, types, ty): (_, &[ScalarType], _) =
                match (f.library.as_str(), function::map_component_name(f)) {
                    ("core", Some("equal")) => ("equal", &[Text, Text], Bool),
                    ("core", Some("not_equal")) => ("not_equal", &[Text, Text], Bool),
                    ("core", Some("and")) => ("and", &[Bool, Bool], Bool),
                    ("core", Some("or")) => ("or", &[Bool, Bool], Bool),
                    ("core", Some("not")) => ("not", &[Bool], Bool),
                    ("core" | "lang", Some("normalize_space")) => {
                        ("normalize_space", &[Text], Text)
                    }
                    ("core" | "lang", Some("trim")) => ("trim", &[Text], Text),
                    ("core" | "lang", Some("is_empty")) => ("is_empty", &[Text], Bool),
                    _ => return Err("unsupported Count predicate operator".into()),
                };
            self.budget.claim(types.len(), 0, depth)?;
            let mut args = Vec::new();
            for (position, &expected) in types.iter().enumerate() {
                let arg = self.predicate(
                    self.input(f, position)?,
                    selected,
                    depth + 1,
                    active,
                    visited,
                )?;
                if arg.ty() != expected {
                    return Err("Count predicate type mismatch".into());
                }
                args.push(arg);
            }
            Ok(Predicate::Call { name, args, ty })
        })();
        active.remove(&feed);
        result
    }
}

fn checked_path(path: &[String], budget: &CountConstruction) -> Result<(), String> {
    let bytes = path
        .iter()
        .try_fold(0usize, |n, s| n.checked_add(s.len()))
        .ok_or("Count path overflow")?;
    budget.claim(path.len() + 1, bytes, path.len())?;
    if bytes > 4096 {
        return Err("Count path exceeds 4 KiB".into());
    }
    Ok(())
}

fn non_count_header(owner: roxmltree::Node<'_, '_>) -> bool {
    owner
        .children()
        .filter(|n| n.has_tag_name("data"))
        .any(|data| {
            matches!(owner.attribute("library"), Some("xml" | "json" | "text"))
                && data.children().any(|n| n.has_tag_name("root"))
                || owner.attribute("kind") == Some("19")
                    && data
                        .children()
                        .filter(|n| n.has_tag_name("root"))
                        .flat_map(|n| n.children())
                        .any(|n| n.has_tag_name("entry") && n.attribute("componentid").is_some())
                || owner.attribute("library") == Some("core")
                    && match owner.attribute("kind") {
                        Some("2") => data.children().any(|n| n.has_tag_name("constant")),
                        Some("6") => data.children().any(|n| n.has_tag_name("input")),
                        Some("7") => data.children().any(|n| n.has_tag_name("output")),
                        _ => false,
                    }
        })
}

fn copy_path(path: &[String], budget: &CountConstruction) -> Result<Vec<String>, String> {
    checked_path(path, budget)?;
    Ok(path.to_vec())
}
fn joined_path(
    left: &[String],
    right: &[String],
    budget: &CountConstruction,
) -> Result<Vec<String>, String> {
    let length = left
        .len()
        .checked_add(right.len())
        .ok_or("Count path depth overflow")?;
    let bytes = left
        .iter()
        .chain(right)
        .try_fold(0usize, |n, s| n.checked_add(s.len()))
        .ok_or("Count path text overflow")?;
    budget.claim(length + 1, bytes, length)?;
    if bytes > 4096 {
        return Err("Count path exceeds 4 KiB".into());
    }
    let mut path = left.to_vec();
    path.extend_from_slice(right);
    Ok(path)
}
fn exact_node<'a>(
    root: &'a SchemaNode,
    path: &[String],
    budget: &CountConstruction,
) -> Result<&'a SchemaNode, String> {
    checked_path(path, budget)?;
    let valid = |node: &SchemaNode| {
        !node.json_any
            && node.recursive_ref.is_none()
            && node.dynamic_fields().is_none()
            && node.alternatives().is_empty()
            && node.xml_name_alternatives.is_empty()
            && !node.xml_type_alternatives
            && !node.container_nullable
            && !(node.nillable && matches!(node.kind, SchemaKind::Group { .. }))
            && !matches!(
                node.name.as_str(),
                ir::XML_ATTRIBUTES_FIELD | ir::XML_ELEMENTS_FIELD
            )
            && !(node.repeating && (node.attribute || node.text || node.name == ir::XML_TEXT_FIELD))
            && node.json_object_item_nullable != Some(true)
            && !node.json_array_wrapper
            && !matches!(node.xml_namespace, Some(ir::XmlNamespace::Qualified(_)))
    };
    let mut node = root;
    for segment in path {
        if !valid(node) || node.nillable {
            return Err("unproved schema ancestry".into());
        }
        let SchemaKind::Group { children, .. } = &node.kind else {
            return Err("non-group schema ancestry".into());
        };
        budget.claim(children.len() + 1, segment.len(), 0)?;
        let mut found = children.iter().filter(|n| n.name == *segment);
        node = found.next().ok_or("missing schema path")?;
        if found.next().is_some() {
            return Err("duplicate effective schema child".into());
        }
    }
    if !valid(node) {
        return Err("unsupported schema endpoint".into());
    }
    Ok(node)
}

fn target_node<'a>(
    root: &'a SchemaNode,
    path: &[String],
    budget: &CountConstruction,
) -> Result<&'a SchemaNode, String> {
    let plain = |node: &SchemaNode| {
        !node.json_any
            && node.recursive_ref.is_none()
            && node.alternatives().is_empty()
            && node.xml_name_alternatives.is_empty()
            && !node.xml_type_alternatives
            && !matches!(node.xml_namespace, Some(ir::XmlNamespace::Qualified(_)))
    };
    checked_path(path, budget)?;
    let mut node = root;
    for segment in path {
        if !plain(node) {
            return Err("nonstatic Count target path".into());
        }
        let SchemaKind::Group { children, .. } = &node.kind else {
            return Err("nongroup Count target ancestry".into());
        };
        budget.claim(children.len() + 1, segment.len(), 0)?;
        let mut found = children.iter().filter(|n| n.name == *segment);
        node = found.next().ok_or("missing target schema path")?;
        if found.next().is_some() {
            return Err("ambiguous target schema path".into());
        }
    }
    if !plain(node) {
        return Err("nonstatic Count target leaf".into());
    }
    Ok(node)
}

#[derive(Clone, Copy)]
pub(in crate::import) enum Counter {
    RawBuild,
    RawElement,
    RawPin,
    Edge,
    FinalBuild,
    Closure,
    Receiver,
    Audit,
    Template,
    Lower,
    Header,
    Grammar,
    Origin,
    OriginLookup,
    FinalClaim,
    QuotaFailure,
    EdgeIndex,
    Speculative,
    RefusalElement,
    RefusalPin,
    NegativeHeader,
    NegativeBuild,
    NegativeWork,
    NegativeReservation,
}
pub(in crate::import) fn count_visit(counter: Counter) {
    #[cfg(test)]
    COUNTERS.with(|c| {
        c.borrow_mut()[counter as usize] += 1;
    });
    #[cfg(not(test))]
    let _ = counter;
}
#[cfg(test)]
thread_local! { static COUNTERS: std::cell::RefCell<[usize; 24]> = const { std::cell::RefCell::new([0; 24]) }; }

impl GraphBuilder<'_> {
    pub(in crate::import) fn owns_filtered_count_index(&self, index: usize) -> bool {
        self.filtered_counts.owns_index(index)
    }
    pub(in crate::import) fn refuse_filtered_count(&mut self, reason: &str) -> String {
        let message = format!("physical filtered count: {reason}");
        if self.materialized_counts.failure.is_none() {
            self.materialized_counts.failure = Some(message.clone());
        }
        message
    }
    pub(in crate::import) fn filtered_count_template(
        &mut self,
        index: usize,
    ) -> Result<Node, String> {
        let table = self.filtered_counts;
        let result = (|| {
            let budget = table.budget.as_ref().ok_or("missing Count budget")?;
            self.materialized_counts.budget = Some(budget.clone());
            if let Some(reason) = &table.failure {
                return Err(reason.clone());
            }
            let candidate = table
                .candidates
                .iter()
                .find(|c| c.registration == Some(index))
                .ok_or("missing Count candidate")?;
            let plan = candidate.plan.as_ref().map_err(String::clone)?;
            let proof = receiver::root(&self.sources[0].schema, plan, budget)?;
            let Mode::Sum(_) = proof.mode else {
                return Err("root Count template is not Sum".into());
            };
            count_visit(Counter::Template);
            let start = self.next_id;
            let node = lower(self, plan, &proof.mode, budget)?;
            budget.claim((self.next_id - start) as usize, 0, 0)?;
            self.materialized_counts
                .templates
                .extend(start..self.next_id);
            Ok(node)
        })();
        result.map_err(|e: String| self.refuse_filtered_count(&e))
    }
    pub(in crate::import) fn filtered_count_binding(
        &mut self,
        target: &SchemaComponent,
        input: u32,
        feed: u32,
        path: &[String],
        scopes: &ScopeBuilder,
    ) -> CountBinding {
        let table = self.filtered_counts;
        let Some(candidate) = table.candidate(feed) else {
            return CountBinding::Legacy;
        };
        #[cfg(test)]
        if let Some(result) = tests::probe_binding(self, target, input, feed, path, scopes) {
            return result;
        }
        let result = (|| {
            let id = candidate?;
            let candidate = &table.candidates[id];
            if let Some(index) = candidate.registration {
                if self.fn_by_output.get(&feed) != Some(&index)
                    || self.source_abs_path(feed).is_some()
                {
                    return Err(
                        "candidate Count is not the unchanged selected aggregate route".into(),
                    );
                }
                match self.aggregate_earlier(AggregateOp::Count, index) {
                    Err(_) => {
                        // The unchanged earlier recognizer owns this failure;
                        // retain its ordinary cached refusal and warning once.
                        let node = self.fn_node(index);
                        self.materialized_counts.earlier(index, node);
                        return Ok(node);
                    }
                    Ok(super::AggregateRoute::Existing(Some(node))) => {
                        let node = self.alloc(node);
                        self.materialized_counts.earlier(index, node);
                        return Ok(node);
                    }
                    Ok(super::AggregateRoute::Existing(None)) => {
                        return Err("unresolved candidate sequence".into());
                    }
                    Ok(super::AggregateRoute::Physical { .. }) => {}
                }
            }
            let budget = table.budget.as_ref().ok_or("missing Count budget")?;
            self.materialized_counts.budget = Some(budget.clone());
            budget.claim(1, 0, 0)?;
            if let Some(reason) = &table.failure {
                return Err(reason.clone());
            }
            let plan = candidate.plan.as_ref().map_err(String::clone)?;
            let slot = *table
                .targets
                .get(&(std::ptr::from_ref(target) as usize))
                .ok_or("unproved receiving target slot")?;
            let use_id = UseId {
                target: slot,
                input,
            };
            let use_ = table
                .uses
                .get(&use_id)
                .ok_or("unrecorded authored Count use")?;
            if use_.candidate != id || use_.path != path {
                return Err("Count authored use changed".into());
            }
            let proof = receiver::prove(
                &self.sources[0].schema,
                plan,
                use_,
                &scopes.root,
                Some(&scopes.anchors),
                budget,
            )?;
            count_visit(Counter::Lower);
            let start = self.next_id;
            let node = lower(self, plan, &proof.mode, budget)?;
            budget.claim(1, 0, 0)?;
            let root = self.alloc(node);
            let mut nodes = BTreeMap::new();
            for id in start..self.next_id {
                budget.claim(1, 0, 0)?;
                nodes.insert(
                    id,
                    seal::fingerprint(
                        self.graph
                            .nodes
                            .get(&id)
                            .ok_or("missing lowered Count node")?,
                        budget,
                    )?,
                );
            }
            if self
                .materialized_counts
                .receipts
                .insert(
                    use_id,
                    seal::Receipt {
                        root,
                        nodes,
                        facts: proof.facts,
                        mode: proof.mode,
                    },
                )
                .is_some()
            {
                return Err("Count use installed twice".into());
            }
            Ok(root)
        })();
        match result {
            Ok(node) => CountBinding::Handled(node),
            Err(reason) => {
                self.refuse_filtered_count(&reason);
                CountBinding::Refused
            }
        }
    }
}

fn lower(
    builder: &mut GraphBuilder<'_>,
    plan: &Plan,
    mode: &Mode,
    budget: &CountConstruction,
) -> Result<Node, String> {
    fn predicate(
        builder: &mut GraphBuilder<'_>,
        plan: &Plan,
        p: &Predicate,
        budget: &CountConstruction,
        depth: usize,
    ) -> Result<NodeId, String> {
        budget.claim(2, 0, depth)?;
        let node = match p {
            Predicate::Field(_) => Node::SourceField {
                frame: Some(copy_path(&plan.owner, budget)?),
                path: copy_path(&plan.suffix, budget)?,
            },
            Predicate::Literal(value) => {
                budget.claim(
                    1,
                    if let Value::String(s) = value {
                        s.len()
                    } else {
                        0
                    },
                    depth,
                )?;
                Node::Const {
                    value: value.clone(),
                }
            }
            Predicate::Call { name, args, .. } => {
                budget.claim(args.len(), name.len(), depth)?;
                let args = args
                    .iter()
                    .map(|p| predicate(builder, plan, p, budget, depth + 1))
                    .collect::<Result<Vec<_>, _>>()?;
                Node::Call {
                    function: (*name).into(),
                    args,
                }
            }
        };
        Ok(builder.alloc(node))
    }
    let mut condition = predicate(builder, plan, &plan.predicate, budget, 0)?;
    budget.claim(8, 32, 0)?;
    if plan.exclude {
        condition = builder.alloc(Node::Call {
            function: "not".into(),
            args: vec![condition],
        });
    }
    let one = builder.alloc(Node::Const {
        value: Value::Int(1),
    });
    let zero = builder.alloc(Node::Const {
        value: Value::Int(0),
    });
    let contribution = Node::If {
        condition,
        then: one,
        else_: zero,
    };
    let expression = if plan.repeated_scalar {
        contribution
    } else {
        let contribution = builder.alloc(contribution);
        let field = builder.alloc(Node::SourceField {
            frame: Some(copy_path(&plan.owner, budget)?),
            path: copy_path(&plan.suffix, budget)?,
        });
        let presence = builder.alloc(Node::Call {
            function: "exists".into(),
            args: vec![field],
        });
        Node::If {
            condition: presence,
            then: contribution,
            else_: zero,
        }
    };
    Ok(match mode {
        Mode::CurrentItem => expression,
        Mode::Sum(driver) => {
            let expression = builder.alloc(expression);
            Node::Aggregate {
                function: AggregateOp::Sum,
                collection: copy_path(driver, budget)?,
                value: Vec::new(),
                expression: Some(expression),
                arg: None,
            }
        }
    })
}
