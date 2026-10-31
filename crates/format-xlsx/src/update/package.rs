//! Package preflight for hierarchical updates. The workbook reader parses
//! without budgets and panics on content it cannot handle, and its writer
//! re-emits only the parts it models, so the package is expanded with real
//! byte counts, its XML is checked against the budgets, and its relationship
//! graph is walked the way that reader resolves it before anything is parsed.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

use quick_xml::events::{BytesStart, Event};

use super::{HierarchicalUpdateError, HierarchicalUpdateRefusal};
use crate::bounded::{add, check};
use crate::{XlsxFormatError, XlsxLimit, XlsxLimitOptions};

const OFFICE_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
const CORE_PROPERTIES: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
/// Parts the workbook reader loads by name rather than by relationship.
const NAMED_PROJECT_PARTS: &[(&str, &str)] = &[
    (
        "xl/vbaproject.bin",
        "http://schemas.microsoft.com/office/2006/relationships/vbaProject",
    ),
    (
        "xl/jsaproject.bin",
        "http://schemas.onlyoffice.com/jsaProject",
    ),
];

/// The role of a package part, which decides the relationships its writer
/// re-emits and how its content is parsed.
#[derive(Clone, Copy)]
enum Role {
    Package,
    Workbook,
    Worksheet,
    Drawing,
    VmlDrawing,
    /// An XML part whose relationships the workbook writer never re-emits.
    XmlLeaf,
    /// A binary part whose relationships the workbook writer never re-emits.
    BinaryLeaf,
}

impl Role {
    /// The role of a related part, or `None` when the writer would drop the
    /// relationship. A fixed name is the part the reader loads by name, which
    /// the relationship must therefore target.
    fn related(self, relationship: &str) -> Option<(Self, Option<&'static str>)> {
        if matches!(self, Self::Package) && relationship == CORE_PROPERTIES {
            return Some((Self::XmlLeaf, Some("docprops/core.xml")));
        }
        let kind = relationship.strip_prefix(OFFICE_RELATIONSHIP)?;
        Some(match (self, kind) {
            (Self::Package, "officeDocument") => (Self::Workbook, Some("xl/workbook.xml")),
            (Self::Package, "extended-properties") => (Self::XmlLeaf, Some("docprops/app.xml")),
            (Self::Package, "custom-properties") => (Self::XmlLeaf, Some("docprops/custom.xml")),
            (Self::Workbook, "worksheet") => (Self::Worksheet, None),
            (Self::Workbook, "styles") => (Self::XmlLeaf, Some("xl/styles.xml")),
            (Self::Workbook, "sharedStrings") => (Self::XmlLeaf, Some("xl/sharedstrings.xml")),
            // calcChain is a derived cache that spreadsheet applications rebuild.
            (Self::Workbook, "theme" | "calcChain") => (Self::XmlLeaf, None),
            (Self::Worksheet, "drawing") => (Self::Drawing, None),
            (Self::Worksheet, "vmlDrawing") => (Self::VmlDrawing, None),
            (Self::Worksheet, "comments" | "table") | (Self::Drawing, "chart") => {
                (Self::XmlLeaf, None)
            }
            (Self::Worksheet, "hyperlink" | "printerSettings")
            | (Self::Drawing | Self::VmlDrawing, "image") => (Self::BinaryLeaf, None),
            _ => return None,
        })
    }

    /// Whether the reader uses this part's relationships at all and, below a
    /// worksheet, the case-sensitive prefix its relationships part must have:
    /// the reader picks a worksheet's, drawing's, or VML drawing's
    /// relationships from everything loaded for the worksheet by that prefix.
    fn relationships(self) -> Option<Option<&'static str>> {
        match self {
            Self::Package | Self::Workbook => Some(None),
            Self::Worksheet => Some(Some("xl/worksheets/_rels/sheet")),
            Self::Drawing => Some(Some("xl/drawings/_rels/drawing")),
            Self::VmlDrawing => Some(Some("xl/drawings/_rels/vmlDrawing")),
            Self::XmlLeaf | Self::BinaryLeaf => None,
        }
    }

    /// `Some(lenient)` for parts the reader parses as XML. VML is legacy
    /// markup that may leave HTML-style elements unclosed.
    fn xml(self) -> Option<bool> {
        match self {
            Self::Package | Self::BinaryLeaf => None,
            Self::VmlDrawing => Some(true),
            _ => Some(false),
        }
    }
}

/// One expanded package part.
struct Part {
    /// The ZIP entry name as stored.
    name: String,
    bytes: Vec<u8>,
}

