//! Opt-in budgets for the native table codec, not a taxonomy processor.

use ir::{Instance, SchemaKind, SchemaNode, Value};

use crate::{Dimension, Fact, TargetRow, XbrlFormatError};

mod profile;
#[cfg(test)]
mod tests;
mod xml;

pub(crate) use xml::XmlBudget;

/// A resource counted by the bounded codec. XML nodes count the document and
/// every start/empty, text/CDATA/reference, comment, PI and declaration event.
/// Adjacent text fragments can therefore count more than one DOM text node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XbrlLimit {
    InputBytes,
    OutputBytes,
    XmlDepth,
    XmlNodes,
    XmlAttributes,
    XmlTextBytes,
    Facts,
    Contexts,
    Units,
    Dimensions,
    ModelDepth,
    ModelNodes,
    ModelTextBytes,
    StringBytes,
}

/// Finite budgets. Text lengths are UTF-8 bytes. XML text includes raw attribute
/// values (including namespace declarations), comments and PI data; references
/// count their wire spelling, so decoding cannot enlarge the budget. Model
/// budgets cover schema/options plus input on write, and schema/options plus
/// the materialized result on read. Counts are aggregate, not per context.
#[derive(Clone, Debug)]
pub struct XbrlLimitOptions {
    pub max_input_bytes: usize,
    pub max_output_bytes: usize,
    pub max_xml_depth: usize,
    pub max_xml_nodes: usize,
    pub max_xml_attributes: usize,
    pub max_xml_text_bytes: usize,
    pub max_facts: usize,
    pub max_contexts: usize,
    pub max_units: usize,
    pub max_dimensions: usize,
    pub max_model_depth: usize,
    pub max_model_nodes: usize,
    pub max_model_text_bytes: usize,
    pub max_string_bytes: usize,
}

impl Default for XbrlLimitOptions {
    fn default() -> Self {
        Self {
            max_input_bytes: 16 * 1024 * 1024,
            max_output_bytes: 16 * 1024 * 1024,
            max_xml_depth: 64,
            max_xml_nodes: 1_000_000,
            max_xml_attributes: 1_000_000,
            max_xml_text_bytes: 16 * 1024 * 1024,
            max_facts: 100_000,
            max_contexts: 10_000,
            max_units: 10_000,
            max_dimensions: 100_000,
            max_model_depth: 64,
            max_model_nodes: 1_000_000,
            max_model_text_bytes: 64 * 1024 * 1024,
            max_string_bytes: 1024 * 1024,
        }
    }
}

/// Validated, immutable limits. Zero permits no occurrences of a resource;
/// depth must be in 1..=256 and every value must fit in `isize`.
#[derive(Clone, Debug, Default)]
pub struct XbrlLimits(XbrlLimitOptions);

impl XbrlLimits {
    pub fn new(options: XbrlLimitOptions) -> Result<Self, XbrlFormatError> {
        use XbrlLimit::*;
        for (limit, value) in [
            (InputBytes, options.max_input_bytes),
            (OutputBytes, options.max_output_bytes),
            (XmlDepth, options.max_xml_depth),
            (XmlNodes, options.max_xml_nodes),
            (XmlAttributes, options.max_xml_attributes),
            (XmlTextBytes, options.max_xml_text_bytes),
            (Facts, options.max_facts),
            (Contexts, options.max_contexts),
            (Units, options.max_units),
            (Dimensions, options.max_dimensions),
            (ModelDepth, options.max_model_depth),
            (ModelNodes, options.max_model_nodes),
            (ModelTextBytes, options.max_model_text_bytes),
            (StringBytes, options.max_string_bytes),
        ] {
            if value > isize::MAX as usize
                || (matches!(limit, XmlDepth | ModelDepth) && (value == 0 || value > 256))
            {
                return Err(XbrlFormatError::InvalidLimit(limit));
            }
        }
        Ok(Self(options))
    }

    pub fn options(&self) -> &XbrlLimitOptions {
        &self.0
    }
}

