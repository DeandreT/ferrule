use std::fmt::Write as _;
use std::path::Path;

use ir::SchemaNode;
use mapping::{
    FormatOptions, PdfCapture, PdfCaptureAlgorithm, PdfCommand, PdfCoordinate, PdfLayout,
    PdfPageSelection, PdfReference, PdfRegion, PdfWhitespaceMode, PdfWordSeparation,
};

use crate::MfdError;

use super::schema::{GeneratedSibling, PortTree, RenderedSchemaComponent, Side, xml_escape};

mod native_anchor_body;
mod native_anchor_rows;
mod native_merge;
mod native_rows;

const MAX_PXT_BYTES: usize = 1024 * 1024;

pub(super) struct RenderArgs<'a> {
    pub(super) schema: &'a SchemaNode,
    pub(super) ports: &'a PortTree,
    pub(super) side: Side,
    pub(super) instance_path: Option<&'a str>,
    pub(super) options: &'a FormatOptions,
    pub(super) mfd_path: &'a Path,
    pub(super) component_name: &'a str,
    pub(super) component_uid: u32,
    pub(super) sibling_suffix: &'a str,
    pub(super) force_root_port: bool,
}

pub(super) fn validate_side(
    schema: &SchemaNode,
    options: &FormatOptions,
    side: Side,
    side_name: &str,
) -> Result<(), MfdError> {
    if let Some(dependency) = options.pdf.as_ref().and_then(PdfLayout::repair_dependency) {
        return Err(unsupported(format!(
            "the {side_name} PDF boundary requires {dependency}"
        )));
    }
    let Some(layout) = options.pdf.as_ref() else {
        return Ok(());
    };
    if side != Side::Source {
        return Err(unsupported(format!(
            "the {side_name} PDF boundary is input-only"
        )));
    }
    if has_conflicting_options(options) {
        return Err(unsupported(format!(
            "the {side_name} PDF boundary conflicts with another format's options"
        )));
    }
    if layout.schema() != *schema {
        return Err(unsupported(format!(
            "the {side_name} schema does not exactly match its embedded PDF layout"
        )));
    }
    Ok(())
}

pub(super) fn render(args: RenderArgs<'_>) -> Result<RenderedSchemaComponent, MfdError> {
    validate_side(args.schema, args.options, args.side, "mapping side")?;
    let layout = args.options.pdf.as_ref().ok_or_else(|| {
        unsupported("internal PDF export is missing its visual extraction layout")
    })?;
    let instance_path = args
        .instance_path
        .ok_or_else(|| unsupported("a PDF source requires its input instance path"))?;
    let stem = args
        .mfd_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("mapping");
    let template_file = format!("{stem}-{}.pxt", args.sibling_suffix);
    let template_path = args
        .mfd_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&template_file);
    let template = match native_capture_template(layout)
        .or_else(|| native_rows::template(layout))
        .or_else(|| native_merge::template(layout))
        .or_else(|| native_anchor_rows::template(layout))
        .or_else(|| native_anchor_body::template(layout))
    {
        Some(template) => template,
        None => canonical_template(layout)?,
    };
    let entries =
        args.ports
            .entries_xml(args.schema, "outkey", 10, args.force_root_port, None, None);

    let mut xml = String::new();
    let _ = write!(
        xml,
        "\t\t\t\t<component name=\"{}\" library=\"pdf\" uid=\"{}\" kind=\"34\">\n\
         \t\t\t\t\t<view rbx=\"300\" rby=\"400\"/>\n\
         \t\t\t\t\t<data>\n\
         \t\t\t\t\t\t<root>\n\
         \t\t\t\t\t\t\t<header><namespaces><namespace/></namespaces></header>\n\
         \t\t\t\t\t\t\t<entry name=\"FileInstance\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t<file role=\"inputinstance\" name=\"{}\"/>\n\
         \t\t\t\t\t\t\t\t<entry name=\"document\" type=\"doc-pdf\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t\t<document schemafile=\"{}\" root=\"{}\"/>\n\
         {entries}\
         \t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t</root>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n",
        xml_escape(args.component_name),
        args.component_uid,
        xml_escape(instance_path),
        xml_escape(&template_file),
        xml_escape(layout.root_name()),
    );
    Ok(RenderedSchemaComponent {
        xml,
        siblings: vec![GeneratedSibling {
            path: template_path,
            contents: template,
        }],
    })
}