/// Expanded parts by ASCII-lowercased name, matching the reader's
/// case-insensitive lookup.
type Parts = BTreeMap<String, Part>;

/// Refuses packages whose content the workbook reader cannot parse within the
/// limits, or whose related parts the workbook writer would drop.
pub(super) fn inspect_package(
    bytes: &[u8],
    limits: &XlsxLimitOptions,
) -> Result<(), HierarchicalUpdateError> {
    let parts = expand_package(bytes, limits)?;
    for (name, relationship) in NAMED_PROJECT_PARTS {
        if let Some(part) = parts.get(*name) {
            return Err(HierarchicalUpdateRefusal::UnsupportedPart {
                part: part.name.clone(),
                relationship: (*relationship).to_string(),
            }
            .into());
        }
    }
    let mut budget = XmlBudget {
        limits,
        nodes: 0,
        text: 0,
        validated: BTreeMap::new(),
    };
    for (key, part) in &parts {
        let lenient = key.ends_with(".vml");
        if lenient || key.ends_with(".xml") || key.ends_with(".rels") {
            budget.validate(key, part, lenient)?;
        }
    }
    let sheet_ids = sheet_references(&parts)?;

    let mut walk = Walk {
        parts: &parts,
        limits,
        budget,
        loaded: 0,
        sheet_ids,
    };
    walk.part(&mut Vec::new(), "", Role::Package, &mut BTreeSet::new())?;
    // The reader loads the workbook and its relationships by name in any case.
    walk.part(
        &mut Vec::new(),
        "xl/workbook.xml",
        Role::Workbook,
        &mut BTreeSet::new(),
    )
}

/// Expands every entry while counting actual bytes against the budgets. Names
/// must be unambiguous under the reader's backslash- and case-tolerant lookup.
fn expand_package(bytes: &[u8], limits: &XlsxLimitOptions) -> Result<Parts, XlsxFormatError> {
    let package_error = |error: zip::result::ZipError| XlsxFormatError::Update(error.to_string());
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(package_error)?;
    check(XlsxLimit::Entries, archive.len(), limits.max_entries)?;
    let mut total = 0_usize;
    let mut parts = Parts::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(package_error)?;
        let name = file.name().to_string();
        let key = name.to_ascii_lowercase();
        if name.contains('\\') || parts.contains_key(&key) {
            return Err(XlsxFormatError::Update(format!(
                "ZIP part name `{name}` is ambiguous"
            )));
        }
        let declared = file.size();
        let mut content = Vec::new();
        let mut chunk = [0; 8192];
        loop {
            // Probe one byte beyond either budget; never trust declared sizes.
            let remaining = limits
                .max_entry_bytes
                .saturating_sub(content.len())
                .min(limits.max_expanded_bytes.saturating_sub(total));
            let size = chunk.len().min(remaining.saturating_add(1));
            let read = file.read(&mut chunk[..size])?;
            if read == 0 {
                break;
            }
            check(
                XlsxLimit::EntryBytes,
                content.len() + read,
                limits.max_entry_bytes,
            )?;
            add(
                &mut total,
                read,
                XlsxLimit::ExpandedBytes,
                limits.max_expanded_bytes,
            )?;
            content.extend_from_slice(&chunk[..read]);
        }
        if u64::try_from(content.len()).ok() != Some(declared) {
            return Err(XlsxFormatError::Update(format!(
                "expanded size of `{name}` disagrees with the ZIP directory"
            )));
        }
        parts.insert(
            key,
            Part {
                name,
                bytes: content,
            },
        );
    }
    Ok(parts)
}

/// The shared XML budgets, charged once per part and parse mode.
struct XmlBudget<'a> {
    limits: &'a XlsxLimitOptions,
    nodes: usize,
    text: usize,
    /// Validated parts: lowercased name -> whether only leniently.
    validated: BTreeMap<String, bool>,
}

impl XmlBudget<'_> {
    fn validate(&mut self, key: &str, part: &Part, lenient: bool) -> Result<(), XlsxFormatError> {
        if self
            .validated
            .get(key)
            .is_some_and(|&only_lenient| lenient || !only_lenient)
        {
            return Ok(());
        }
        validate_xml(
            &part.name,
            lenient,
            &part.bytes,
            self.limits,
            &mut self.nodes,
            &mut self.text,
        )?;
        self.validated.insert(key.to_string(), lenient);
        Ok(())
    }
}

