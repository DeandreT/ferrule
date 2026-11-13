//! An inert, source-bound CSV declaration witness, not executable admission.
//!
//! This companion never resolves a path or constructs mapping nodes. Unknown
//! XML survives as source ranges; even a complete declaration witness carries
//! unqualified semantics. Only the new grouped CSV composition gets a warning.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ops::Range;

use roxmltree::{Node, NodeType};
use sha2::{Digest, Sha256};

const MAX_BYTES: usize = 1024 * 1024;
const MAX_ELEMENTS: usize = 8192;
const MAX_NODES: usize = 16384;
const MAX_DEPTH: usize = 64;
const MAX_COMPONENTS: usize = 128;
const MAX_PORTS: usize = 2048;
const MAX_EDGES: usize = 4096;
const MAX_COLUMNS: usize = 256;
const MAX_ATTRIBUTES: usize = 32;
const MAX_SLOTS: usize = 64;
const MAX_VALUE: usize = 64 * 1024;
const MAX_FINDINGS: usize = 128;
const PROTOCOL: &str = "http://www.altova.com/mapforce";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    NotClaimed,
    GroupedCsvComposition,
    Uncertain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FindingKind {
    SourceMismatch,
    LimitExceeded,
    MalformedToken,
    UnsupportedEnvelope,
    UnknownXml,
    UnknownControl,
    DuplicateContainer,
    DuplicateComponent,
    DuplicatePort,
    AmbiguousEndpoint,
    MalformedWrapper,
    NamespaceMismatch,
    ConflictingSetting,
    InvalidColumnOrder,
    DuplicateColumn,
    UnsupportedColumnType,
    InvalidLiteral,
    InvalidParameter,
    InvalidFunctionSignature,
    DanglingEdge,
    WrongPortDirection,
    DuplicateEdgeClaim,
    ConflictingFeed,
    UnmatchedAnnotation,
    GraphCycle,
}

#[derive(Debug)]
struct RawFinding {
    kind: FindingKind,
    origin: Range<usize>,
    related: Vec<Range<usize>>,
    limit: Option<(&'static str, usize, usize)>,
    owners: Vec<u32>,
    ports: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Classification {
    Unknown,
    Syntax,
    Identity,
    Declaration,
    Layout,
    Descriptive,
    SavedGeneration,
    UnqualifiedAnnotation,
}

// These fields are intentionally retained witnesses, not inputs to lowering.
// Specific record-level allowances keep this inert data out of runtime APIs.
#[allow(
    dead_code,
    reason = "Retained lexical witness; no executable consumer exists"
)]
#[derive(Clone, Debug)]
struct LexicalValue<'text> {
    raw: &'text str,
    decoded: String,
    origin: Range<usize>,
}

#[allow(
    dead_code,
    reason = "Retained attribute identity and classification witness"
)]
#[derive(Debug)]
struct RawAttribute<'text> {
    name: String,
    namespace: Option<String>,
    origin: Range<usize>,
    value: LexicalValue<'text>,
    classification: Classification,
}

#[allow(
    dead_code,
    reason = "Local xmlns declarations are not ordinary XML attributes"
)]
#[derive(Debug)]
struct NamespaceBinding<'text> {
    prefix: &'text str,
    uri: String,
    origin: Range<usize>,
    raw_value: &'text str,
    classification: Classification,
}

#[derive(Debug)]
struct RawNode<'doc, 'text> {
    xml: Node<'doc, 'text>,
    parent: Option<usize>,
    owner: Option<usize>,
    depth: usize,
    children: Vec<usize>,
    attributes: Vec<RawAttribute<'text>>,
    namespaces: Vec<NamespaceBinding<'text>>,
    classification: Classification,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Input,
    Output,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PortRole {
    Unclassified,
    Slot,
    Filename,
    Row,
    Column,
}

#[derive(Debug)]
struct Port {
    key: u32,
    owner: Option<usize>,
    node: usize,
    direction: Direction,
    role: PortRole,
    origin: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Vocabulary {
    Modern,
    Legacy,
}

#[derive(Debug)]
struct EdgeClaim {
    from: Option<u32>,
    to: Option<u32>,
    annotation: Option<u32>,
    node: usize,
    vocabulary: Vocabulary,
    origin: Range<usize>,
}

#[allow(
    dead_code,
    reason = "Raw placeholder ordinal is distinct from authored position/key"
)]
#[derive(Clone, Debug)]
struct Slot {
    ordinal: usize,
    position: Option<u32>,
    key: Option<u32>,
    position_presence: Presence,
    key_presence: Presence,
    node: usize,
    origin: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Presence {
    Missing,
    Empty,
    Present,
}

fn presence(value: Option<&str>) -> Presence {
    match value {
        None => Presence::Missing,
        Some("") => Presence::Empty,
        Some(_) => Presence::Present,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CallKind {
    GroupBy,
    Sum,
    AutoNumber,
    Now,
    Empty,
    RightTrim,
    SubstringBefore,
    IfElse,
    SubstituteNull,
    Concat,
    FormatDateTime,
}

#[allow(
    dead_code,
    reason = "Complete raw operand/receiver signature, not normalized function IR"
)]
#[derive(Debug)]
struct Callable {
    owner: usize,
    kind: CallKind,
    inputs: Vec<Slot>,
    outputs: Vec<Slot>,
}

#[allow(
    dead_code,
    reason = "Literal text is retained privately and never formatted into warnings"
)]
#[derive(Debug)]
struct Literal<'text> {
    owner: usize,
    output: Option<u32>,
    value: LexicalValue<'text>,
    datatype: Option<LexicalValue<'text>>,
    origin: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flag {
    Missing,
    Empty,
    Invalid,
    Zero,
    One,
}

#[allow(
    dead_code,
    reason = "Raw missing attributes and preview are not runtime defaults"
)]
#[derive(Debug)]
struct Parameter<'text> {
    owner: usize,
    output: Option<u32>,
    name: LexicalValue<'text>,
    datatype: Option<LexicalValue<'text>>,
    usage: Option<LexicalValue<'text>>,
    required: Flag,
    optional: Flag,
    preview: Option<LexicalValue<'text>>,
    use_preview: Flag,
    source_slots: Vec<Slot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EndpointRole {
    Source,
    Target,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RecordTerminator {
    NotDeclared,
    ExplicitUnqualified,
}

#[allow(
    dead_code,
    reason = "Typed physical declarations remain unqualified, not writer settings"
)]
#[derive(Debug)]
struct Transport<'text> {
    controls: BTreeMap<String, LexicalValue<'text>>,
    encoding: Option<u32>,
    byteorder: Option<u32>,
    bom: Flag,
    separator: char,
    quote: Option<char>,
    header: bool,
    remove_empty: bool,
    record_terminator: RecordTerminator,
    preview_instance: Option<LexicalValue<'text>>,
}

#[allow(
    dead_code,
    reason = "Inline and leaf origins/order must survive later qualification"
)]
#[derive(Debug)]
struct Column<'text> {
    index: usize,
    xml_ordinal: usize,
    name: LexicalValue<'text>,
    declared_type: LexicalValue<'text>,
    port: Option<u32>,
    declaration_origin: Range<usize>,
    leaf_origin: Range<usize>,
}

#[allow(
    dead_code,
    reason = "Endpoint wrapper/schema witness is not an executable boundary"
)]
#[derive(Debug)]
struct Endpoint<'text> {
    owner: usize,
    role: EndpointRole,
    filename: Option<u32>,
    row: Option<u32>,
    root: usize,
    wrapper: usize,
    document: usize,
    row_node: usize,
    namespace_table: Vec<Option<LexicalValue<'text>>>,
    inline_root: LexicalValue<'text>,
    inline_block: LexicalValue<'text>,
    columns: Vec<Column<'text>>,
    transport: Transport<'text>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObligationKind {
    GroupReceiverOwnership,
    AggregateFrameAndStringConversion,
    ScalarPresenceAndEvaluation,
    ClockAndDatePicture,
    NumberingScope,
    FilenameAcquisitionAndComposedOutput,
    InputPhysicalSemantics,
    OutputPhysicalSemantics,
    DisabledQuotePhysicalBoundary,
    RecordTerminatorNotDeclared,
    SavedGenerationPathPolicy,
    UnqualifiedSavedAnnotationControl,
    GeneratedAndHostComposition,
}

#[allow(
    dead_code,
    reason = "All operand and receiver origins are retained pending qualification"
)]
#[derive(Debug)]
pub(super) struct SemanticObligation {
    kind: ObligationKind,
    owner: usize,
    ports: Vec<u32>,
    edge_claims: Vec<usize>,
    origins: Vec<Range<usize>>,
    // No qualified state or escape hatch is provided by this companion.
}