/// The simple native-shaped template families: all pages with direct text
/// captures, or one named page group containing only direct text captures.
/// The bounded vertical-boundary/edge-row family lives in `native_rows`.
fn native_capture_template(layout: &PdfLayout) -> Option<String> {
    if layout.page_selection() != PdfPageSelection::All
        || layout.commands().is_empty()
        || !native_label(layout.root_name())
    {
        return None;
    }
    let (captures, group) = match layout.commands() {
        [PdfCommand::GroupPerPage(group)] if native_label(&group.name) => {
            (group.children.as_slice(), Some(group))
        }
        commands => (commands, None),
    };
    let group_region = match group {
        Some(group) => Some(native_region(&group.region)?),
        None => None,
    };
    let mut template = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <Document>\n\
         \t<Template version=\"1\">\n\
         \t\t<Model>\n\
         \t\t\t<Root id=\"1\">\n\
         \t\t\t\t<Children>\n\
         \t\t\t\t\t<Grouping id=\"2\">\n\
         \t\t\t\t\t\t<Children>\n",
    );
    for (index, command) in captures.iter().enumerate() {
        let PdfCommand::Capture(capture) = command else {
            return None;
        };
        if !native_label(&capture.name)
            || !matches!(
                capture.algorithm,
                PdfCaptureAlgorithm::BasicVisual {
                    separate_words: PdfWordSeparation::InsertSpace,
                    whitespace: PdfWhitespaceMode::Default
                }
            )
        {
            return None;
        }
        let region = native_region(&capture.region)?;
        let _ = write!(
            template,
            "\t\t\t\t\t\t\t<Capture id=\"{}\">\n\
             \t\t\t\t\t\t\t\t<Label>{}</Label>\n\
             \t\t\t\t\t\t\t\t<Region>{}</Region>\n\
             \t\t\t\t\t\t\t\t<Algorithm><BasicVisual><BaselineCapture/><ParagraphSpacing/><BaselineAngle/><AngleDeviation/><SeparateWords><InsertSpace/></SeparateWords><WhitespaceMode><Default/></WhitespaceMode></BasicVisual></Algorithm>\n\
             \t\t\t\t\t\t\t</Capture>\n",
            index + 3,
            xml_escape(&capture.name),
            xml_escape(&region),
        );
        if template.len() > MAX_PXT_BYTES {
            return None;
        }
    }
    template.push_str("\t\t\t\t\t\t</Children>\n");
    if let Some(group) = group {
        let _ = write!(
            template,
            "\t\t\t\t\t\t<Label>{}</Label>\n\
             \t\t\t\t\t\t<Region>{}</Region>\n",
            xml_escape(&group.name),
            xml_escape(group_region.as_deref()?),
        );
    } else {
        template.push_str("\t\t\t\t\t\t<Label/>\n");
    }
    let _ = write!(
        template,
        "\t\t\t\t\t\t<Kind><OneGroupPerPage/></Kind>\n\
         \t\t\t\t\t\t<Filter/>\n\
         \t\t\t\t\t</Grouping>\n\
         \t\t\t\t</Children>\n\
         \t\t\t\t<Label>{}</Label>\n\
         \t\t\t</Root>\n\
         \t\t</Model>\n\
         \t</Template>\n\
         </Document>\n",
        xml_escape(layout.root_name()),
    );
    if template.len() > MAX_PXT_BYTES {
        return None;
    }
    let parsed =
        crate::import::parse_native_pdf_template_text(&template, layout.root_name()).ok()?;
    same_capture_layout(layout, &parsed).then_some(template)
}

fn native_label(name: &str) -> bool {
    !name.is_empty() && name.trim() == name
}

fn native_region(region: &PdfRegion) -> Option<String> {
    Some(format!(
        "{{ Left: {}, Top: {}, Right: {}, Bottom: {} }}",
        native_coordinate(&region.left)?,
        native_coordinate(&region.top)?,
        native_coordinate(&region.right)?,
        native_coordinate(&region.bottom)?,
    ))
}

fn native_coordinate(coordinate: &PdfCoordinate) -> Option<String> {
    let reference = match coordinate.reference {
        PdfReference::Left => "Left",
        PdfReference::Top => "Top",
        PdfReference::Right => "Right",
        PdfReference::Bottom => "Bottom",
        PdfReference::Anchor(_) => return None,
    };
    if !coordinate.offset.is_finite() {
        return None;
    }
    let sign = if coordinate.offset.is_sign_negative() {
        '-'
    } else {
        '+'
    };
    Some(format!("{reference} {sign} {}pt", coordinate.offset.abs()))
}