/// Checks well-formedness and the shared XML budgets; a lenient check applies
/// only the budgets.
fn validate_xml(
    name: &str,
    lenient: bool,
    xml: &[u8],
    limits: &XlsxLimitOptions,
    nodes: &mut usize,
    text: &mut usize,
) -> Result<(), XlsxFormatError> {
    let invalid = |reason: &dyn std::fmt::Display| {
        XlsxFormatError::Update(format!("`{name}` is not well-formed XML: {reason}"))
    };
    let mut reader = quick_xml::Reader::from_reader(xml);
    reader.config_mut().check_end_names = !lenient;
    let mut depth = 0_usize;
    loop {
        let event = reader.read_event().map_err(|error| invalid(&error))?;
        let element = match &event {
            Event::Start(element) => {
                depth += 1;
                Some(element)
            }
            Event::Empty(element) => Some(element),
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                None
            }
            Event::Text(content) => {
                add(
                    text,
                    content.len(),
                    XlsxLimit::XmlTextBytes,
                    limits.max_xml_text_bytes,
                )?;
                None
            }
            Event::CData(content) => {
                add(
                    text,
                    content.len(),
                    XlsxLimit::XmlTextBytes,
                    limits.max_xml_text_bytes,
                )?;
                None
            }
            Event::Eof if depth != 0 && !lenient => return Err(invalid(&"unclosed element")),
            Event::Eof => return Ok(()),
            _ => None,
        };
        if let Some(element) = element {
            let level = depth + usize::from(matches!(event, Event::Empty(_)));
            check(XlsxLimit::XmlDepth, level, limits.max_xml_depth)?;
            add(nodes, 1, XlsxLimit::XmlNodes, limits.max_xml_nodes)?;
            for attribute in element.attributes() {
                let attribute = attribute.map_err(|error| invalid(&error))?;
                add(
                    text,
                    attribute.value.len(),
                    XlsxLimit::XmlTextBytes,
                    limits.max_xml_text_bytes,
                )?;
            }
        }
    }
}

/// The relationship ids the workbook's sheets name. Each must be unique: the
/// reader loads a worksheet's parts once per sheet that names it.
fn sheet_references(parts: &Parts) -> Result<BTreeSet<String>, XlsxFormatError> {
    let mut ids = BTreeSet::new();
    let Some(workbook) = parts.get("xl/workbook.xml") else {
        return Ok(ids);
    };
    let invalid = |reason: &dyn std::fmt::Display| {
        XlsxFormatError::Update(format!("invalid sheets in `{}`: {reason}", workbook.name))
    };
    let mut reader = quick_xml::Reader::from_reader(workbook.bytes.as_slice());
    loop {
        let element = match reader.read_event().map_err(|error| invalid(&error))? {
            Event::Start(element) | Event::Empty(element) => element,
            Event::Eof => return Ok(ids),
            _ => continue,
        };
        if element.name().as_ref() != b"sheet" {
            continue;
        }
        let id = reader_relationship_id(&element)
            .ok_or_else(|| invalid(&"a sheet has no relationship id"))?;
        let id = String::from_utf8(id).map_err(|error| invalid(&error))?;
        if ids.contains(&id) {
            return Err(invalid(&format_args!(
                "relationship `{id}` is named by more than one sheet"
            )));
        }
        ids.insert(id);
    }
}

/// An element's `r:id`, or else its first attribute with the local name `id`,
/// as the reader looks it up.
fn reader_relationship_id(element: &BytesStart<'_>) -> Option<Vec<u8>> {
    let mut local = None;
    for attribute in element.attributes().with_checks(false).flatten() {
        if attribute.key.as_ref() == b"r:id" {
            return Some(attribute.value.into_owned());
        }
        if local.is_none() && attribute.key.local_name().as_ref() == b"id" {
            local = Some(attribute.value.into_owned());
        }
    }
    local
}

/// Requires every worksheet relationship to be named by a sheet, and every
/// sheet to name an internal worksheet relationship: the reader loads only
/// the worksheets sheets name, and its writer emits only those.
fn check_workbook_references(
    relationships_part: Option<&Part>,
    relationships: &[Relationship],
    sheet_ids: &BTreeSet<String>,
) -> Result<(), HierarchicalUpdateError> {
    let worksheet = office_relationship("worksheet");
    for relationship in relationships
        .iter()
        .filter(|relationship| relationship.kind == worksheet)
    {
        if !sheet_ids.contains(&relationship.id) {
            return Err(HierarchicalUpdateRefusal::UnreferencedRelationship {
                part: relationships_part.map_or_else(String::new, |part| part.name.clone()),
                id: relationship.id.clone(),
            }
            .into());
        }
    }
    for id in sheet_ids {
        if !relationships.iter().any(|relationship| {
            &relationship.id == id && relationship.kind == worksheet && !relationship.external
        }) {
            return Err(XlsxFormatError::Update(format!(
                "sheet relationship `{id}` names no internal worksheet"
            ))
            .into());
        }
    }
    Ok(())
}