#[allow(
    dead_code,
    reason = "Inert census retains the exact source and uncensused ranges"
)]
#[derive(Debug)]
pub(super) struct RawCensus<'doc, 'text> {
    source: &'text str,
    digest: Option<[u8; 32]>,
    grammar_version: u32,
    complete: bool,
    family: Family,
    unexamined_ranges: Vec<Range<usize>>,
    nodes: Vec<RawNode<'doc, 'text>>,
    line_starts: Vec<usize>,
    markup_events: Vec<(Range<usize>, Classification)>,
    components: BTreeMap<u32, Vec<usize>>,
    component_uids: BTreeMap<usize, u32>,
    ports: Vec<Port>,
    ports_by_key: BTreeMap<u32, Vec<usize>>,
    port_record_at: BTreeMap<(usize, &'static str), usize>,
    edges: Vec<EdgeClaim>,
    edge_at: BTreeMap<usize, usize>,
    incoming: BTreeMap<u32, Vec<usize>>,
    outgoing: BTreeMap<u32, Vec<usize>>,
    endpoints: Vec<Endpoint<'text>>,
    literals: Vec<Literal<'text>>,
    parameters: Vec<Parameter<'text>>,
    functions: Vec<Callable>,
    findings: Vec<RawFinding>,
    suppressed_findings: usize,
    element_visits: usize,
    edge_visits: usize,
    probe_port_visits: usize,
    probe_edge_visits: usize,
    role_record_visits: usize,
}

#[derive(Debug)]
pub(super) struct RawCsvDeclarationProof<'doc, 'text> {
    census: RawCensus<'doc, 'text>,
}

#[derive(Debug)]
pub(super) struct PendingObligations {
    items: Vec<SemanticObligation>,
}

impl PendingObligations {
    fn new(first: SemanticObligation, remaining: impl Iterator<Item = SemanticObligation>) -> Self {
        Self {
            items: std::iter::once(first).chain(remaining).collect(),
        }
    }

    fn as_slice(&self) -> &[SemanticObligation] {
        &self.items
    }
}

#[derive(Debug)]
pub(super) enum ApplicabilityReason {
    NoInlineCsvPair,
}

#[derive(Debug)]
pub(super) enum CsvDeclarationAssessment<'doc, 'text> {
    NotApplicable {
        reason: ApplicabilityReason,
    },
    Refused {
        partial: RawCensus<'doc, 'text>,
    },
    DeclarationOnly {
        proof: RawCsvDeclarationProof<'doc, 'text>,
        pending: PendingObligations,
    },
}

impl CsvDeclarationAssessment<'_, '_> {
    pub(super) fn blocks_executable(&self) -> bool {
        match self {
            Self::NotApplicable {
                reason: ApplicabilityReason::NoInlineCsvPair,
            } => false,
            Self::Refused { partial } => partial.family == Family::GroupedCsvComposition,
            Self::DeclarationOnly { proof, .. } => {
                proof.census.family == Family::GroupedCsvComposition
            }
        }
    }

    pub(super) fn append_warnings(&self, warnings: &mut Vec<String>) {
        if !self.blocks_executable() {
            return;
        }
        // Deliberately exclude names, paths, literals, preview and annotation text.
        match self {
            Self::Refused { partial } => {
                for finding in &partial.findings {
                    warnings.push(format!(
                        "CSV declaration proof: {:?} at bytes {}..{}; {} related origins, {} owners, {} ports; grouped CSV composition remains unqualified{}",
                        finding.kind, finding.origin.start, finding.origin.end, finding.related.len(),finding.owners.len(),finding.ports.len(),
                        finding.limit.map(|(resource,actual,limit)|format!("; {resource} {actual} exceeds {limit}")).unwrap_or_default()
                    ));
                }
                if partial.suppressed_findings != 0 {
                    warnings.push(format!(
                        "CSV declaration proof: {} further findings suppressed; census incomplete",
                        partial.suppressed_findings
                    ));
                }
            }
            Self::DeclarationOnly { pending, .. } => warnings.push(format!(
                "CSV declaration proof: declaration-only; {} semantic/physical obligations remain unqualified; grouped CSV composition is not executable",
                pending.as_slice().len()
            )),
            Self::NotApplicable { .. } => {}
        }
    }
}

impl<'doc, 'text> RawCensus<'doc, 'text> {
    fn new(source: &'text str) -> Self {
        Self {
            source,
            digest: None,
            grammar_version: 1,
            complete: true,
            family: Family::Uncertain,
            unexamined_ranges: Vec::new(),
            nodes: Vec::new(),
            line_starts: vec![0],
            markup_events: Vec::new(),
            components: BTreeMap::new(),
            component_uids: BTreeMap::new(),
            ports: Vec::new(),
            ports_by_key: BTreeMap::new(),
            port_record_at: BTreeMap::new(),
            edges: Vec::new(),
            edge_at: BTreeMap::new(),
            incoming: BTreeMap::new(),
            outgoing: BTreeMap::new(),
            endpoints: Vec::new(),
            literals: Vec::new(),
            parameters: Vec::new(),
            functions: Vec::new(),
            findings: Vec::new(),
            suppressed_findings: 0,
            element_visits: 0,
            edge_visits: 0,
            probe_port_visits: 0,
            probe_edge_visits: 0,
            role_record_visits: 0,
        }
    }

    // A capped finding returns an out-of-range handle. Callers must never attach
    // its metadata to the previously stored finding (especially at a limit).
    fn finding(&mut self, kind: FindingKind, origin: Range<usize>) -> usize {
        if self.findings.len() == MAX_FINDINGS {
            self.suppressed_findings += 1;
            self.complete = false;
            if self.unexamined_ranges.is_empty() {
                self.unexamined_ranges.push(origin.start..self.source.len());
            }
            MAX_FINDINGS
        } else {
            let index = self.findings.len();
            self.findings.push(RawFinding {
                kind,
                origin,
                related: Vec::new(),
                limit: None,
                owners: Vec::new(),
                ports: Vec::new(),
            });
            index
        }
    }

    fn at(&mut self, kind: FindingKind, node: usize) {
        self.at_index(kind, node);
    }

    fn at_index(&mut self, kind: FindingKind, node: usize) -> usize {
        let index = self.finding(kind, self.nodes[node].xml.range());
        if index == MAX_FINDINGS {
            return index;
        }
        let mut owners = BTreeSet::new();
        if let Some(uid) = self.nodes[node]
            .owner
            .and_then(|owner| self.component_uids.get(&owner))
        {
            owners.insert(*uid);
        }
        let mut ports = BTreeSet::new();
        for attr in ["key", "inpkey", "outkey"] {
            if let Some(&ordinal) = self.port_record_at.get(&(node, attr)) {
                ports.insert(self.ports[ordinal].key);
            }
        }
        if let Some(&edge) = self.edge_at.get(&node) {
            ports.extend(self.edges[edge].from);
            ports.extend(self.edges[edge].to);
        }
        for key in &ports {
            if let Some(indices) = self.ports_by_key.get(key).filter(|p| p.len() == 1)
                && let Some(uid) = self.ports[indices[0]]
                    .owner
                    .and_then(|owner| self.component_uids.get(&owner))
            {
                owners.insert(*uid);
            }
        }
        self.findings[index].owners = owners.into_iter().collect();
        self.findings[index].ports = ports.into_iter().collect();
        index
    }

    fn stop(&mut self, resource: &'static str, actual: usize, limit: usize, at: usize) {
        self.complete = false;
        self.unexamined_ranges
            .push(at.min(self.source.len())..self.source.len());
        let index = self.finding(FindingKind::LimitExceeded, at..at);
        if let Some(last) = self.findings.get_mut(index) {
            last.limit = Some((resource, actual, limit));
        }
    }

    fn tag(&self, i: usize) -> &str {
        self.nodes[i].xml.tag_name().name()
    }
    fn attr(&self, i: usize, name: &str) -> Option<&str> {
        self.nodes[i].xml.attribute(name)
    }

    fn value(&self, i: usize, name: &str) -> Option<LexicalValue<'text>> {
        self.nodes[i]
            .attributes
            .iter()
            .find(|a| a.name == name && a.namespace.is_none())
            .map(|a| a.value.clone())
    }

    fn children(&self, i: usize, tag: &str) -> Vec<usize> {
        self.nodes[i]
            .children
            .iter()
            .copied()
            .filter(|&j| self.tag(j) == tag)
            .collect()
    }

    fn one(&mut self, i: usize, tag: &str, required: bool) -> Option<usize> {
        let children = self.children(i, tag);
        match children.as_slice() {
            [child] => Some(*child),
            [] => {
                if required {
                    self.at(FindingKind::UnsupportedEnvelope, i);
                }
                None
            }
            _ => {
                let index = self.at_index(FindingKind::DuplicateContainer, i);
                if let Some(f) = self.findings.get_mut(index) {
                    f.related = children
                        .iter()
                        .map(|&j| self.nodes[j].xml.range())
                        .collect();
                }
                None
            }
        }
    }

    fn shape(&mut self, i: usize, class: Classification, attrs: &[&str]) {
        self.nodes[i].classification = if self.nodes[i].xml.tag_name().namespace().is_none() {
            class
        } else {
            Classification::Unknown
        };
        for n in 0..self.nodes[i].attributes.len() {
            let a = &self.nodes[i].attributes[n];
            if a.namespace.is_none() && attrs.contains(&a.name.as_str()) {
                self.nodes[i].attributes[n].classification = class;
            }
        }
    }

    fn attribute_class(&mut self, i: usize, name: &str, class: Classification) {
        for attribute in &mut self.nodes[i].attributes {
            if attribute.namespace.is_none() && attribute.name == name {
                attribute.classification = class;
            }
        }
    }

    fn leaf(&mut self, i: usize, class: Classification, attrs: &[&str]) {
        self.shape(i, class, attrs);
        if !self.nodes[i].children.is_empty() {
            self.at(FindingKind::UnknownXml, i);
        }
    }

    fn exact(&mut self, i: usize, name: &str, value: &str, kind: FindingKind) {
        if self.attr(i, name) != Some(value) {
            self.at(kind, i);
        }
    }

    fn token(&mut self, i: usize, name: &str, nonzero: bool) -> Option<u32> {
        let value = self.attr(i, name)?;
        let parsed = canonical(value, nonzero);
        if parsed.is_none() {
            self.at(FindingKind::MalformedToken, i);
        }
        parsed
    }