fn same_capture_layout(expected: &PdfLayout, actual: &PdfLayout) -> bool {
    expected.root_name() == actual.root_name()
        && expected.page_selection() == actual.page_selection()
        && match (expected.commands(), actual.commands()) {
            ([PdfCommand::GroupPerPage(expected)], [PdfCommand::GroupPerPage(actual)]) => {
                expected.name == actual.name
                    && same_region(&expected.region, &actual.region)
                    && same_capture_commands(&expected.children, &actual.children)
            }
            (expected, actual) => same_capture_commands(expected, actual),
        }
}

fn same_capture_commands(expected: &[PdfCommand], actual: &[PdfCommand]) -> bool {
    expected.len() == actual.len()
        && expected
            .iter()
            .zip(actual)
            .all(|(expected, actual)| match (expected, actual) {
                (PdfCommand::Capture(expected), PdfCommand::Capture(actual)) => {
                    same_capture(expected, actual)
                }
                _ => false,
            })
}

fn same_capture(expected: &PdfCapture, actual: &PdfCapture) -> bool {
    expected.name == actual.name
        && expected.algorithm == actual.algorithm
        && same_region(&expected.region, &actual.region)
}

fn same_region(expected: &PdfRegion, actual: &PdfRegion) -> bool {
    [
        (&expected.left, &actual.left),
        (&expected.top, &actual.top),
        (&expected.right, &actual.right),
        (&expected.bottom, &actual.bottom),
    ]
    .into_iter()
    .all(|(expected, actual)| {
        expected.reference == actual.reference
            && expected.offset.to_bits() == actual.offset.to_bits()
    })
}

fn canonical_template(layout: &PdfLayout) -> Result<String, MfdError> {
    let encoded = mapping::pdf_layout_file::encode_pretty(layout).map_err(|error| {
        unsupported(format!(
            "could not encode the retained PDF layout ({error})"
        ))
    })?;
    if encoded.len() > MAX_PXT_BYTES {
        return Err(unsupported(format!(
            "PDF template exceeds the {MAX_PXT_BYTES}-byte limit"
        )));
    }
    let document: serde_json::Value = serde_json::from_str(&encoded).map_err(|error| {
        unsupported(format!(
            "could not inspect the retained PDF layout ({error})"
        ))
    })?;
    let version = if document.get("__ferrule_file").is_some() {
        2
    } else {
        1
    };
    let template = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <Document>\n\
         \t<FerruleLayout version=\"{version}\">{}</FerruleLayout>\n\
         </Document>\n",
        xml_escape(&encoded)
    );
    if template.len() > MAX_PXT_BYTES {
        return Err(unsupported(format!(
            "PDF template exceeds the {MAX_PXT_BYTES}-byte limit"
        )));
    }
    roxmltree::Document::parse(&template).map_err(|error| {
        unsupported(format!(
            "could not render the retained PDF layout ({error})"
        ))
    })?;
    Ok(template)
}

fn has_conflicting_options(options: &FormatOptions) -> bool {
    options.lenient_segments
        || options.edi_kind.is_some()
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.delimiter.is_some()
        || options.csv_quote.is_some()
        || options.csv_quote_disabled
        || options.csv_utf8_bom
        || options.csv_preserve_empty_strings
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.xml_document
        || options.local_xml_file_set
        || options.json_document
        || options.json_lines
        || options.protobuf.is_some()
        || options.xbrl.is_some()
        || options.xlsx_sheet.is_some()
        || options.xlsx_start_row.is_some()
        || !options.xlsx_columns.is_empty()
        || !options.xlsx_headers.is_empty()
        || options.xlsx_update_existing
        || !options.xlsx_rows.is_empty()
        || options.xlsx_composite.is_some()
        || options.xlsx_worksheet_set.is_some()
        || options.xlsx_grid.is_some()
        || options.xlsx_hierarchical.is_some()
}

fn unsupported(message: impl Into<String>) -> MfdError {
    MfdError::Unsupported(message.into())
}

#[cfg(test)]
mod tests {
    use mapping::{PdfCapture, PdfCommand, PdfLayout, PdfPageSelection, PdfRegion};

    use super::{canonical_template, native_capture_template};

    #[test]
    fn oversized_capture_template_cannot_bypass_the_pxt_file_limit() {
        let label = "x".repeat(3_300);
        let commands = (0..300)
            .map(|index| {
                PdfCommand::Capture(PdfCapture {
                    name: format!("Field{index}{label}"),
                    region: PdfRegion::full(),
                    algorithm: Default::default(),
                })
            })
            .collect();
        let layout = PdfLayout::new("Document", PdfPageSelection::All, commands).unwrap();
        assert!(native_capture_template(&layout).is_none());
        let error = canonical_template(&layout).unwrap_err();
        assert!(error.to_string().contains("1048576-byte limit"), "{error}");
    }
}
