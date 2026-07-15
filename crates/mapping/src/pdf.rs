use std::collections::HashSet;
use std::num::NonZeroU32;

use ir::{ScalarType, SchemaNode};
use serde::{Deserialize, Serialize};

pub const MAX_PDF_LAYOUT_DEPTH: usize = 64;
pub const MAX_PDF_LAYOUT_NODES: usize = 4_096;
pub const MAX_PDF_LAYOUT_STRING_BYTES: usize = 1_048_576;

/// A validated visual extraction layout for a PDF source.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PdfLayout {
    root_name: String,
    page_selection: PdfPageSelection,
    commands: Vec<PdfCommand>,
}

impl PdfLayout {
    pub fn new(
        root_name: impl Into<String>,
        page_selection: PdfPageSelection,
        commands: Vec<PdfCommand>,
    ) -> Result<Self, PdfLayoutError> {
        let root_name = root_name.into();
        validate_name(&root_name, "root")?;
        page_selection.validate()?;
        let mut state = ValidationState::new(root_name.len());
        let mut anchors = Vec::new();
        let schema = validate_commands(&commands, 1, true, &mut anchors, &mut state)?;
        if schema.is_empty() {
            return Err(PdfLayoutError::NoOutput);
        }
        Ok(Self {
            root_name,
            page_selection,
            commands,
        })
    }

    pub fn root_name(&self) -> &str {
        &self.root_name
    }

    pub const fn page_selection(&self) -> PdfPageSelection {
        self.page_selection
    }

    pub fn commands(&self) -> &[PdfCommand] {
        &self.commands
    }

    pub fn schema(&self) -> SchemaNode {
        SchemaNode::group(
            &self.root_name,
            self.commands
                .iter()
                .flat_map(PdfCommand::schema_nodes)
                .collect(),
        )
    }
}

impl<'de> Deserialize<'de> for PdfLayout {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Repr {
            root_name: String,
            page_selection: PdfPageSelection,
            commands: Vec<PdfCommand>,
        }

        let value = Repr::deserialize(deserializer)?;
        Self::new(value.root_name, value.page_selection, value.commands)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PdfPageSelection {
    All,
    First,
    From { first: NonZeroU32 },
    Range { first: NonZeroU32, last: NonZeroU32 },
}

impl PdfPageSelection {
    fn validate(self) -> Result<(), PdfLayoutError> {
        if let Self::Range { first, last } = self
            && first > last
        {
            return Err(PdfLayoutError::InvalidPageRange {
                first: first.get(),
                last: last.get(),
            });
        }
        Ok(())
    }

    pub fn includes(self, page: u32) -> bool {
        match self {
            Self::All => true,
            Self::First => page == 1,
            Self::From { first } => page >= first.get(),
            Self::Range { first, last } => (first.get()..=last.get()).contains(&page),
        }
    }