    fn flag(&mut self, i: usize, name: &str, only: Option<Flag>) -> Flag {
        let flag = match self.attr(i, name) {
            None => Flag::Missing,
            Some("0") => Flag::Zero,
            Some("1") => Flag::One,
            Some("") => {
                self.at(FindingKind::InvalidParameter, i);
                Flag::Empty
            }
            _ => {
                self.at(FindingKind::InvalidParameter, i);
                Flag::Invalid
            }
        };
        if flag != Flag::Missing && only.is_some_and(|expected| flag != expected) {
            self.at(FindingKind::InvalidParameter, i);
        }
        flag
    }

    fn port(&mut self, node: usize, name: &'static str, role: PortRole) -> Option<u32> {
        let ordinal = *self.port_record_at.get(&(node, name))?;
        // Count the actual single record examination/update, not a derived
        // estimate. Duplicate-key alternatives remain in ports_by_key, but
        // assigning this declaration's role must never walk those alternatives.
        self.role_record_visits += 1;
        let port = &mut self.ports[ordinal];
        port.role = role;
        Some(port.key)
    }

    fn slots(&mut self, owner: usize, tag: &str) -> Vec<Slot> {
        let Some(container) = self.one(owner, tag, false) else {
            return Vec::new();
        };
        self.shape(container, Classification::Declaration, &[]);
        let points = self.nodes[container].children.clone();
        if points.len() > MAX_SLOTS {
            self.stop(
                "slots",
                points.len(),
                MAX_SLOTS,
                self.nodes[container].xml.range().start,
            );
            return Vec::new();
        }
        let mut result = Vec::new();
        let mut positions = BTreeMap::new();
        for (ordinal, point) in points.into_iter().enumerate() {
            if self.tag(point) != "datapoint" {
                self.at(FindingKind::InvalidFunctionSignature, point);
                continue;
            }
            self.leaf(point, Classification::Declaration, &["pos", "key"]);
            let position = self.token(point, "pos", false);
            let key = self.port(point, "key", PortRole::Slot);
            if key.is_some() != position.is_some()
                || position.is_some_and(|p| p as usize >= MAX_SLOTS)
            {
                self.at(FindingKind::InvalidFunctionSignature, point);
            }
            if key.is_none() && !self.nodes[point].attributes.is_empty() {
                self.at(FindingKind::InvalidFunctionSignature, point);
            }
            if let Some(pos) = position
                && positions.insert(pos, point).is_some()
            {
                self.at(FindingKind::InvalidFunctionSignature, point);
            }
            result.push(Slot {
                ordinal,
                position,
                key,
                position_presence: presence(self.attr(point, "pos")),
                key_presence: presence(self.attr(point, "key")),
                node: point,
                origin: self.nodes[point].xml.range(),
            });
        }
        result
    }

    fn component_layout(&mut self, i: usize) {
        if let Some(view) = self.one(i, "view", false) {
            self.leaf(view, Classification::Layout, &["ltx", "lty", "rbx", "rby"]);
            for name in ["ltx", "lty", "rbx", "rby"] {
                if let Some(value) = self.attr(view, name) {
                    let digits = value.strip_prefix('-').unwrap_or(value);
                    if canonical(digits, false).is_none()
                        || value.parse::<i32>().is_err()
                        || value == "-0"
                    {
                        self.at(FindingKind::MalformedToken, view);
                    }
                }
            }
        }
    }

    fn empty_properties(&mut self, i: usize) {
        if let Some(properties) = self.one(i, "properties", false) {
            self.leaf(properties, Classification::Declaration, &[]);
        }
    }
}

fn canonical(value: &str, nonzero: bool) -> Option<u32> {
    if value.is_empty()
        || !value.bytes().all(|b| b.is_ascii_digit())
        || value.len() > 1 && value.starts_with('0')
    {
        return None;
    }
    value.parse::<u32>().ok().filter(|&v| !nonzero || v != 0)
}

// Scan only source markup, skipping quoted values and comments. This catches
// local xmlns declarations and controls which the parsed tree can omit/merge.
#[derive(Debug)]
struct LexAttr {
    name: Range<usize>,
    value: Range<usize>,
    whole: Range<usize>,
}

fn markup(c: &mut RawCensus<'_, '_>) -> Option<BTreeMap<usize, Vec<LexAttr>>> {
    let bytes = c.source.as_bytes();
    let mut headers = BTreeMap::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'<' {
            at += 1;
            continue;
        }
        let start = at;
        let tail = &c.source[at..];
        if tail.starts_with("<!--") {
            let end = at + tail.find("-->")? + 3;
            if end - at > MAX_VALUE {
                c.stop("comment bytes", end - at, MAX_VALUE, at);
                return None;
            }
            at = end;
            continue;
        }
        if tail.starts_with("<![CDATA[") {
            if c.markup_events.len() == MAX_NODES {
                c.stop("markup controls", c.markup_events.len() + 1, MAX_NODES, at);
                return None;
            }
            let end = at + tail.find("]]>")? + 3;
            c.finding(FindingKind::UnknownXml, at..end);
            c.markup_events.push((at..end, Classification::Unknown));
            at = end;
            continue;
        }
        if tail.starts_with("<?") {
            if c.markup_events.len() == MAX_NODES {
                c.stop("markup controls", c.markup_events.len() + 1, MAX_NODES, at);
                return None;
            }
            let end = at + tail.find("?>")? + 2;
            let declaration = (at == 0 || at == 3 && bytes.starts_with(&[239, 187, 191]))
                && tail.starts_with("<?xml ");
            if !declaration {
                c.finding(FindingKind::UnknownXml, at..end);
            }
            c.markup_events.push((
                at..end,
                if declaration {
                    Classification::Syntax
                } else {
                    Classification::Unknown
                },
            ));
            at = end;
            continue;
        }
        if tail.starts_with("<!") {
            if c.markup_events.len() == MAX_NODES {
                c.stop("markup controls", c.markup_events.len() + 1, MAX_NODES, at);
                return None;
            }
            // DTDs may contain quoted '>' and an internal subset. Never resolve them.
            let mut quote = None;
            let mut subset = 0usize;
            at += 2;
            while at < bytes.len() {
                let b = bytes[at];
                if quote == Some(b) {
                    quote = None;
                } else if quote.is_none() {
                    match b {
                        b'\'' | b'"' => quote = Some(b),
                        b'[' => subset += 1,
                        b']' => subset = subset.saturating_sub(1),
                        b'>' if subset == 0 => {
                            at += 1;
                            break;
                        }
                        _ => {}
                    }
                }
                at += 1;
            }
            c.finding(FindingKind::UnknownXml, start..at);
            c.markup_events.push((start..at, Classification::Unknown));
            continue;
        }
        at += 1;
        if bytes.get(at) == Some(&b'/') {
            while at < bytes.len() && bytes[at] != b'>' {
                at += 1;
            }
            at += usize::from(at < bytes.len());
            continue;
        }
        while at < bytes.len()
            && !bytes[at].is_ascii_whitespace()
            && !matches!(bytes[at], b'/' | b'>')
        {
            at += 1;
        }
        let mut attrs = Vec::new();
        while at < bytes.len() {
            while at < bytes.len() && bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            if at == bytes.len() || matches!(bytes[at], b'/' | b'>') {
                break;
            }
            if attrs.len() == MAX_ATTRIBUTES {
                c.stop("attributes", attrs.len() + 1, MAX_ATTRIBUTES, at);
                return None;
            }
            let name_start = at;
            while at < bytes.len() && !bytes[at].is_ascii_whitespace() && bytes[at] != b'=' {
                at += 1;
            }
            let name_end = at;
            while at < bytes.len() && bytes[at] != b'=' {
                at += 1;
            }
            at += 1;
            while at < bytes.len() && bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            let quote = *bytes.get(at)?;
            if !matches!(quote, b'\'' | b'"') {
                return None;
            }
            at += 1;
            let value_start = at;
            while at < bytes.len() && bytes[at] != quote {
                at += 1;
            }
            if at - value_start > MAX_VALUE || name_end - name_start > MAX_VALUE {
                c.stop(
                    "attribute bytes",
                    (at - value_start).max(name_end - name_start),
                    MAX_VALUE,
                    value_start,
                );
                return None;
            }
            attrs.push(LexAttr {
                name: name_start..name_end,
                value: value_start..at,
                whole: name_start..at + 1,
            });
            at += 1;
        }
        while at < bytes.len() && bytes[at] != b'>' {
            at += 1;
        }
        at += usize::from(at < bytes.len());
        if headers.len() == MAX_ELEMENTS {
            c.stop("elements", headers.len() + 1, MAX_ELEMENTS, start);
            return None;
        }
        headers.insert(start, attrs);
    }
    Some(headers)
}