pub(super) fn check(
    limit: XbrlLimit,
    actual: usize,
    maximum: usize,
) -> Result<(), XbrlFormatError> {
    if actual > maximum {
        Err(XbrlFormatError::LimitExceeded {
            limit,
            maximum,
            actual,
        })
    } else {
        Ok(())
    }
}

pub(super) fn add(
    total: &mut usize,
    amount: usize,
    limit: XbrlLimit,
    maximum: usize,
) -> Result<(), XbrlFormatError> {
    *total = total
        .checked_add(amount)
        .ok_or(XbrlFormatError::LimitExceeded {
            limit,
            maximum,
            actual: usize::MAX,
        })?;
    check(limit, *total, maximum)
}

/// Reads UTF-8 XBRL after bounded XML preflight, then uses the existing typed
/// table projection. Namespace bindings are mandatory. All facts, including
/// unselected concepts, must have valid references and supported leaf shapes.
/// No resource is opened, fetched or expanded. See the crate README for the
/// deliberately closed instance profile and dimension projection limitation.
pub fn from_bytes_bounded(
    bytes: &[u8],
    schema: &SchemaNode,
    options: &mapping::XbrlBoundaryOptions,
    limits: &XbrlLimits,
) -> Result<Instance, XbrlFormatError> {
    let limits = limits.options();
    check(XbrlLimit::InputBytes, bytes.len(), limits.max_input_bytes)?;
    let text = std::str::from_utf8(bytes)
        .map_err(|_| XbrlFormatError::XmlPreflight("input is not UTF-8".into()))?;
    xml::preflight(text, limits)?;
    let mut budget = ModelBudget::new(limits);
    budget.schema(schema, 1, true)?;
    budget.options(options)?;
    // This is the first DOM allocation. DTDs and entity expansion have already
    // been refused and every XML event, including unselected content, counted.
    let document = roxmltree::Document::parse(text)?;
    profile::validate(&document)?;
    let namespaces = crate::namespace_map(options);
    crate::project_document(&document, schema, Some(&namespaces), Some(&mut budget))
}

/// Writes byte-identical native XML for the supported profile. Input/schema
/// preflight bounds collection and scratch data; the shared renderer first
/// runs into a checked counting sink. Only a successful count allocates the
/// exact output buffer, which is checked again while rendering.
pub fn to_bytes_bounded(
    schema: &SchemaNode,
    instance: &Instance,
    options: &mapping::XbrlBoundaryOptions,
    limits: &XbrlLimits,
) -> Result<Vec<u8>, XbrlFormatError> {
    let limits = limits.options();
    let mut budget = ModelBudget::new(limits);
    budget.schema(schema, 1, false)?;
    budget.options(options)?;
    budget.instance(instance, 1)?;
    crate::table_row_path(schema)?;
    profile::target_schema(schema, &crate::namespace_map(options), &mut Vec::new())?;
    let (rows, defaults, prefixes) = crate::target_parts(schema, instance, options, Some(limits))?;
    profile::target(&rows, &defaults.units)?;
    let count = crate::render::render_bounded(
        std::io::sink(),
        options.taxonomy(),
        &rows,
        &defaults.units,
        &prefixes,
        limits,
    )?
    .1;
    let (bytes, _) = crate::render::render_bounded(
        Vec::with_capacity(count),
        options.taxonomy(),
        &rows,
        &defaults.units,
        &prefixes,
        limits,
    )?;
    // The same closed profile applies to generated XML (names, QNames, units,
    // contexts and references), without substituting another business codec.
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| XbrlFormatError::XmlPreflight("output is not UTF-8".into()))?;
    let document = roxmltree::Document::parse(text)?;
    profile::validate(&document)?;
    Ok(bytes)
}

pub(crate) struct ModelBudget<'a> {
    limits: &'a XbrlLimitOptions,
    nodes: usize,
    text: usize,
}

impl<'a> ModelBudget<'a> {
    pub(crate) fn new(limits: &'a XbrlLimitOptions) -> Self {
        Self {
            limits,
            nodes: 0,
            text: 0,
        }
    }