/// Worksheet elements whose relationship the reader keeps, the relationship
/// type each must name, and whether the reader reads only the empty element.
/// Comments, drawings, VML drawings, and tables are loaded by relationship
/// type; hyperlinks and printer settings only through their element.
const WORKSHEET_REFERENCES: &[(&[u8], &str, bool)] = &[
    (b"drawing", "drawing", false),
    (b"legacyDrawing", "vmlDrawing", false),
    (b"tablePart", "table", false),
    (b"hyperlink", "hyperlink", true),
    (b"pageSetup", "printerSettings", true),
];

/// Refuses relationship references from worksheet elements the reader
/// ignores (such as `legacyDrawingHF`, `control`, or `picture`) and internal
/// relationships nothing references, apart from comments, which the reader
/// loads without one.
fn check_worksheet_references(
    sheet: &Part,
    relationships_part: Option<&Part>,
    relationships: &[Relationship],
) -> Result<(), HierarchicalUpdateError> {
    let invalid = |error: &dyn std::fmt::Display| {
        XlsxFormatError::Update(format!("`{}` is not well-formed XML: {error}", sheet.name))
    };
    let mut referenced = BTreeSet::new();
    let mut reader = quick_xml::Reader::from_reader(sheet.bytes.as_slice());
    loop {
        let (element, empty) = match reader.read_event().map_err(|error| invalid(&error))? {
            Event::Start(element) => (element, false),
            Event::Empty(element) => (element, true),
            Event::Eof => break,
            _ => continue,
        };
        let reference = WORKSHEET_REFERENCES
            .iter()
            .find(|(name, ..)| element.name().as_ref() == *name);
        let id = match reference {
            Some(_) => reader_relationship_id(&element),
            None => element
                .attributes()
                .with_checks(false)
                .flatten()
                .find(|attribute| {
                    attribute.key.prefix().is_some() && attribute.key.local_name().as_ref() == b"id"
                })
                .map(|attribute| attribute.value.into_owned()),
        };
        let Some(id) = id else {
            continue;
        };
        let refusal = || HierarchicalUpdateRefusal::UnsupportedReference {
            part: sheet.name.clone(),
            element: String::from_utf8_lossy(element.name().as_ref()).into_owned(),
        };
        let Some(&(_, kind, empty_only)) = reference else {
            return Err(refusal().into());
        };
        let id = String::from_utf8(id).map_err(|_| refusal())?;
        let kind = office_relationship(kind);
        if (empty_only && !empty)
            || !relationships
                .iter()
                .any(|relationship| relationship.id == id && relationship.kind == kind)
        {
            return Err(refusal().into());
        }
        referenced.insert(id);
    }
    let comments = office_relationship("comments");
    for relationship in relationships {
        if !relationship.external
            && relationship.kind != comments
            && !referenced.contains(&relationship.id)
        {
            return Err(HierarchicalUpdateRefusal::UnreferencedRelationship {
                part: relationships_part.map_or_else(String::new, |part| part.name.clone()),
                id: relationship.id.clone(),
            }
            .into());
        }
    }
    Ok(())
}

/// VML element names that draw a shape.
const VML_SHAPES: &[&[u8]] = &[
    b"shape",
    b"rect",
    b"roundrect",
    b"oval",
    b"line",
    b"polyline",
    b"curve",
    b"arc",
    b"image",
    b"group",
];