pub(super) fn inspect<'doc, 'text>(
    mapping: Node<'doc, 'text>,
    raw_text: &'text str,
) -> CsvDeclarationAssessment<'doc, 'text> {
    let mut c = RawCensus::new(raw_text);
    let actual = mapping.document().input_text();
    if !std::ptr::eq(actual.as_ptr(), raw_text.as_ptr())
        || actual.len() != raw_text.len()
        || mapping != mapping.document().root_element()
        || !mapping.is_element()
    {
        c.complete = false;
        c.unexamined_ranges.push(0..raw_text.len());
        c.finding(FindingKind::SourceMismatch, 0..raw_text.len());
        return CsvDeclarationAssessment::Refused { partial: c };
    }
    if raw_text.len() > MAX_BYTES {
        c.stop("source bytes", raw_text.len(), MAX_BYTES, 0);
        return CsvDeclarationAssessment::Refused { partial: c };
    }
    c.digest = Some(Sha256::digest(raw_text.as_bytes()).into());
    c.line_starts.extend(
        raw_text
            .bytes()
            .enumerate()
            .filter_map(|(i, b)| (b == b'\n').then_some(i + 1)),
    );
    let Some(mut headers) = markup(&mut c) else {
        if c.complete {
            c.complete = false;
            c.unexamined_ranges.push(0..raw_text.len());
            c.finding(FindingKind::UnknownXml, 0..raw_text.len());
        }
        return CsvDeclarationAssessment::Refused { partial: c };
    };
    let mut ids = BTreeMap::<u32, usize>::new();
    let mut component_count = 0;
    for xml in mapping.document().root().descendants() {
        if c.nodes.len() == MAX_NODES {
            c.stop("XML nodes", c.nodes.len() + 1, MAX_NODES, xml.range().start);
            break;
        }
        let parent = xml.parent().and_then(|p| ids.get(&p.id().get()).copied());
        let depth = parent.map_or(0, |p| c.nodes[p].depth) + usize::from(xml.is_element());
        if depth > MAX_DEPTH {
            c.stop("element depth", depth, MAX_DEPTH, xml.range().start);
            break;
        }
        if xml.is_element() {
            c.element_visits += 1;
            if c.element_visits > MAX_ELEMENTS {
                c.stop(
                    "elements",
                    c.element_visits,
                    MAX_ELEMENTS,
                    xml.range().start,
                );
                break;
            }
        }
        let i = c.nodes.len();
        let is_component = xml.is_element() && xml.tag_name().name() == "component";
        let owner = if is_component {
            Some(i)
        } else {
            parent.and_then(|p| c.nodes[p].owner)
        };
        let mut record = RawNode {
            xml,
            parent,
            owner,
            depth,
            children: Vec::new(),
            attributes: Vec::new(),
            namespaces: Vec::new(),
            classification: Classification::Unknown,
        };
        if xml.is_element() {
            let lexical = headers.remove(&xml.range().start).unwrap_or_default();
            let by_start: BTreeMap<_, _> = xml.attributes().map(|a| (a.range().start, a)).collect();
            for attr in lexical {
                let name = &raw_text[attr.name.clone()];
                let value = &raw_text[attr.value.clone()];
                if name == "xmlns" || name.starts_with("xmlns:") {
                    let prefix = name.strip_prefix("xmlns:").unwrap_or("");
                    let uri = xml
                        .lookup_namespace_uri((!prefix.is_empty()).then_some(prefix))
                        .unwrap_or("")
                        .to_owned();
                    let known = xml == mapping
                        && prefix == "xsi"
                        && uri == "http://www.w3.org/2001/XMLSchema-instance";
                    record.namespaces.push(NamespaceBinding {
                        prefix,
                        uri,
                        origin: attr.whole.clone(),
                        raw_value: value,
                        classification: if known {
                            Classification::Syntax
                        } else {
                            Classification::Unknown
                        },
                    });
                    if !known {
                        c.finding(FindingKind::NamespaceMismatch, attr.whole);
                    }
                } else if let Some(decoded) = by_start.get(&attr.whole.start) {
                    if decoded.value().len() > MAX_VALUE {
                        c.stop(
                            "decoded value bytes",
                            decoded.value().len(),
                            MAX_VALUE,
                            attr.value.start,
                        );
                        break;
                    }
                    record.attributes.push(RawAttribute {
                        name: decoded.name().to_owned(),
                        namespace: decoded.namespace().map(str::to_owned),
                        origin: attr.whole,
                        value: LexicalValue {
                            raw: value,
                            decoded: decoded.value().to_owned(),
                            origin: attr.value,
                        },
                        classification: Classification::Unknown,
                    });
                } else {
                    c.finding(FindingKind::UnknownXml, attr.whole);
                }
            }
            if xml.tag_name().namespace().is_some() {
                c.finding(FindingKind::UnknownXml, xml.range());
            }
        } else {
            record.classification = match xml.node_type() {
                NodeType::Root => Classification::Syntax,
                NodeType::Comment => Classification::Descriptive,
                NodeType::Text
                    if xml.text().is_some_and(|s| {
                        s.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
                    }) =>
                {
                    Classification::Syntax
                }
                _ => {
                    c.finding(FindingKind::UnknownXml, xml.range());
                    Classification::Unknown
                }
            };
            if xml.text().is_some_and(|s| s.len() > MAX_VALUE) {
                c.stop(
                    "text bytes",
                    xml.text().map_or(0, str::len),
                    MAX_VALUE,
                    xml.range().start,
                );
            }
        }
        c.nodes.push(record);
        ids.insert(xml.id().get(), i);
        if let Some(p) = parent
            && xml.is_element()
        {
            c.nodes[p].children.push(i);
        }
        if !c.complete {
            break;
        }
        if is_component {
            component_count += 1;
            if component_count > MAX_COMPONENTS {
                c.stop(
                    "components",
                    component_count,
                    MAX_COMPONENTS,
                    xml.range().start,
                );
                break;
            }
            if let Some(uid) = c.token(i, "uid", true) {
                c.component_uids.insert(i, uid);
                c.components.entry(uid).or_default().push(i);
            } else {
                c.at(FindingKind::MalformedToken, i);
            }
        }
        for name in ["key", "inpkey", "outkey"] {
            if xml.is_element() && xml.attribute(name).is_some() {
                if c.ports.len() == MAX_PORTS {
                    c.stop("ports", c.ports.len() + 1, MAX_PORTS, xml.range().start);
                    break;
                }
                if let Some(key) = c.token(i, name, true) {
                    let direction = match name {
                        "inpkey" => Direction::Input,
                        "outkey" => Direction::Output,
                        _ => match parent.map(|p| c.tag(p)) {
                            Some("sources") => Direction::Input,
                            Some("targets") => Direction::Output,
                            _ => Direction::Unknown,
                        },
                    };
                    let p = c.ports.len();
                    c.ports.push(Port {
                        key,
                        owner,
                        node: i,
                        direction,
                        role: PortRole::Unclassified,
                        origin: xml.range(),
                    });
                    c.ports_by_key.entry(key).or_default().push(p);
                    c.port_record_at.insert((i, name), p);
                }
            }
        }
        if !c.complete {
            break;
        }
        let annotation_definition = parent.is_some_and(|p| {
            c.tag(p) == "edges" && c.nodes[p].parent.is_some_and(|g| c.tag(g) == "graph")
        });
        if xml.is_element()
            && xml.tag_name().name() == "edge"
            && (!annotation_definition
                || xml.attribute("from").is_some()
                || xml.attribute("to").is_some())
        {
            if c.edges.len() == MAX_EDGES {
                c.stop(
                    "edge claims",
                    c.edges.len() + 1,
                    MAX_EDGES,
                    xml.range().start,
                );
                break;
            }
            let modern = parent.is_some_and(|p| {
                c.tag(p) == "edges" && c.nodes[p].parent.is_some_and(|v| c.tag(v) == "vertex")
            });
            let vocabulary = if modern || xml.attribute("vertexkey").is_some() {
                Vocabulary::Modern
            } else {
                Vocabulary::Legacy
            };
            let (from, to) = if vocabulary == Vocabulary::Modern {
                let vertex = parent.and_then(|p| c.nodes[p].parent);
                (
                    vertex.and_then(|v| c.token(v, "vertexkey", true)),
                    c.token(i, "vertexkey", true),
                )
            } else {
                (c.token(i, "from", true), c.token(i, "to", true))
            };
            let annotation = c.token(i, "edgekey", true);
            let e = c.edges.len();
            if let Some(from) = from {
                c.outgoing.entry(from).or_default().push(e);
            }
            if let Some(to) = to {
                c.incoming.entry(to).or_default().push(e);
            }
            c.edges.push(EdgeClaim {
                from,
                to,
                annotation,
                node: i,
                vocabulary,
                origin: xml.range(),
            });
            c.edge_at.insert(i, e);
        }
    }
    if !c.complete {
        return CsvDeclarationAssessment::Refused { partial: c };
    }
    // Duplicate ownership includes declarations hidden in otherwise unknown XML.
    for nodes in c
        .components
        .values()
        .filter(|v| v.len() > 1)
        .cloned()
        .collect::<Vec<_>>()
    {
        let index = c.at_index(FindingKind::DuplicateComponent, nodes[0]);
        if let Some(f) = c.findings.get_mut(index) {
            f.related = nodes.iter().map(|&n| c.nodes[n].xml.range()).collect();
        }
    }
    for ports in c
        .ports_by_key
        .values()
        .filter(|v| v.len() > 1)
        .cloned()
        .collect::<Vec<_>>()
    {
        let index = c.at_index(FindingKind::DuplicatePort, c.ports[ports[0]].node);
        if let Some(f) = c.findings.get_mut(index) {
            f.related = ports.iter().map(|&p| c.ports[p].origin.clone()).collect();
        }
    }
    c.family = probe(&mut c);
    let csv_count = c
        .nodes
        .iter()
        .filter(|n| n.xml.has_tag_name("text") && n.xml.attribute("type") == Some("csv"))
        .count();
    if csv_count < 2 {
        return CsvDeclarationAssessment::NotApplicable {
            reason: ApplicabilityReason::NoInlineCsvPair,
        };
    }
    let root = *ids
        .get(&mapping.id().get())
        .expect("the validated root was censused");
    validate_envelope(&mut c, root);
    validate_edges(&mut c);
    for i in 0..c.nodes.len() {
        if c.nodes[i].classification == Classification::Unknown {
            c.at(FindingKind::UnknownXml, i);
        }
        for a in 0..c.nodes[i].attributes.len() {
            if c.nodes[i].attributes[a].classification == Classification::Unknown {
                let origin = c.nodes[i].attributes[a].origin.clone();
                let index = c.at_index(FindingKind::UnknownControl, i);
                if let Some(finding) = c.findings.get_mut(index) {
                    finding.origin = origin;
                }
            }
        }
    }
    c.findings.sort_by_key(|f| (f.origin.start, f.origin.end));
    if !c.findings.is_empty() || !c.complete {
        return CsvDeclarationAssessment::Refused { partial: c };
    }
    let mut obligations = obligations(&c).into_iter();
    let Some(first) = obligations.next() else {
        c.at(FindingKind::AmbiguousEndpoint, root);
        return CsvDeclarationAssessment::Refused { partial: c };
    };
    let pending = PendingObligations::new(first, obligations);
    CsvDeclarationAssessment::DeclarationOnly {
        proof: RawCsvDeclarationProof { census: c },
        pending,
    }
}