    pub(crate) fn node(&mut self, depth: usize) -> Result<(), XbrlFormatError> {
        check(XbrlLimit::ModelDepth, depth, self.limits.max_model_depth)?;
        add(
            &mut self.nodes,
            1,
            XbrlLimit::ModelNodes,
            self.limits.max_model_nodes,
        )
    }

    pub(crate) fn text(&mut self, text: &str) -> Result<(), XbrlFormatError> {
        check(
            XbrlLimit::StringBytes,
            text.len(),
            self.limits.max_string_bytes,
        )?;
        add(
            &mut self.text,
            text.len(),
            XbrlLimit::ModelTextBytes,
            self.limits.max_model_text_bytes,
        )
    }

    fn schema(
        &mut self,
        schema: &SchemaNode,
        depth: usize,
        reading: bool,
    ) -> Result<(), XbrlFormatError> {
        self.node(depth)?;
        self.text(&schema.name)?;
        if schema.name == "typedMember" || (reading && schema.name == "explicitMember") {
            return Err(XbrlFormatError::UnsupportedBoundedProfile(
                "typed dimensions and input dimension ports are not supported by the native table codec",
            ));
        }
        if let SchemaKind::Group { children, .. } = &schema.kind {
            for child in children {
                self.schema(child, depth + 1, reading)?;
            }
        }
        Ok(())
    }

    fn options(&mut self, options: &mapping::XbrlBoundaryOptions) -> Result<(), XbrlFormatError> {
        self.text(options.taxonomy())?;
        for binding in options.namespace_bindings() {
            self.node(1)?;
            self.text(binding.namespace())?;
            for segment in binding.path() {
                self.text(segment)?;
            }
        }
        for binding in options.fact_bindings() {
            self.node(1)?;
            for segment in binding.path() {
                self.text(segment)?;
            }
        }
        Ok(())
    }

    fn instance(&mut self, instance: &Instance, depth: usize) -> Result<(), XbrlFormatError> {
        self.node(depth)?;
        match instance {
            Instance::Scalar(Value::String(text)) => self.text(text)?,
            Instance::Scalar(_) => {}
            Instance::Group(fields) => {
                for (name, value) in fields.iter() {
                    self.text(name)?;
                    self.instance(value, depth + 1)?;
                }
            }
            Instance::Repeated(items) => {
                for item in items {
                    self.instance(item, depth + 1)?;
                }
            }
            Instance::DocumentSet(_) | Instance::MappedSequence(_) => {
                return Err(XbrlFormatError::UnsupportedBoundedProfile(
                    "document sets or mapped sequences",
                ));
            }
        }
        Ok(())
    }
}

// Collection budgets are separate from caller-owned model storage. This also
// caps repeated copies of shared entity identifiers and typed fact defaults.
pub(crate) struct BuildBudget<'a> {
    model: ModelBudget<'a>,
}

impl<'a> BuildBudget<'a> {
    pub(crate) fn new(limits: &'a XbrlLimitOptions) -> Self {
        Self {
            model: ModelBudget::new(limits),
        }
    }

    pub(crate) fn row(&mut self, row: &TargetRow) -> Result<(), XbrlFormatError> {
        // Candidate rows are bounded as model data even when later pruned.
        // XML resource counts apply only to the emitted document.
        self.model.node(1)?;
        self.model.text(&row.path)?;
        self.model.text(&row.identifier.value)?;
        self.model.text(&row.identifier.scheme)
    }

    pub(crate) fn fact(&mut self, fact: &Fact) -> Result<(), XbrlFormatError> {
        self.model.node(1)?;
        for text in [&fact.namespace, &fact.name, &fact.value]
            .into_iter()
            .chain(fact.unit_ref.iter())
            .chain(fact.decimals.iter())
        {
            self.model.text(text)?;
        }
        Ok(())
    }

    pub(crate) fn dimension(&mut self, dimension: &Dimension) -> Result<(), XbrlFormatError> {
        self.model.node(1)?;
        for text in [&dimension.namespace, &dimension.name, &dimension.member] {
            self.model.text(text)?;
        }
        Ok(())
    }
}