/// Refuses VML drawings the writer would not keep whole. It keeps a VML
/// drawing only for a worksheet with comments, and of its shapes only
/// `v:shape` elements whose `x:ClientData` is a comment note (`ObjectType`
/// `Note` with an `x:Column`); other shapes would belong to OLE objects,
/// which are refused separately, and are otherwise dropped.
fn check_vml_drawing(part: &Part, has_comments: bool) -> Result<(), HierarchicalUpdateError> {
    let refusal = || -> HierarchicalUpdateError {
        HierarchicalUpdateRefusal::UnsupportedVmlShape {
            part: part.name.clone(),
        }
        .into()
    };
    if !has_comments {
        return Err(refusal());
    }
    let mut reader = quick_xml::Reader::from_reader(part.bytes.as_slice());
    reader.config_mut().check_end_names = false;
    // Inside a shape: whether its client data is a comment note so far.
    let mut shape = None::<bool>;
    let mut client_data = false;
    loop {
        let (element, start) = match reader.read_event().map_err(|error| {
            XlsxFormatError::Update(format!("`{}` is not readable VML: {error}", part.name))
        })? {
            Event::Start(element) => (element, true),
            Event::Empty(element) => (element, false),
            Event::End(element) => {
                match element.name().as_ref() {
                    b"x:ClientData" => client_data = false,
                    b"v:shape" if shape.take() != Some(true) => return Err(refusal()),
                    _ => {}
                }
                continue;
            }
            Event::Eof if shape.is_some() => return Err(refusal()),
            Event::Eof => return Ok(()),
            _ => continue,
        };
        let local = element.local_name();
        if VML_SHAPES.contains(&local.as_ref()) {
            if !start || element.name().as_ref() != b"v:shape" || shape.is_some() {
                return Err(refusal());
            }
            shape = Some(false);
        } else if local.as_ref() == b"ClientData" {
            let note = element
                .attributes()
                .with_checks(false)
                .flatten()
                .find(|attribute| attribute.key.as_ref() == b"ObjectType")
                .is_some_and(|attribute| attribute.value.as_ref() == b"Note");
            if !start || element.name().as_ref() != b"x:ClientData" || shape.is_none() || !note {
                return Err(refusal());
            }
            client_data = true;
        } else if start && client_data && element.name().as_ref() == b"x:Column" {
            shape = Some(true);
        }
    }
}

/// Children the drawing reader keeps for each anchor; it ignores
/// `xdr:absoluteAnchor` and anchors inside `mc:AlternateContent`. Its shape
/// readers end only at `xdr:`-prefixed names, so other spellings are refused.
const TWO_CELL_CHILDREN: &[&[u8]] = &[
    b"xdr:from",
    b"xdr:to",
    b"xdr:grpSp",
    b"xdr:graphicFrame",
    b"xdr:sp",
    b"xdr:cxnSp",
    b"xdr:pic",
    b"xdr:clientData",
];
const ONE_CELL_CHILDREN: &[&[u8]] = &[
    b"xdr:from",
    b"xdr:ext",
    b"xdr:grpSp",
    b"xdr:sp",
    b"xdr:pic",
    b"xdr:clientData",
];
/// The only group children the reader keeps; it drops charts, connectors,
/// and nested groups (with the pictures inside them).
const GROUP_CHILDREN: &[&[u8]] = &[b"xdr:nvGrpSpPr", b"xdr:grpSpPr", b"xdr:pic", b"xdr:sp"];