fn probe(c: &mut RawCensus<'_, '_>) -> Family {
    let mut csv = BTreeSet::new();
    let mut source = false;
    let mut target_rows = BTreeMap::<usize, BTreeSet<u32>>::new();
    let mut filenames = BTreeMap::<usize, BTreeSet<u32>>::new();
    let mut group_outputs = BTreeSet::new();
    let mut computed_outputs = BTreeSet::new();
    for n in &c.nodes {
        if n.xml.has_tag_name("text")
            && n.xml.attribute("type") == Some("csv")
            && let Some(owner) = n.owner
        {
            csv.insert(owner);
        }
    }
    for p in &c.ports {
        c.probe_port_visits += 1;
        if p.owner.is_some_and(|o| {
            c.attr(o, "library") == Some("core") && c.attr(o, "name") == Some("group-by")
        }) && p.direction == Direction::Output
            && c.attr(p.node, "pos") == Some("0")
        {
            group_outputs.insert(p.key);
        }
        if p.direction == Direction::Output
            && p.owner
                .is_some_and(|o| matches!(c.attr(o, "kind"), Some("4" | "5")))
        {
            computed_outputs.insert(p.key);
        }
        let Some(owner) = p.owner.filter(|o| csv.contains(o)) else {
            continue;
        };
        let n = &c.nodes[p.node];
        if n.xml.attribute("name") == Some("FileInstance") && p.direction == Direction::Input {
            filenames.entry(owner).or_default().insert(p.key);
        }
        if n.parent
            .is_some_and(|parent| c.attr(parent, "name") == Some("document"))
        {
            if p.direction == Direction::Output {
                source = true;
            }
            if p.direction == Direction::Input {
                target_rows.entry(owner).or_default().insert(p.key);
            }
        }
    }
    if !source {
        return Family::NotClaimed;
    }
    // Join marker sets once. Even ambiguous duplicate keys cannot trigger a
    // per-receiver walk over every alternative producer declaration/edge.
    let mut grouped_receivers = BTreeSet::new();
    let mut computed_receivers = BTreeSet::new();
    for edge in &c.edges {
        c.probe_edge_visits += 1;
        if let (Some(from), Some(to)) = (edge.from, edge.to) {
            if group_outputs.contains(&from) {
                grouped_receivers.insert(to);
            }
            if computed_outputs.contains(&from) {
                computed_receivers.insert(to);
            }
        }
    }
    for (owner, rows) in target_rows {
        let group = rows.iter().any(|row| grouped_receivers.contains(row));
        let computed = filenames
            .get(&owner)
            .is_some_and(|ports| ports.iter().any(|port| computed_receivers.contains(port)));
        if group && computed {
            return Family::GroupedCsvComposition;
        }
    }
    Family::NotClaimed
}

fn validate_envelope(c: &mut RawCensus<'_, '_>, root: usize) {
    c.shape(root, Classification::Syntax, &["version"]);
    c.exact(root, "version", "26", FindingKind::UnsupportedEnvelope);
    if c.tag(root) != "mapping" {
        c.at(FindingKind::UnsupportedEnvelope, root);
    }
    if let Some(resources) = c.one(root, "resources", false) {
        c.leaf(resources, Classification::Declaration, &[]);
    }
    let Some(wrapper) = c.one(root, "component", true) else {
        return;
    };
    c.shape(
        wrapper,
        Classification::Identity,
        &["name", "uid", "editable", "blackbox"],
    );
    c.attribute_class(wrapper, "editable", Classification::Layout);
    c.exact(wrapper, "editable", "1", FindingKind::UnsupportedEnvelope);
    c.exact(wrapper, "blackbox", "0", FindingKind::UnsupportedEnvelope);
    if let Some(properties) = c.one(wrapper, "properties", true) {
        c.leaf(
            properties,
            Classification::SavedGeneration,
            &[
                "AppName",
                "JavaBasePackage",
                "SelectedLanguage",
                "MakePathsAbsoluteOnGeneration",
                "UseWindowsPathConvention",
                "ShowLibraryNameInHeader",
            ],
        );
        c.attribute_class(
            properties,
            "ShowLibraryNameInHeader",
            Classification::Layout,
        );
        for (name, value) in [
            ("SelectedLanguage", "builtin"),
            ("MakePathsAbsoluteOnGeneration", "1"),
            ("UseWindowsPathConvention", "0"),
        ] {
            if c.attr(properties, name).is_some() {
                c.exact(properties, name, value, FindingKind::UnsupportedEnvelope);
            }
        }
        if c.attr(properties, "ShowLibraryNameInHeader")
            .is_some_and(|v| !matches!(v, "0" | "1"))
        {
            c.at(FindingKind::UnknownControl, properties);
        }
    }
    let Some(structure) = c.one(wrapper, "structure", true) else {
        return;
    };
    c.shape(structure, Classification::Declaration, &[]);
    let Some(children) = c.one(structure, "children", true) else {
        return;
    };
    c.shape(children, Classification::Declaration, &[]);
    for component in c.nodes[children].children.clone() {
        if c.tag(component) != "component" {
            continue;
        }
        validate_component(c, component);
    }
    let graph = c.one(structure, "graph", false);
    let local = c.one(structure, "connections", false);
    let parent = c.one(wrapper, "connections", false);
    let containers = [graph, local, parent]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if containers.len() != 1 {
        c.at(FindingKind::DuplicateContainer, structure);
    }
    if let Some(graph) = graph {
        validate_modern(c, graph);
    }
    for legacy in [local, parent].into_iter().flatten() {
        c.shape(legacy, Classification::Declaration, &[]);
        for e in c.nodes[legacy].children.clone() {
            if c.tag(e) == "edge" {
                c.leaf(e, Classification::Declaration, &["from", "to"]);
            }
        }
    }
    if c.endpoints.len() != 2
        || c.endpoints
            .iter()
            .filter(|e| e.role == EndpointRole::Source)
            .count()
            != 1
        || c.endpoints
            .iter()
            .filter(|e| e.role == EndpointRole::Target)
            .count()
            != 1
    {
        c.at(FindingKind::AmbiguousEndpoint, children);
    }
    let mut names = BTreeMap::new();
    let duplicates = c
        .parameters
        .iter()
        .filter_map(|p| {
            names
                .insert(p.name.decoded.clone(), p.owner)
                .map(|old| (old, p.owner))
        })
        .collect::<Vec<_>>();
    for (old, new) in duplicates {
        let index = c.at_index(FindingKind::InvalidParameter, new);
        if let Some(f) = c.findings.get_mut(index) {
            f.related.push(c.nodes[old].xml.range());
        }
    }
}