    fn is_single_page(self) -> bool {
        matches!(self, Self::First) || matches!(self, Self::Range { first, last } if first == last)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PdfCommand {
    Capture(PdfCapture),
    GroupPerPage(PdfGroup),
    EdgeRows(PdfEdgeRows),
    TextGroups(PdfTextGroups),
    TextRows(PdfTextRows),
    Pages(PdfPages),
    Merge(PdfMerge),
    Anchor(PdfAnchorAssignment),
    BoundaryFindVertical(PdfVerticalBoundaryFind),
}

impl PdfCommand {
    fn schema_nodes(&self) -> Vec<SchemaNode> {
        match self {
            Self::Capture(capture) => {
                vec![SchemaNode::scalar(&capture.name, ScalarType::String)]
            }
            Self::GroupPerPage(group) => {
                let mut node = SchemaNode::group(
                    &group.name,
                    group.children.iter().flat_map(Self::schema_nodes).collect(),
                );
                node.repeating = true;
                vec![node]
            }
            Self::EdgeRows(rows) => rows.children.iter().flat_map(Self::schema_nodes).collect(),
            Self::TextGroups(groups) => groups
                .groups
                .iter()
                .flat_map(PdfTextGroup::schema_nodes)
                .collect(),
            Self::TextRows(rows) => rows.children.iter().flat_map(Self::schema_nodes).collect(),
            Self::Pages(pages) => pages.children.iter().flat_map(Self::schema_nodes).collect(),
            Self::Merge(merge) => merge.children.iter().flat_map(Self::schema_nodes).collect(),
            Self::Anchor(_) | Self::BoundaryFindVertical(_) => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PdfCapture {
    pub name: String,
    pub region: PdfRegion,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PdfGroup {
    pub name: String,
    pub region: PdfRegion,
    pub children: Vec<PdfCommand>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PdfEdgeRows {
    pub region: PdfRegion,
    pub find: PdfEdgeFind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_extent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_anchor: Option<PdfRegion>,
    pub children: Vec<PdfCommand>,
}

/// Marker-delimited regions evaluated in their visual order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PdfTextGroups {
    pub region: PdfRegion,
    pub groups: Vec<PdfTextGroup>,
}

/// One marker and the output produced for each of its occurrences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PdfTextGroup {
    pub output: PdfTextGroupOutput,
    pub matcher: PdfTextMatch,
    pub children: Vec<PdfCommand>,
}

impl PdfTextGroup {
    fn schema_nodes(&self) -> Vec<SchemaNode> {
        let children = self
            .children
            .iter()
            .flat_map(PdfCommand::schema_nodes)
            .collect::<Vec<_>>();
        match &self.output {
            PdfTextGroupOutput::Flatten => children,
            PdfTextGroupOutput::Repeated { name } => {
                let mut node = SchemaNode::group(name, children);
                node.repeating = true;
                vec![node]
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PdfTextGroupOutput {
    Flatten,
    Repeated { name: String },
}

/// A bounded literal visual-text match.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PdfTextMatch {
    pub needle: String,
    pub case: PdfTextCase,
    pub flexible_whitespace: bool,
    #[serde(default, skip_serializing_if = "PdfTextProperties::is_empty")]
    pub properties: PdfTextProperties,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PdfTextProperties {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_face: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_height: Option<PdfMetricMatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_angle: Option<PdfMetricMatch>,
}

impl PdfTextProperties {
    pub fn is_empty(&self) -> bool {
        self.font_face.is_none() && self.cell_height.is_none() && self.baseline_angle.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PdfMetricMatch {
    pub value: f64,
    pub deviation: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PdfTextCase {
    Sensitive,
    AsciiInsensitive,
}

/// Non-empty visual text lines evaluated as independent candidate rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PdfTextRows {
    pub region: PdfRegion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_extent: Option<f64>,
    pub children: Vec<PdfCommand>,
}

/// A transparent command block restricted to selected physical pages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PdfPages {
    pub selection: PdfPageSelection,
    pub children: Vec<PdfCommand>,
}

/// A named, ordered set of physical page regions evaluated as one logical source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PdfMerge {
    pub name: String,
    #[serde(default)]
    pub composition: PdfMergeComposition,
    pub sources: Vec<PdfMergeSource>,
    pub children: Vec<PdfCommand>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PdfMergeComposition {
    #[default]
    Independent,
    VerticalCollage,
}

/// One physical page region contributing to a [`PdfMerge`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PdfMergeSource {
    pub page_selection: PdfPageSelection,
    pub region: PdfRegion,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PdfAnchorAssignment {
    pub name: String,
    pub axis: PdfAnchorAxis,
    pub at: PdfCoordinate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PdfVerticalBoundaryFind {
    pub region: PdfRegion,
    pub begin_anchor: String,
    pub end_anchor: String,
    pub find: PdfEdgeFind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PdfAnchorAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PdfEdgeFind {
    pub fill: f64,
    pub prominence: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PdfRegion {
    pub left: PdfCoordinate,
    pub top: PdfCoordinate,
    pub right: PdfCoordinate,
    pub bottom: PdfCoordinate,
}

impl PdfRegion {
    pub const fn full() -> Self {
        Self {
            left: PdfCoordinate::edge(PdfReference::Left),
            top: PdfCoordinate::edge(PdfReference::Top),
            right: PdfCoordinate::edge(PdfReference::Right),
            bottom: PdfCoordinate::edge(PdfReference::Bottom),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PdfCoordinate {
    pub reference: PdfReference,
    pub offset: f64,
}

impl PdfCoordinate {
    pub const fn new(reference: PdfReference, offset: f64) -> Self {
        Self { reference, offset }
    }

    pub const fn edge(reference: PdfReference) -> Self {
        Self::new(reference, 0.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "name", rename_all = "snake_case")]
pub enum PdfReference {
    Left,
    Top,
    Right,
    Bottom,
    Anchor(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfLayoutError {
    EmptyName(&'static str),
    InvalidName { role: &'static str, name: String },
    DuplicateOutput(String),
    DuplicateAnchor(String),
    UnknownAnchor(String),
    InvalidAnchorAxis(String),
    InvalidCoordinate,
    InvalidEdgeFind,
    InvalidMinimumExtent,
    EmptyTextGroups,
    EmptyTextNeedle,
    InvalidTextMetric,
    NonRepeatingRowOutput(String),
    NonRepeatingDocumentOutput { command: &'static str, name: String },
    EmptyMergeSources(String),
    NestedDocumentCommand(&'static str),
    InvalidPageRange { first: u32, last: u32 },
    NoOutput,
    TooDeep,
    TooManyNodes,
    TooManyStringBytes,
}

impl std::fmt::Display for PdfLayoutError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyName(role) => write!(formatter, "PDF {role} name must not be empty"),
            Self::InvalidName { role, name } => {
                write!(
                    formatter,
                    "PDF {role} name `{name}` contains a path separator"
                )
            }
            Self::DuplicateOutput(name) => write!(formatter, "duplicate PDF output `{name}`"),
            Self::DuplicateAnchor(name) => write!(formatter, "duplicate PDF anchor `{name}`"),
            Self::UnknownAnchor(name) => write!(formatter, "unknown PDF anchor `{name}`"),
            Self::InvalidAnchorAxis(name) => {
                write!(formatter, "PDF anchor `{name}` is used on the wrong axis")
            }
            Self::InvalidCoordinate => formatter.write_str("PDF coordinates must be finite"),
            Self::InvalidEdgeFind => formatter.write_str(
                "PDF edge-find fill must be positive and prominence nonnegative and finite",
            ),
            Self::InvalidMinimumExtent => {
                formatter.write_str("PDF minimum row extent must be positive and finite")
            }
            Self::EmptyTextGroups => {
                formatter.write_str("PDF text-groups command must define at least one matcher")
            }
            Self::EmptyTextNeedle => {
                formatter.write_str("PDF text matcher must not normalize to empty")
            }
            Self::InvalidTextMetric => formatter.write_str(
                "PDF text metric values must be finite and deviations must be nonnegative",
            ),
            Self::NonRepeatingRowOutput(name) => {
                write!(
                    formatter,
                    "PDF edge rows expose non-repeating output `{name}`"
                )
            }
            Self::NonRepeatingDocumentOutput { command, name } => {
                write!(
                    formatter,
                    "PDF {command} exposes non-repeating output `{name}` across multiple candidates"
                )
            }
            Self::EmptyMergeSources(name) => {
                write!(formatter, "PDF merge `{name}` has no source regions")
            }
            Self::NestedDocumentCommand(command) => {
                write!(
                    formatter,
                    "PDF document-level `{command}` command is nested"
                )
            }
            Self::InvalidPageRange { first, last } => {
                write!(formatter, "PDF page range {first}..={last} is reversed")
            }
            Self::NoOutput => formatter.write_str("PDF layout does not define any output"),
            Self::TooDeep => write!(formatter, "PDF layout exceeds depth {MAX_PDF_LAYOUT_DEPTH}"),
            Self::TooManyNodes => {
                write!(formatter, "PDF layout exceeds {MAX_PDF_LAYOUT_NODES} nodes")
            }
            Self::TooManyStringBytes => write!(
                formatter,
                "PDF layout strings exceed {MAX_PDF_LAYOUT_STRING_BYTES} bytes"
            ),
        }
    }
}

impl std::error::Error for PdfLayoutError {}

struct ValidationState {
    nodes: usize,
    string_bytes: usize,
}

impl ValidationState {
    const fn new(string_bytes: usize) -> Self {
        Self {
            nodes: 0,
            string_bytes,
        }
    }

    fn add_node(&mut self) -> Result<(), PdfLayoutError> {
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or(PdfLayoutError::TooManyNodes)?;
        if self.nodes > MAX_PDF_LAYOUT_NODES {
            return Err(PdfLayoutError::TooManyNodes);
        }
        Ok(())
    }

    fn add_string(&mut self, value: &str) -> Result<(), PdfLayoutError> {
        self.string_bytes = self
            .string_bytes
            .checked_add(value.len())
            .ok_or(PdfLayoutError::TooManyStringBytes)?;
        if self.string_bytes > MAX_PDF_LAYOUT_STRING_BYTES {
            return Err(PdfLayoutError::TooManyStringBytes);
        }
        Ok(())
    }
}

type Anchors = Vec<(String, PdfAnchorAxis)>;

fn validate_commands(
    commands: &[PdfCommand],
    depth: usize,
    allow_document_commands: bool,
    anchors: &mut Anchors,
    state: &mut ValidationState,
) -> Result<Vec<SchemaNode>, PdfLayoutError> {
    if depth > MAX_PDF_LAYOUT_DEPTH {
        return Err(PdfLayoutError::TooDeep);
    }
    let mut output_names = HashSet::new();
    let mut schema = Vec::new();
    for command in commands {
        state.add_node()?;
        match command {
            PdfCommand::Capture(capture) => {
                validate_name(&capture.name, "capture")?;
                state.add_string(&capture.name)?;
                validate_region(&capture.region, anchors, state)?;
            }
            PdfCommand::GroupPerPage(group) => {
                validate_name(&group.name, "group")?;
                state.add_string(&group.name)?;
                validate_region(&group.region, anchors, state)?;
                let mut child_anchors = anchors.clone();
                if validate_commands(&group.children, depth + 1, false, &mut child_anchors, state)?
                    .is_empty()
                {
                    return Err(PdfLayoutError::NoOutput);
                }
            }
            PdfCommand::EdgeRows(rows) => {
                validate_region(&rows.region, anchors, state)?;
                if let Some(anchor) = &rows.fallback_anchor {
                    validate_region(anchor, anchors, state)?;
                }
                validate_edge_find(rows.find)?;
                if rows
                    .minimum_extent
                    .is_some_and(|extent| !extent.is_finite() || extent <= 0.0)
                {
                    return Err(PdfLayoutError::InvalidMinimumExtent);
                }
                let mut child_anchors = anchors.clone();
                let child_schema =
                    validate_commands(&rows.children, depth + 1, false, &mut child_anchors, state)?;
                if child_schema.is_empty() {
                    return Err(PdfLayoutError::NoOutput);
                }
                if let Some(node) = child_schema.iter().find(|node| !node.repeating) {
                    return Err(PdfLayoutError::NonRepeatingRowOutput(node.name.clone()));
                }
            }
            PdfCommand::TextGroups(groups) => {
                validate_region(&groups.region, anchors, state)?;
                if groups.groups.is_empty() {
                    return Err(PdfLayoutError::EmptyTextGroups);
                }
                for group in &groups.groups {
                    state.add_node()?;
                    if group.matcher.normalizes_empty() {
                        return Err(PdfLayoutError::EmptyTextNeedle);
                    }
                    state.add_string(&group.matcher.needle)?;
                    if let Some(face) = &group.matcher.properties.font_face {
                        validate_name(face, "font face")?;
                        state.add_string(face)?;
                    }
                    for metric in [
                        group.matcher.properties.cell_height,
                        group.matcher.properties.baseline_angle,
                    ]
                    .into_iter()
                    .flatten()
                    {
                        if !metric.value.is_finite()
                            || !metric.deviation.is_finite()
                            || metric.deviation < 0.0
                        {
                            return Err(PdfLayoutError::InvalidTextMetric);
                        }
                    }
                    if group
                        .matcher
                        .properties
                        .cell_height
                        .is_some_and(|metric| metric.value <= 0.0)
                    {
                        return Err(PdfLayoutError::InvalidTextMetric);
                    }
                    if let PdfTextGroupOutput::Repeated { name } = &group.output {
                        validate_name(name, "text group")?;
                        state.add_string(name)?;
                    }
                    let mut child_anchors = anchors.clone();
                    if validate_commands(
                        &group.children,
                        depth + 1,
                        false,
                        &mut child_anchors,
                        state,
                    )?
                    .is_empty()
                    {
                        return Err(PdfLayoutError::NoOutput);
                    }
                }
            }
            PdfCommand::TextRows(rows) => {
                validate_region(&rows.region, anchors, state)?;
                if rows
                    .minimum_extent
                    .is_some_and(|extent| !extent.is_finite() || extent <= 0.0)
                {
                    return Err(PdfLayoutError::InvalidMinimumExtent);
                }
                let mut child_anchors = anchors.clone();
                let child_schema =
                    validate_commands(&rows.children, depth + 1, false, &mut child_anchors, state)?;
                if child_schema.is_empty() {
                    return Err(PdfLayoutError::NoOutput);
                }
                if let Some(node) = child_schema.iter().find(|node| !node.repeating) {
                    return Err(PdfLayoutError::NonRepeatingRowOutput(node.name.clone()));
                }
            }
            PdfCommand::Pages(pages) => {
                if !allow_document_commands {
                    return Err(PdfLayoutError::NestedDocumentCommand("pages"));
                }
                pages.selection.validate()?;
                let mut child_anchors = Vec::new();
                let child_schema = validate_commands(
                    &pages.children,
                    depth + 1,
                    false,
                    &mut child_anchors,
                    state,
                )?;
                if child_schema.is_empty() {
                    return Err(PdfLayoutError::NoOutput);
                }
                if !pages.selection.is_single_page()
                    && let Some(node) = child_schema.iter().find(|node| !node.repeating)
                {
                    return Err(PdfLayoutError::NonRepeatingDocumentOutput {
                        command: "page selection",
                        name: node.name.clone(),
                    });
                }
            }
            PdfCommand::Merge(merge) => {
                if !allow_document_commands {
                    return Err(PdfLayoutError::NestedDocumentCommand("merge"));
                }
                validate_name(&merge.name, "merge")?;
                state.add_string(&merge.name)?;
                if merge.sources.is_empty() {
                    return Err(PdfLayoutError::EmptyMergeSources(merge.name.clone()));
                }
                let source_anchors = Vec::new();
                for source in &merge.sources {
                    source.page_selection.validate()?;
                    validate_region(&source.region, &source_anchors, state)?;
                }
                let mut child_anchors = Vec::new();
                let child_schema = validate_commands(
                    &merge.children,
                    depth + 1,
                    false,
                    &mut child_anchors,
                    state,
                )?;
                if child_schema.is_empty() {
                    return Err(PdfLayoutError::NoOutput);
                }
                let has_multiple_candidates = merge.composition == PdfMergeComposition::Independent
                    && (merge.sources.len() > 1
                        || merge
                            .sources
                            .iter()
                            .any(|source| !source.page_selection.is_single_page()));
                if has_multiple_candidates
                    && let Some(node) = child_schema.iter().find(|node| !node.repeating)
                {
                    return Err(PdfLayoutError::NonRepeatingDocumentOutput {
                        command: "merge",
                        name: node.name.clone(),
                    });
                }
            }
            PdfCommand::Anchor(anchor) => {
                validate_name(&anchor.name, "anchor")?;
                state.add_string(&anchor.name)?;
                validate_coordinate(&anchor.at, Some(anchor.axis), anchors, state)?;
                insert_anchor(anchors, &anchor.name, anchor.axis)?;
            }
            PdfCommand::BoundaryFindVertical(boundary) => {
                validate_name(&boundary.begin_anchor, "anchor")?;
                validate_name(&boundary.end_anchor, "anchor")?;
                state.add_string(&boundary.begin_anchor)?;
                state.add_string(&boundary.end_anchor)?;
                validate_region(&boundary.region, anchors, state)?;
                validate_edge_find(boundary.find)?;
                insert_anchor(anchors, &boundary.begin_anchor, PdfAnchorAxis::Vertical)?;
                insert_anchor(anchors, &boundary.end_anchor, PdfAnchorAxis::Vertical)?;
            }
        }
        for node in command.schema_nodes() {
            if !output_names.insert(node.name.clone()) {
                return Err(PdfLayoutError::DuplicateOutput(node.name));
            }
            schema.push(node);
        }
    }
    Ok(schema)
}

impl PdfTextMatch {
    fn normalizes_empty(&self) -> bool {
        if self.flexible_whitespace {
            self.needle.chars().all(char::is_whitespace)
        } else {
            self.needle.is_empty()
        }
    }
}

fn insert_anchor(
    anchors: &mut Anchors,
    name: &str,
    axis: PdfAnchorAxis,
) -> Result<(), PdfLayoutError> {
    if anchors.iter().any(|(candidate, _)| candidate == name) {
        return Err(PdfLayoutError::DuplicateAnchor(name.to_owned()));
    }
    anchors.push((name.to_owned(), axis));
    Ok(())
}

fn validate_region(
    region: &PdfRegion,
    anchors: &Anchors,
    state: &mut ValidationState,
) -> Result<(), PdfLayoutError> {
    validate_coordinate(
        &region.left,
        Some(PdfAnchorAxis::Horizontal),
        anchors,
        state,
    )?;
    validate_coordinate(&region.top, Some(PdfAnchorAxis::Vertical), anchors, state)?;
    validate_coordinate(
        &region.right,
        Some(PdfAnchorAxis::Horizontal),
        anchors,
        state,
    )?;
    validate_coordinate(
        &region.bottom,
        Some(PdfAnchorAxis::Vertical),
        anchors,
        state,
    )
}

fn validate_coordinate(
    coordinate: &PdfCoordinate,
    expected_axis: Option<PdfAnchorAxis>,
    anchors: &Anchors,
    state: &mut ValidationState,
) -> Result<(), PdfLayoutError> {
    if !coordinate.offset.is_finite() {
        return Err(PdfLayoutError::InvalidCoordinate);
    }
    let axis = match &coordinate.reference {
        PdfReference::Left | PdfReference::Right => PdfAnchorAxis::Horizontal,
        PdfReference::Top | PdfReference::Bottom => PdfAnchorAxis::Vertical,
        PdfReference::Anchor(name) => {
            validate_name(name, "anchor")?;
            state.add_string(name)?;
            anchors
                .iter()
                .find(|(candidate, _)| candidate == name)
                .map(|(_, axis)| *axis)
                .ok_or_else(|| PdfLayoutError::UnknownAnchor(name.clone()))?
        }
    };
    if let Some(expected_axis) = expected_axis
        && expected_axis != axis
    {
        let name = match &coordinate.reference {
            PdfReference::Anchor(name) => name.clone(),
            other => format!("{other:?}"),
        };
        return Err(PdfLayoutError::InvalidAnchorAxis(name));
    }
    Ok(())
}

fn validate_edge_find(find: PdfEdgeFind) -> Result<(), PdfLayoutError> {
    if !find.fill.is_finite()
        || find.fill <= 0.0
        || !find.prominence.is_finite()
        || find.prominence < 0.0
    {
        return Err(PdfLayoutError::InvalidEdgeFind);
    }
    Ok(())
}

fn validate_name(name: &str, role: &'static str) -> Result<(), PdfLayoutError> {
    if name.is_empty() {
        return Err(PdfLayoutError::EmptyName(role));
    }
    if name.contains(['/', '\\']) {
        return Err(PdfLayoutError::InvalidName {
            role,
            name: name.to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "pdf_tests.rs"]
mod tests;