/// Refuses drawing content the reader drops, whose image or chart part the
/// writer would then silently omit: anchors and group children it does not
/// read, graphic frames that are not charts, and internal relationships
/// nothing in the drawing references.
fn check_drawing(
    drawing: &Part,
    relationships_part: Option<&Part>,
    relationships: &[Relationship],
) -> Result<(), HierarchicalUpdateError> {
    let refusal = |element: &[u8]| -> HierarchicalUpdateError {
        HierarchicalUpdateRefusal::UnsupportedDrawingContent {
            part: drawing.name.clone(),
            element: String::from_utf8_lossy(element).into_owned(),
        }
        .into()
    };
    let invalid = |error: &dyn std::fmt::Display| {
        XlsxFormatError::Update(format!(
            "`{}` is not well-formed XML: {error}",
            drawing.name
        ))
    };
    let mut reader = quick_xml::Reader::from_reader(drawing.bytes.as_slice());
    let mut open = Vec::<Vec<u8>>::new();
    // Inside an anchor's graphic frame: whether it names a chart so far.
    let mut frame_chart = None::<bool>;
    let mut referenced = BTreeSet::new();
    loop {
        let (element, empty) = match reader.read_event().map_err(|error| invalid(&error))? {
            Event::Start(element) => (element, false),
            Event::Empty(element) => (element, true),
            Event::End(_) => {
                if open.pop().as_deref() == Some(b"xdr:graphicFrame".as_slice())
                    && open.len() == 2
                    && frame_chart.take() != Some(true)
                {
                    return Err(refusal(b"xdr:graphicFrame"));
                }
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let name = element.name().as_ref().to_vec();
        let local = element.local_name();
        let allowed = !matches!(local.as_ref(), b"AlternateContent" | b"absoluteAnchor")
            && match open.as_slice() {
                [] => name == b"xdr:wsDr",
                [_] => matches!(name.as_slice(), b"xdr:oneCellAnchor" | b"xdr:twoCellAnchor"),
                [_, anchor] if anchor == b"xdr:oneCellAnchor" => {
                    ONE_CELL_CHILDREN.contains(&name.as_slice())
                }
                [_, _] => TWO_CELL_CHILDREN.contains(&name.as_slice()),
                [.., parent] if parent == b"xdr:grpSp" => GROUP_CHILDREN.contains(&name.as_slice()),
                _ => true,
            };
        if !allowed || (open.len() == 2 && name == b"xdr:graphicFrame" && empty) {
            return Err(refusal(&name));
        }
        if open.len() == 2 && name == b"xdr:graphicFrame" {
            frame_chart = Some(false);
        }
        if name == b"c:chart" && empty && frame_chart.is_some() {
            frame_chart = Some(reader_relationship_id(&element).is_some());
        }
        for attribute in element.attributes().with_checks(false).flatten() {
            if attribute.key.prefix().is_some()
                && matches!(
                    attribute.key.local_name().as_ref(),
                    b"id" | b"embed" | b"link"
                )
            {
                referenced.insert(String::from_utf8_lossy(&attribute.value).into_owned());
            }
        }
        if !empty {
            open.push(name);
        }
    }
    for relationship in relationships {
        if !relationship.external && !referenced.contains(&relationship.id) {
            return Err(HierarchicalUpdateRefusal::UnreferencedRelationship {
                part: relationships_part.map_or_else(String::new, |part| part.name.clone()),
                id: relationship.id.clone(),
            }
            .into());
        }
    }
    Ok(())
}

/// The plot-area chart types the chart reader models. A drawing anchor whose
/// chart has none of them is dropped, and any other chart type (stock or
/// surface charts) is dropped from a combined chart.
const CHART_TYPES: &[&[u8]] = &[
    b"c:lineChart",
    b"c:line3DChart",
    b"c:pieChart",
    b"c:pie3DChart",
    b"c:doughnutChart",
    b"c:scatterChart",
    b"c:barChart",
    b"c:bar3DChart",
    b"c:radarChart",
    b"c:bubbleChart",
    b"c:areaChart",
    b"c:area3DChart",
    b"c:ofPieChart",
];
/// The other plot-area, chart, and chart-space children the reader models.
const PLOT_AREA_CHILDREN: &[&[u8]] = &[
    b"c:layout",
    b"c:catAx",
    b"c:dateAx",
    b"c:valAx",
    b"c:serAx",
    b"c:spPr",
];
const CHART_CHILDREN: &[&[u8]] = &[
    b"c:title",
    b"c:view3D",
    b"c:floor",
    b"c:sideWall",
    b"c:backWall",
    b"c:plotArea",
    b"c:legend",
    b"c:autoTitleDeleted",
    b"c:plotVisOnly",
    b"c:dispBlanksAs",
    b"c:showDLblsOverMax",
];
const CHART_SPACE_CHILDREN: &[&[u8]] = &[
    b"mc:AlternateContent",
    b"c:chart",
    b"c:printSettings",
    b"c:spPr",
    b"c:date1904",
    b"c:lang",
    b"c:roundedCorners",
    b"c:style",
];

/// Refuses charts the chart reader would not rebuild: the chart is written
/// back from its model, so the chart space, chart, and plot area may hold
/// only what it models, including at least one modeled chart type.
fn check_chart(chart: &Part) -> Result<(), HierarchicalUpdateError> {
    let refusal = |element: &[u8]| -> HierarchicalUpdateError {
        HierarchicalUpdateRefusal::UnsupportedChart {
            part: chart.name.clone(),
            element: String::from_utf8_lossy(element).into_owned(),
        }
        .into()
    };
    let invalid = |error: &dyn std::fmt::Display| {
        XlsxFormatError::Update(format!("`{}` is not well-formed XML: {error}", chart.name))
    };
    let mut reader = quick_xml::Reader::from_reader(chart.bytes.as_slice());
    let mut open = Vec::<Vec<u8>>::new();
    let mut chart_types = 0_usize;
    loop {
        let (element, empty) = match reader.read_event().map_err(|error| invalid(&error))? {
            Event::Start(element) => (element, false),
            Event::Empty(element) => (element, true),
            Event::End(_) => {
                open.pop();
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let name = element.name().as_ref().to_vec();
        let allowed = match open.as_slice() {
            [] => name == b"c:chartSpace",
            [_] => CHART_SPACE_CHILDREN.contains(&name.as_slice()),
            [_, parent] if parent == b"c:chart" => CHART_CHILDREN.contains(&name.as_slice()),
            [_, _, parent] if parent == b"c:plotArea" => {
                let chart_type = CHART_TYPES.contains(&name.as_slice());
                chart_types += usize::from(chart_type);
                chart_type || PLOT_AREA_CHILDREN.contains(&name.as_slice())
            }
            _ => true,
        };
        if !allowed {
            return Err(refusal(&name));
        }
        if !empty {
            open.push(name);
        }
    }
    if chart_types == 0 {
        return Err(refusal(b"c:plotArea"));
    }
    Ok(())
}

fn office_relationship(kind: &str) -> String {
    format!("{OFFICE_RELATIONSHIP}{kind}")
}

/// Walks the relationship graph as the workbook reader loads it.
struct Walk<'a> {
    parts: &'a Parts,
    limits: &'a XlsxLimitOptions,
    budget: XmlBudget<'a>,
    /// Bytes the reader loads: each part once per relationship that reaches
    /// it, since the reader keeps a separate copy for each.
    loaded: usize,
    /// The relationship ids the workbook's sheets name.
    sheet_ids: BTreeSet<String>,
}

impl Walk<'_> {
    /// Checks the relationships of `part` (the case-preserved resolved name;
    /// empty for the package) and of every part they reach. `path` holds the
    /// lowercased parts being walked; `prefixes` the relationship prefixes
    /// already used below the current worksheet.
    fn part(
        &mut self,
        path: &mut Vec<String>,
        part: &str,
        role: Role,
        prefixes: &mut BTreeSet<&'static str>,
    ) -> Result<(), HierarchicalUpdateError> {
        let (directory, file) = part.rsplit_once('/').unwrap_or(("", part));
        let relationships_name = if directory.is_empty() {
            format!("_rels/{file}.rels")
        } else {
            format!("{directory}/_rels/{file}.rels")
        };
        let parts = self.parts;
        let relationships_part = parts.get(&relationships_name.to_ascii_lowercase());
        let relationships = match relationships_part {
            Some(relationships_part) => {
                let loadable = role.relationships().is_some_and(|prefix| {
                    prefix.is_none_or(|prefix| {
                        relationships_name.starts_with(prefix) && prefixes.insert(prefix)
                    })
                });
                if !loadable {
                    return Err(HierarchicalUpdateRefusal::UnloadableRelationships {
                        part: relationships_part.name.clone(),
                    }
                    .into());
                }
                add(
                    &mut self.loaded,
                    relationships_part.bytes.len(),
                    XlsxLimit::ExpandedBytes,
                    self.limits.max_expanded_bytes,
                )?;
                relationships(&relationships_part.name, &relationships_part.bytes)?
            }
            None => Vec::new(),
        };
        for relationship in &relationships {
            // The reader builds one relationship per element and load.
            add(
                &mut self.budget.nodes,
                1,
                XlsxLimit::XmlNodes,
                self.limits.max_xml_nodes,
            )?;
            if role.related(&relationship.kind).is_none() {
                return Err(HierarchicalUpdateRefusal::UnsupportedPart {
                    part: relationships_part.map_or_else(String::new, |part| part.name.clone()),
                    relationship: relationship.kind.clone(),
                }
                .into());
            }
        }
        let has_comments = match role {
            Role::Workbook => {
                check_workbook_references(relationships_part, &relationships, &self.sheet_ids)?;
                false
            }
            Role::Worksheet => {
                if let Some(sheet) = parts.get(&part.to_ascii_lowercase()) {
                    check_worksheet_references(sheet, relationships_part, &relationships)?;
                }
                let comments = office_relationship("comments");
                relationships
                    .iter()
                    .any(|relationship| relationship.kind == comments && !relationship.external)
            }
            Role::Drawing => {
                if let Some(drawing) = parts.get(&part.to_ascii_lowercase()) {
                    check_drawing(drawing, relationships_part, &relationships)?;
                }
                false
            }
            _ => false,
        };
        let Some(relationships_part) = relationships_part else {
            return Ok(());
        };
        path.push(part.to_ascii_lowercase());
        for relationship in &relationships {
            let unsupported = || HierarchicalUpdateRefusal::UnsupportedPart {
                part: relationships_part.name.clone(),
                relationship: relationship.kind.clone(),
            };
            let (related, fixed) = role.related(&relationship.kind).ok_or_else(unsupported)?;
            if relationship.external {
                continue;
            }
            let invalid = |reason: &str| {
                XlsxFormatError::Update(format!(
                    "relationship target `{}` in `{}` {reason}",
                    relationship.target, relationships_part.name
                ))
            };
            // The reader splits on `\` only on Windows.
            if relationship.target.contains('\\') {
                return Err(invalid("contains a backslash").into());
            }
            let target = join_part(directory, &relationship.target);
            let key = target.to_ascii_lowercase();
            let Some(target_part) = parts.get(&key) else {
                return Err(invalid("does not name a package part").into());
            };
            // The reader follows every relationship without a visited set.
            if path.contains(&key) {
                return Err(HierarchicalUpdateRefusal::RelationshipCycle {
                    part: target_part.name.clone(),
                }
                .into());
            }
            if fixed.is_some_and(|fixed| fixed != key) {
                return Err(unsupported().into());
            }
            add(
                &mut self.loaded,
                target_part.bytes.len(),
                XlsxLimit::ExpandedBytes,
                self.limits.max_expanded_bytes,
            )?;
            // Parts are parsed by the role that reaches them, whatever their
            // names, so they are checked before anything else reads them.
            if let Some(lenient) = related.xml() {
                self.budget.validate(&key, target_part, lenient)?;
            }
            match related {
                // Walked once, from the name the reader loads.
                Role::Workbook => {}
                Role::Worksheet => self.part(path, &target, related, &mut BTreeSet::new())?,
                Role::VmlDrawing => {
                    check_vml_drawing(target_part, has_comments)?;
                    self.part(path, &target, related, prefixes)?;
                }
                _ if matches!(role, Role::Drawing)
                    && relationship.kind == office_relationship("chart") =>
                {
                    check_chart(target_part)?;
                    self.part(path, &target, related, prefixes)?;
                }
                _ => self.part(path, &target, related, prefixes)?,
            }
        }
        path.pop();
        Ok(())
    }
}

struct Relationship {
    id: String,
    kind: String,
    target: String,
    external: bool,
}

/// Parses relationships with the attribute values exactly as the reader
/// takes them: raw, without unescaping. Ids must be unique, since the reader
/// resolves references by the first or last match depending on the part.
fn relationships(part: &str, xml: &[u8]) -> Result<Vec<Relationship>, XlsxFormatError> {
    let invalid = |reason: &dyn std::fmt::Display| {
        XlsxFormatError::Update(format!("invalid relationships in `{part}`: {reason}"))
    };
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut relationships = Vec::new();
    let mut ids = BTreeSet::new();
    loop {
        let (element, empty) = match reader.read_event().map_err(|error| invalid(&error))? {
            Event::Start(element) => (element, false),
            Event::Empty(element) => (element, true),
            Event::Eof => return Ok(relationships),
            _ => continue,
        };
        if element.local_name().as_ref() != b"Relationship" {
            continue;
        }
        // The reader ignores non-empty and prefixed elements, which would
        // drop their parts.
        if !empty || element.name().as_ref() != b"Relationship" {
            return Err(invalid(
                &"Relationship elements must be empty and unprefixed",
            ));
        }
        let mut values = BTreeMap::new();
        for attribute in element.attributes() {
            let attribute = attribute.map_err(|error| invalid(&error))?;
            let value = std::str::from_utf8(&attribute.value).map_err(|error| invalid(&error))?;
            // The reader keeps references escaped and its writer escapes the
            // value again, so neither resolution nor round trip would match.
            if value.contains('&') {
                return Err(invalid(
                    &"escaped Relationship attribute values are not supported",
                ));
            }
            values.insert(attribute.key.as_ref().to_vec(), value.to_string());
        }
        let mut take = |key: &[u8]| {
            values
                .remove(key)
                .ok_or_else(|| invalid(&"a Relationship lacks Id, Type, or Target"))
        };
        let id = take(b"Id")?;
        let kind = take(b"Type")?;
        let target = take(b"Target")?;
        let external = values.get(b"TargetMode".as_slice()).map(String::as_str) == Some("External");
        if !ids.insert(id.clone()) {
            return Err(invalid(&format_args!("relationship id `{id}` is repeated")));
        }
        relationships.push(Relationship {
            id,
            kind,
            target,
            external,
        });
    }
}

/// Resolves a relationship target against its source directory, as the
/// workbook reader does, preserving case: a leading `/` is package-absolute,
/// `.` and empty segments vanish, and `..` removes one segment.
fn join_part(directory: &str, target: &str) -> String {
    let joined = match target.strip_prefix('/') {
        Some(absolute) => absolute.to_string(),
        None => format!("{directory}/{target}"),
    };
    let mut segments = Vec::new();
    for segment in joined.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            segment => segments.push(segment),
        }
    }
    segments.join("/")
}