fn validate_component(c: &mut RawCensus<'_, '_>, i: usize) {
    c.shape(
        i,
        Classification::Identity,
        &[
            "name",
            "library",
            "uid",
            "kind",
            "growable",
            "growablebasename",
        ],
    );
    c.component_layout(i);
    let library = c.attr(i, "library").unwrap_or("").to_owned();
    let name = c.attr(i, "name").unwrap_or("").to_owned();
    let kind = c.token(i, "kind", true);
    if name.is_empty() || library.is_empty() {
        c.at(FindingKind::InvalidFunctionSignature, i);
    }
    if library == "text" && kind == Some(16) {
        validate_endpoint(c, i);
        return;
    }
    c.empty_properties(i);
    let inputs = c.slots(i, "sources");
    let outputs = c.slots(i, "targets");
    if inputs.len() + outputs.len() > MAX_SLOTS {
        c.stop(
            "component slots",
            inputs.len() + outputs.len(),
            MAX_SLOTS,
            c.nodes[i].xml.range().start,
        );
    }
    let single = keyed(&outputs, 1);
    if library == "core" && kind == Some(2) {
        if c.one(i, "sources", false).is_some() || !inputs.is_empty() || !single {
            c.at(FindingKind::InvalidLiteral, i);
        }
        let Some(data) = c.one(i, "data", true) else {
            return;
        };
        c.shape(data, Classification::Declaration, &[]);
        if let Some(wsdl) = c.one(data, "wsdl", false) {
            c.leaf(wsdl, Classification::Declaration, &[]);
        }
        if let Some(literal) = c.one(data, "constant", true) {
            c.leaf(literal, Classification::Declaration, &["datatype", "value"]);
            c.exact(literal, "datatype", "string", FindingKind::InvalidLiteral);
            if let Some(value) = c.value(literal, "value") {
                let datatype = c.value(literal, "datatype");
                c.literals.push(Literal {
                    owner: i,
                    output: outputs.first().and_then(|s| s.key),
                    value,
                    datatype,
                    origin: c.nodes[literal].xml.range(),
                });
            } else {
                c.at(FindingKind::InvalidLiteral, literal);
            }
        }
        if c.attr(i, "growable").is_some() || c.attr(i, "growablebasename").is_some() {
            c.at(FindingKind::InvalidLiteral, i);
        }
        return;
    }
    if library == "core" && kind == Some(6) {
        if !single || !(inputs.is_empty() || inputs.len() == 1 && empty(&inputs[0])) {
            c.at(FindingKind::InvalidParameter, i);
        }
        let Some(data) = c.one(i, "data", true) else {
            return;
        };
        c.shape(data, Classification::Declaration, &[]);
        let (Some(input), Some(parameter)) =
            (c.one(data, "input", true), c.one(data, "parameter", true))
        else {
            return;
        };
        c.leaf(
            input,
            Classification::Declaration,
            &["datatype", "required", "previewvalue", "usepreviewvalue"],
        );
        c.leaf(
            parameter,
            Classification::Declaration,
            &["usageKind", "name", "optional"],
        );
        c.exact(input, "datatype", "string", FindingKind::InvalidParameter);
        c.exact(
            parameter,
            "usageKind",
            "input",
            FindingKind::InvalidParameter,
        );
        let required = c.flag(input, "required", Some(Flag::One));
        let optional = c.flag(parameter, "optional", Some(Flag::Zero));
        let use_preview = c.flag(input, "usepreviewvalue", None);
        if let Some(name) = c.value(parameter, "name").filter(|n| !n.decoded.is_empty()) {
            let preview = c.value(input, "previewvalue");
            let datatype = c.value(input, "datatype");
            let usage = c.value(parameter, "usageKind");
            c.parameters.push(Parameter {
                owner: i,
                output: outputs.first().and_then(|s| s.key),
                name,
                datatype,
                usage,
                required,
                optional,
                preview,
                use_preview,
                source_slots: inputs,
            });
        } else {
            c.at(FindingKind::InvalidParameter, parameter);
        }
        if c.attr(i, "growable").is_some() || c.attr(i, "growablebasename").is_some() {
            c.at(FindingKind::InvalidParameter, i);
        }
        return;
    }
    let call = match (library.as_str(), name.as_str(), kind) {
        ("core", "group-by", Some(5)) => Some(CallKind::GroupBy),
        ("core", "sum", Some(5)) => Some(CallKind::Sum),
        ("core", "auto-number", Some(5)) => Some(CallKind::AutoNumber),
        ("lang", "now", Some(5)) => Some(CallKind::Now),
        ("lang", "empty", Some(5)) => Some(CallKind::Empty),
        ("lang", "right-trim", Some(5)) => Some(CallKind::RightTrim),
        ("core", "substring-before", Some(5)) => Some(CallKind::SubstringBefore),
        ("core", "if-else", Some(4)) => Some(CallKind::IfElse),
        ("db", "substitute-null", Some(5)) => Some(CallKind::SubstituteNull),
        ("core", "concat", Some(5)) => Some(CallKind::Concat),
        ("core", "format-dateTime", Some(5)) => Some(CallKind::FormatDateTime),
        _ => None,
    };
    let Some(call) = call else {
        c.at(FindingKind::InvalidFunctionSignature, i);
        return;
    };
    let valid_inputs = match call {
        CallKind::GroupBy | CallKind::SubstringBefore | CallKind::SubstituteNull => {
            keyed(&inputs, 2)
        }
        CallKind::Sum => {
            inputs.len() == 2
                && empty(&inputs[0])
                && inputs[1].position == Some(1)
                && inputs[1].key.is_some()
        }
        CallKind::AutoNumber => inputs.len() == 4 && inputs.iter().all(empty),
        CallKind::Now => inputs.is_empty(),
        CallKind::Empty | CallKind::RightTrim => keyed(&inputs, 1),
        CallKind::IfElse => keyed(&inputs, 3),
        CallKind::Concat => !inputs.is_empty() && keyed(&inputs, inputs.len()),
        CallKind::FormatDateTime => {
            inputs.len() == 3 && keyed(&inputs[..2], 2) && empty(&inputs[2])
        }
    };
    let valid_outputs = if call == CallKind::GroupBy {
        outputs.len() == 2
            && outputs.iter().any(|s| s.key.is_some())
            && outputs
                .iter()
                .all(|s| empty(s) || s.key.is_some() && s.position.is_some_and(|p| p < 2))
    } else {
        single
    };
    if !valid_inputs || !valid_outputs {
        c.at(FindingKind::InvalidFunctionSignature, i);
    }
    if call == CallKind::Concat {
        c.exact(i, "growable", "1", FindingKind::InvalidFunctionSignature);
        c.exact(
            i,
            "growablebasename",
            "value",
            FindingKind::InvalidFunctionSignature,
        );
    } else if c.attr(i, "growable").is_some() || c.attr(i, "growablebasename").is_some() {
        c.at(FindingKind::InvalidFunctionSignature, i);
    }
    if c.one(i, "data", false).is_some() {
        c.at(FindingKind::InvalidFunctionSignature, i);
    }
    c.functions.push(Callable {
        owner: i,
        kind: call,
        inputs,
        outputs,
    });
}

fn empty(slot: &Slot) -> bool {
    slot.key.is_none() && slot.position.is_none()
}
fn keyed(slots: &[Slot], count: usize) -> bool {
    slots.len() == count
        && slots.iter().all(|s| s.key.is_some())
        && slots
            .iter()
            .filter_map(|s| s.position)
            .collect::<BTreeSet<_>>()
            == (0..count as u32).collect()
}

fn validate_endpoint(c: &mut RawCensus<'_, '_>, i: usize) {
    if c.attr(i, "growable").is_some() || c.attr(i, "growablebasename").is_some() {
        c.at(FindingKind::UnknownControl, i);
    }
    let properties = c.one(i, "properties", true);
    if let Some(p) = properties {
        c.leaf(
            p,
            Classification::Declaration,
            &["DynamicFilenames", "XSLTDefaultOutput"],
        );
        c.exact(p, "DynamicFilenames", "1", FindingKind::UnknownControl);
    }
    let Some(data) = c.one(i, "data", true) else {
        return;
    };
    c.shape(data, Classification::Declaration, &[]);
    let (Some(root), Some(text)) = (c.one(data, "root", true), c.one(data, "text", true)) else {
        return;
    };
    c.shape(root, Classification::Declaration, &["scrollposition"]);
    c.attribute_class(root, "scrollposition", Classification::Layout);
    if c.attr(root, "scrollposition").is_some() {
        c.token(root, "scrollposition", false);
    }
    let Some(header) = c.one(root, "header", true) else {
        return;
    };
    c.shape(header, Classification::Identity, &[]);
    let Some(namespaces) = c.one(header, "namespaces", true) else {
        return;
    };
    c.shape(namespaces, Classification::Identity, &[]);
    let ns = c.nodes[namespaces].children.clone();
    let mut table = Vec::new();
    if ns.len() != 2 {
        c.at(FindingKind::NamespaceMismatch, namespaces);
    }
    for (ordinal, n) in ns.into_iter().enumerate() {
        if c.tag(n) != "namespace" {
            c.at(FindingKind::NamespaceMismatch, n);
            continue;
        }
        c.leaf(n, Classification::Identity, &["uid"]);
        let value = c.value(n, "uid");
        if ordinal == 0 && value.is_some()
            || ordinal == 1 && value.as_ref().is_none_or(|v| v.decoded != PROTOCOL)
        {
            c.at(FindingKind::NamespaceMismatch, n);
        }
        table.push(value);
    }
    let Some(wrapper) = c.one(root, "entry", true) else {
        return;
    };
    c.shape(
        wrapper,
        Classification::Identity,
        &["name", "ns", "inpkey", "expanded"],
    );
    c.exact(
        wrapper,
        "name",
        "FileInstance",
        FindingKind::MalformedWrapper,
    );
    c.exact(wrapper, "ns", "1", FindingKind::NamespaceMismatch);
    let filename = c.port(wrapper, "inpkey", PortRole::Filename);
    if filename.is_none() {
        c.at(FindingKind::MalformedWrapper, wrapper);
    }
    let Some(document) = c.one(wrapper, "entry", true) else {
        return;
    };
    c.shape(
        document,
        Classification::Identity,
        &["name", "ns", "expanded"],
    );
    c.exact(document, "name", "document", FindingKind::MalformedWrapper);
    c.exact(document, "ns", "1", FindingKind::NamespaceMismatch);
    let Some(row) = c.one(document, "entry", true) else {
        return;
    };
    c.shape(
        row,
        Classification::Identity,
        &["name", "inpkey", "outkey", "expanded"],
    );
    let role = match (
        c.attr(row, "inpkey").is_some(),
        c.attr(row, "outkey").is_some(),
    ) {
        (true, false) => EndpointRole::Target,
        (false, true) => EndpointRole::Source,
        _ => {
            c.at(FindingKind::WrongPortDirection, row);
            return;
        }
    };
    let key_attr = if role == EndpointRole::Source {
        "outkey"
    } else {
        "inpkey"
    };
    let row_key = c.port(row, key_attr, PortRole::Row);
    for n in [wrapper, document, row] {
        c.attribute_class(n, "expanded", Classification::Layout);
        if c.attr(n, "expanded")
            .is_some_and(|v| !matches!(v, "0" | "1"))
        {
            c.at(FindingKind::UnknownControl, n);
        }
    }
    if let Some(p) = properties {
        if role == EndpointRole::Target {
            c.exact(p, "XSLTDefaultOutput", "1", FindingKind::UnknownControl);
        } else if c.attr(p, "XSLTDefaultOutput").is_some() {
            c.at(FindingKind::UnknownControl, p);
        }
    }
    let Some(transport) = read_transport(c, text, role) else {
        return;
    };
    let Some(settings) = c.one(text, "settings", true) else {
        return;
    };
    let Some(names) = c.one(settings, "names", true) else {
        return;
    };
    c.shape(names, Classification::Declaration, &["root", "block"]);
    let (Some(inline_root), Some(inline_block)) = (c.value(names, "root"), c.value(names, "block"))
    else {
        c.at(FindingKind::InvalidColumnOrder, names);
        return;
    };
    if inline_root.decoded.is_empty()
        || inline_block.decoded.is_empty()
        || c.attr(i, "name") != Some(inline_root.decoded.as_str())
        || c.attr(row, "name") != Some(inline_block.decoded.as_str())
    {
        c.at(FindingKind::MalformedWrapper, names);
    }
    let declarations = c.nodes[names].children.clone();
    if declarations.is_empty() || declarations.len() > MAX_COLUMNS {
        if declarations.len() > MAX_COLUMNS {
            c.stop(
                "columns",
                declarations.len(),
                MAX_COLUMNS,
                c.nodes[names].xml.range().start,
            );
        } else {
            c.at(FindingKind::InvalidColumnOrder, names);
        }
        return;
    }
    let mut ordered = BTreeMap::new();
    let mut field_names = BTreeSet::new();
    for (ordinal, field) in declarations.into_iter().enumerate() {
        c.leaf(field, Classification::Declaration, &["name", "type"]);
        let index = c
            .tag(field)
            .strip_prefix("field")
            .and_then(|s| canonical(s, false))
            .map(|v| v as usize);
        let Some(index) = index.filter(|&v| v < MAX_COLUMNS) else {
            c.at(FindingKind::InvalidColumnOrder, field);
            continue;
        };
        if ordered.insert(index, (ordinal, field)).is_some() {
            c.at(FindingKind::DuplicateColumn, field);
        }
        match c.attr(field, "name") {
            Some(name) if !name.is_empty() => {
                if !field_names.insert(name.to_owned()) {
                    c.at(FindingKind::DuplicateColumn, field);
                }
            }
            _ => c.at(FindingKind::InvalidColumnOrder, field),
        }
        c.exact(field, "type", "string", FindingKind::UnsupportedColumnType);
    }
    let leaves = c.nodes[row].children.clone();
    if ordered.len() != leaves.len() || ordered.keys().copied().ne(0..ordered.len()) {
        c.at(FindingKind::InvalidColumnOrder, names);
    }
    let mut columns = Vec::new();
    for (index, (ordinal, field)) in ordered {
        let Some(&leaf) = leaves.get(index) else {
            continue;
        };
        if c.tag(leaf) != "entry" {
            c.at(FindingKind::MalformedWrapper, leaf);
            continue;
        }
        c.leaf(
            leaf,
            Classification::Declaration,
            &["name", key_attr, "expanded"],
        );
        c.attribute_class(leaf, "expanded", Classification::Layout);
        if c.attr(leaf, "expanded")
            .is_some_and(|v| !matches!(v, "0" | "1"))
        {
            c.at(FindingKind::UnknownControl, leaf);
        }
        if c.attr(field, "name") != c.attr(leaf, "name") {
            c.at(FindingKind::InvalidColumnOrder, leaf);
        }
        let port = c.port(leaf, key_attr, PortRole::Column);
        if port.is_none() {
            c.at(FindingKind::WrongPortDirection, leaf);
        }
        if let (Some(name), Some(declared_type)) = (c.value(field, "name"), c.value(field, "type"))
        {
            columns.push(Column {
                index,
                xml_ordinal: ordinal,
                name,
                declared_type,
                port,
                declaration_origin: c.nodes[field].xml.range(),
                leaf_origin: c.nodes[leaf].xml.range(),
            });
        }
    }
    c.endpoints.push(Endpoint {
        owner: i,
        role,
        filename,
        row: row_key,
        root,
        wrapper,
        document,
        row_node: row,
        namespace_table: table,
        inline_root,
        inline_block,
        columns,
        transport,
    });
}

fn read_transport<'doc, 'text>(
    c: &mut RawCensus<'doc, 'text>,
    text: usize,
    role: EndpointRole,
) -> Option<Transport<'text>> {
    c.shape(
        text,
        Classification::Declaration,
        &[
            "type",
            "encoding",
            "byteorder",
            "byteordermark",
            "inputinstance",
        ],
    );
    for (name, value) in [
        ("type", "csv"),
        ("encoding", "1000"),
        ("byteorder", "1"),
        ("byteordermark", "0"),
    ] {
        c.exact(text, name, value, FindingKind::ConflictingSetting);
    }
    let encoding = c.token(text, "encoding", true);
    let byteorder = c.token(text, "byteorder", false);
    let bom = match c.attr(text, "byteordermark") {
        None => Flag::Missing,
        Some("") => Flag::Empty,
        Some("0") => Flag::Zero,
        Some("1") => Flag::One,
        _ => Flag::Invalid,
    };
    if role == EndpointRole::Target && c.attr(text, "inputinstance").is_some() {
        c.at(FindingKind::UnknownControl, text);
    }
    let settings = c.one(text, "settings", true)?;
    c.shape(
        settings,
        Classification::Declaration,
        &["separator", "quote", "firstrownames", "removeempty"],
    );
    let one_char = |s: &str| {
        let mut chars = s.chars();
        let first = chars.next()?;
        chars.next().is_none().then_some(first)
    };
    let separator = c
        .attr(settings, "separator")
        .and_then(one_char)
        .filter(|v| !matches!(v, '\r' | '\n'));
    let quote = match c.attr(settings, "quote") {
        Some("") => Some(None),
        Some(s) => one_char(s).filter(|v| !matches!(v, '\r' | '\n')).map(Some),
        None => None,
    };
    let boolean = |s: Option<&str>| match s {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    };
    let (Some(separator), Some(quote), Some(header), Some(remove_empty)) = (
        separator,
        quote,
        boolean(c.attr(settings, "firstrownames")),
        boolean(c.attr(settings, "removeempty")),
    ) else {
        c.at(FindingKind::ConflictingSetting, settings);
        return None;
    };
    if quote == Some(separator) {
        c.at(FindingKind::ConflictingSetting, settings);
    }
    let mut controls = BTreeMap::new();
    for n in [text, settings] {
        for a in &c.nodes[n].attributes {
            controls.insert(a.name.clone(), a.value.clone());
        }
    }
    let record_terminator = if controls.keys().any(|key| {
        matches!(
            key.as_str(),
            "recordterminator" | "lineterminator" | "newline" | "eol"
        )
    }) {
        RecordTerminator::ExplicitUnqualified
    } else {
        RecordTerminator::NotDeclared
    };
    Some(Transport {
        controls,
        encoding,
        byteorder,
        bom,
        separator,
        quote,
        header,
        remove_empty,
        record_terminator,
        preview_instance: c.value(text, "inputinstance"),
    })
}

fn validate_modern(c: &mut RawCensus<'_, '_>, graph: usize) {
    c.shape(graph, Classification::Identity, &["directed"]);
    c.exact(graph, "directed", "1", FindingKind::UnknownControl);
    let mut annotations = BTreeMap::<u32, Vec<usize>>::new();
    if let Some(edges) = c.one(graph, "edges", false) {
        c.shape(edges, Classification::Declaration, &[]);
        for edge in c.nodes[edges].children.clone() {
            if c.tag(edge) != "edge" {
                continue;
            }
            c.shape(edge, Classification::Declaration, &["edgekey"]);
            if let Some(key) = c.token(edge, "edgekey", true) {
                annotations.entry(key).or_default().push(edge);
            } else {
                c.at(FindingKind::UnmatchedAnnotation, edge);
            }
            if let Some(data) = c.one(edge, "data", true) {
                c.shape(data, Classification::Descriptive, &[]);
                if let Some(connection) = c.one(data, "dataconnection", true) {
                    c.leaf(
                        connection,
                        Classification::Descriptive,
                        &["name", "starting_location"],
                    );
                    if c.attr(connection, "name").is_none() {
                        c.at(FindingKind::UnmatchedAnnotation, connection);
                    }
                    if c.attr(connection, "starting_location").is_some() {
                        c.exact(
                            connection,
                            "starting_location",
                            "0",
                            FindingKind::UnknownControl,
                        );
                        c.nodes[connection].classification = Classification::UnqualifiedAnnotation;
                        c.attribute_class(
                            connection,
                            "starting_location",
                            Classification::UnqualifiedAnnotation,
                        );
                    }
                }
            }
        }
    }
    if let Some(vertices) = c.one(graph, "vertices", true) {
        c.shape(vertices, Classification::Identity, &[]);
        let mut producers = BTreeMap::<u32, Vec<usize>>::new();
        for vertex in c.nodes[vertices].children.clone() {
            if c.tag(vertex) != "vertex" {
                continue;
            }
            c.shape(vertex, Classification::Identity, &["vertexkey"]);
            if let Some(key) = c.token(vertex, "vertexkey", true) {
                producers.entry(key).or_default().push(vertex);
                if !c
                    .ports_by_key
                    .get(&key)
                    .is_some_and(|p| p.len() == 1 && c.ports[p[0]].direction == Direction::Output)
                {
                    c.at(FindingKind::DanglingEdge, vertex);
                }
            } else {
                c.at(FindingKind::DanglingEdge, vertex);
            }
            if let Some(edges) = c.one(vertex, "edges", true) {
                c.shape(edges, Classification::Declaration, &[]);
                for edge in c.nodes[edges].children.clone() {
                    if c.tag(edge) == "edge" {
                        c.leaf(edge, Classification::Declaration, &["vertexkey", "edgekey"]);
                    }
                }
            }
        }
        for declarations in producers.values().filter(|nodes| nodes.len() > 1) {
            let index = c.at_index(FindingKind::DuplicateEdgeClaim, declarations[0]);
            if let Some(finding) = c.findings.get_mut(index) {
                finding.related = declarations
                    .iter()
                    .map(|&n| c.nodes[n].xml.range())
                    .collect();
            }
        }
    }
    let mut refs = BTreeMap::<u32, Vec<usize>>::new();
    for e in &c.edges {
        if let Some(key) = e.annotation {
            refs.entry(key).or_default().push(e.node);
        }
    }
    let keys = annotations
        .keys()
        .chain(refs.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    for key in keys {
        let definitions = annotations.get(&key).map(Vec::as_slice).unwrap_or(&[]);
        let references = refs.get(&key).map(Vec::as_slice).unwrap_or(&[]);
        if definitions.len() != 1 || references.len() != 1 {
            let all = definitions
                .iter()
                .chain(references)
                .copied()
                .collect::<Vec<_>>();
            let index = c.at_index(FindingKind::UnmatchedAnnotation, all[0]);
            if let Some(finding) = c.findings.get_mut(index) {
                finding.related = all.iter().map(|&n| c.nodes[n].xml.range()).collect();
            }
        }
    }
}

fn validate_edges(c: &mut RawCensus<'_, '_>) {
    if c.edges
        .iter()
        .map(|edge| edge.vocabulary)
        .collect::<BTreeSet<_>>()
        .len()
        > 1
    {
        c.at(FindingKind::DuplicateContainer, c.edges[0].node);
    }
    let mut pairs = BTreeMap::<(u32, u32), Vec<usize>>::new();
    let mut adjacency = BTreeMap::<usize, Vec<usize>>::new();
    let mut degrees = c
        .component_uids
        .keys()
        .map(|&o| (o, 0usize))
        .collect::<BTreeMap<_, _>>();
    for e in 0..c.edges.len() {
        c.edge_visits += 1;
        let (from, to, node) = {
            let edge = &c.edges[e];
            (edge.from, edge.to, edge.node)
        };
        let (Some(from), Some(to)) = (from, to) else {
            c.at(FindingKind::DanglingEdge, node);
            continue;
        };
        pairs.entry((from, to)).or_default().push(e);
        let owners = |key| {
            c.ports_by_key
                .get(&key)
                .filter(|v| v.len() == 1)
                .map(|v| v[0])
        };
        let (Some(producer), Some(receiver)) = (owners(from), owners(to)) else {
            c.at(FindingKind::DanglingEdge, node);
            continue;
        };
        let p = &c.ports[producer];
        let r = &c.ports[receiver];
        let (po, ro) = (p.owner, r.owner);
        if p.direction != Direction::Output || r.direction != Direction::Input {
            c.at(FindingKind::WrongPortDirection, node);
        }
        if let (Some(po), Some(ro)) = (po, ro) {
            adjacency.entry(po).or_default().push(ro);
            *degrees.entry(ro).or_default() += 1;
        } else {
            c.at(FindingKind::DanglingEdge, node);
        }
    }
    for claims in pairs.values().filter(|claims| claims.len() > 1) {
        let index = c.at_index(FindingKind::DuplicateEdgeClaim, c.edges[claims[0]].node);
        if let Some(finding) = c.findings.get_mut(index) {
            finding.related = claims.iter().map(|&e| c.edges[e].origin.clone()).collect();
        }
    }
    let conflicts = c
        .incoming
        .values()
        .filter(|claims| claims.len() > 1)
        .cloned()
        .collect::<Vec<_>>();
    for claims in conflicts {
        let distinct = claims
            .iter()
            .filter_map(|&e| c.edges[e].from)
            .collect::<BTreeSet<_>>();
        if distinct.len() > 1 {
            let index = c.at_index(FindingKind::ConflictingFeed, c.edges[claims[0]].node);
            if let Some(f) = c.findings.get_mut(index) {
                f.related = claims.iter().map(|&e| c.edges[e].origin.clone()).collect();
            }
        }
    }
    for p in 0..c.ports.len() {
        if c.ports[p].direction == Direction::Input && !c.incoming.contains_key(&c.ports[p].key) {
            c.at(FindingKind::DanglingEdge, c.ports[p].node);
        }
        if c.ports[p].role == PortRole::Unclassified {
            c.at(FindingKind::UnknownControl, c.ports[p].node);
        }
    }
    let mut ready = degrees
        .iter()
        .filter_map(|(&o, &d)| (d == 0).then_some(o))
        .collect::<VecDeque<_>>();
    let mut visited = 0;
    while let Some(owner) = ready.pop_front() {
        visited += 1;
        for &receiver in adjacency.get(&owner).into_iter().flatten() {
            if let Some(degree) = degrees.get_mut(&receiver) {
                *degree -= 1;
                if *degree == 0 {
                    ready.push_back(receiver);
                }
            }
        }
    }
    if visited != degrees.len()
        && let Some((&owner, _)) = degrees.iter().find(|(_, d)| **d != 0)
    {
        c.at(FindingKind::GraphCycle, owner);
    }
}

fn obligations(c: &RawCensus<'_, '_>) -> Vec<SemanticObligation> {
    let mut pending = Vec::new();
    let mut add = |kind, owner, ports: Vec<u32>, origins: Vec<Range<usize>>| {
        let edge_claims = ports
            .iter()
            .flat_map(|key| {
                c.incoming
                    .get(key)
                    .into_iter()
                    .flatten()
                    .chain(c.outgoing.get(key).into_iter().flatten())
            })
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        pending.push(SemanticObligation {
            kind,
            owner,
            ports,
            edge_claims,
            origins,
        });
    };
    for endpoint in &c.endpoints {
        let ports = endpoint
            .filename
            .into_iter()
            .chain(endpoint.row)
            .chain(endpoint.columns.iter().filter_map(|f| f.port))
            .collect::<Vec<_>>();
        add(
            if endpoint.role == EndpointRole::Source {
                ObligationKind::InputPhysicalSemantics
            } else {
                ObligationKind::OutputPhysicalSemantics
            },
            endpoint.owner,
            ports.clone(),
            vec![c.nodes[endpoint.root].xml.range()],
        );
        if endpoint.transport.record_terminator == RecordTerminator::NotDeclared {
            add(
                ObligationKind::RecordTerminatorNotDeclared,
                endpoint.owner,
                ports.clone(),
                vec![c.nodes[endpoint.root].xml.range()],
            );
        }
        add(
            ObligationKind::FilenameAcquisitionAndComposedOutput,
            endpoint.owner,
            ports.clone(),
            vec![c.nodes[endpoint.wrapper].xml.range()],
        );
        add(
            ObligationKind::GeneratedAndHostComposition,
            endpoint.owner,
            ports.clone(),
            vec![c.nodes[endpoint.document].xml.range()],
        );
        if endpoint.role == EndpointRole::Target && endpoint.transport.quote.is_none() {
            add(
                ObligationKind::DisabledQuotePhysicalBoundary,
                endpoint.owner,
                ports,
                vec![c.nodes[endpoint.row_node].xml.range()],
            );
        }
    }
    for function in &c.functions {
        let kind = match function.kind {
            CallKind::GroupBy => ObligationKind::GroupReceiverOwnership,
            CallKind::Sum => ObligationKind::AggregateFrameAndStringConversion,
            CallKind::AutoNumber => ObligationKind::NumberingScope,
            CallKind::Now | CallKind::FormatDateTime => ObligationKind::ClockAndDatePicture,
            _ => ObligationKind::ScalarPresenceAndEvaluation,
        };
        let ports = function
            .inputs
            .iter()
            .chain(&function.outputs)
            .filter_map(|s| s.key)
            .collect();
        add(
            kind,
            function.owner,
            ports,
            vec![c.nodes[function.owner].xml.range()],
        );
    }
    for (i, node) in c.nodes.iter().enumerate() {
        if node.classification == Classification::SavedGeneration
            && node.attributes.iter().any(|a| {
                matches!(
                    a.name.as_str(),
                    "MakePathsAbsoluteOnGeneration" | "UseWindowsPathConvention"
                )
            })
        {
            add(
                ObligationKind::SavedGenerationPathPolicy,
                node.owner.unwrap_or(i),
                Vec::new(),
                vec![node.xml.range()],
            );
        }
        if node.classification == Classification::UnqualifiedAnnotation {
            add(
                ObligationKind::UnqualifiedSavedAnnotationControl,
                i,
                Vec::new(),
                vec![node.xml.range()],
            );
        }
    }
    pending
}

#[cfg(test)]
#[path = "csv_declaration_proof_tests.rs"]
mod tests;
