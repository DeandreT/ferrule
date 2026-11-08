//! Internal boundary identity and conflict checking. Route admission, metadata
//! prerequisites and codec work deliberately stay at their original call sites.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Direction {
    Input,
    Output,
}

impl Direction {
    fn label(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Origin {
    Explicit,
    Fallback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum JsonMode {
    Document,
    Json5,
    Lines,
}

/// Only codec identity is selected here: inferred EDI dialects, layout parsing,
/// acquisition and stateful/input-only restrictions remain route-local.
pub(super) enum SelectedFormat<'a> {
    Xbrl(&'a mapping::XbrlBoundaryOptions),
    Idoc(&'a mapping::IdocLayout),
    Swift(&'a mapping::SwiftMtLayout),
    External(&'a mapping::ExternalSourceOptions),
    Pdf(&'a mapping::PdfLayout),
    FlexText(&'a mapping::FlexTextLayout),
    Protobuf(&'a ProtobufOptions),
    Edi(Option<EdiBoundaryKind>),
    Xml(Origin),
    Json(JsonMode, Origin),
    HttpXml(&'a str),
    FixedWidth(&'a mapping::FixedWidthLayout),
    Csv,
    Xlsx,
    Sqlite,
    MissingIdoc,
    MissingSwift,
    MissingPdf,
    Unsupported(String),
}

enum Declaration<'a> {
    Selected(SelectedFormat<'a>),
    Json,
}

// Preview checks declarations before it has a destination identity. Its legacy
// input-only target check includes captured sources; ordinary output dispatch
// never selects them. Fixed-width is checked after the filesystem HTTP fallback.
fn declaration(
    options: &FormatOptions,
    direction: Direction,
    captured: bool,
) -> anyhow::Result<Option<Declaration<'_>>> {
    if let Some(dependency) = options.json5_interchange_blocker() {
        return Err(dependency.clone().into());
    }
    let side = direction.label();
    let selected = if let Some(value) = &options.xbrl {
        reject_xbrl_conflicts(options, side)?;
        SelectedFormat::Xbrl(value)
    } else if let Some(value) = &options.idoc {
        reject_idoc_conflicts(options, side)?;
        SelectedFormat::Idoc(value)
    } else if let Some(value) = &options.swift_mt {
        reject_swift_conflicts(options, side)?;
        SelectedFormat::Swift(value)
    } else if let Some(value) = options.external_source.as_ref().filter(|_| captured) {
        reject_external_source_conflicts(options, side)?;
        SelectedFormat::External(value)
    } else if let Some(value) = &options.pdf {
        reject_pdf_conflicts(options, side)?;
        SelectedFormat::Pdf(value)
    } else if let Some(value) = &options.flextext {
        reject_flextext_conflicts(options, side)?;
        SelectedFormat::FlexText(value)
    } else if let Some(value) = &options.protobuf {
        reject_protobuf_conflicts(options, side)?;
        SelectedFormat::Protobuf(value)
    } else if let Some(value) = options.edi_kind {
        reject_edi_conflicts(options, side)?;
        SelectedFormat::Edi(Some(value))
    } else if options.xml_document {
        reject_xml_conflicts(options, side)?;
        SelectedFormat::Xml(Origin::Explicit)
    } else if options.json_document || options.json5 || options.json_lines {
        reject_json_conflicts(options, side)?;
        return Ok(Some(Declaration::Json));
    } else {
        return Ok(None);
    };
    Ok(Some(Declaration::Selected(selected)))
}

pub(super) fn validate_declared(
    options: &FormatOptions,
    direction: Direction,
) -> anyhow::Result<bool> {
    if declaration(options, direction, true)?.is_some() {
        return Ok(true);
    }
    if options.fixed_width.is_some() {
        reject_fixed_width_csv_options(options, direction.label())?;
        return Ok(true);
    }
    Ok(false)
}

pub(super) fn select<'a>(
    path: &'a Path,
    options: &'a FormatOptions,
    direction: Direction,
    filesystem_input: bool,
) -> anyhow::Result<SelectedFormat<'a>> {
    if let Some(declaration) = declaration(options, direction, direction == Direction::Input)? {
        return match declaration {
            Declaration::Selected(selected) => Ok(selected),
            Declaration::Json => Ok(SelectedFormat::Json(
                json_mode(path, options, Origin::Explicit)?,
                Origin::Explicit,
            )),
        };
    }
    if filesystem_input
        && direction == Direction::Input
        && let Some(url) = http_url(path)
    {
        return Ok(SelectedFormat::HttpXml(url));
    }
    if let Some(layout) = &options.fixed_width {
        reject_fixed_width_csv_options(options, direction.label())?;
        return Ok(SelectedFormat::FixedWidth(layout));
    }
    validate_tabular_fallback(path, options, direction.label())?;
    Ok(match extension_for_dispatch(path, options)?.as_str() {
        "csv" | "txt" => SelectedFormat::Csv,
        "xlsx" => SelectedFormat::Xlsx,
        "xml" => SelectedFormat::Xml(Origin::Fallback),
        "json" | "json5" | "jsonl" | "ndjson" => SelectedFormat::Json(
            json_mode(path, options, Origin::Fallback)?,
            Origin::Fallback,
        ),
        "db" | "sqlite" | "sqlite3" => SelectedFormat::Sqlite,
        "edi" | "x12" | "edifact" | "hl7" => SelectedFormat::Edi(None),
        "idoc" => SelectedFormat::MissingIdoc,
        "fin" | "swift" => SelectedFormat::MissingSwift,
        "pdf" => SelectedFormat::MissingPdf,
        other => SelectedFormat::Unsupported(other.to_owned()),
    })
}

/// Preview historically checks JSON flags even on a captured JSON source,
/// whose runtime reader uses the captured-response codec unconditionally.
pub(super) fn select_preview<'a>(
    path: &'a Path,
    options: &'a FormatOptions,
    direction: Direction,
) -> anyhow::Result<SelectedFormat<'a>> {
    let selected = select(path, options, direction, false)?;
    if options.json_document || options.json5 || options.json_lines {
        json5_selected(path, options)?;
    }
    Ok(selected)
}

fn json_mode(path: &Path, options: &FormatOptions, origin: Origin) -> anyhow::Result<JsonMode> {
    let json5 = json5_selected(path, options)?;
    let lines = options.json_lines
        || (origin == Origin::Fallback
            && matches!(extension_of(path)?.as_str(), "jsonl" | "ndjson"));
    if lines && json5 {
        bail!("JSON5 cannot be combined with JSON Lines");
    }
    Ok(if lines {
        JsonMode::Lines
    } else if json5 {
        JsonMode::Json5
    } else {
        JsonMode::Document
    })
}

pub(super) fn validate_csv_metadata_identity(
    path: &Path,
    options: &FormatOptions,
    side: &str,
) -> anyhow::Result<()> {
    format_csv::require_executable_dependency(options.csv_text_repair_dependency)?;
    if options.csv_preserve_empty_strings
        && !matches!(
            extension_for_dispatch(path, options)?.as_str(),
            "csv" | "txt"
        )
    {
        bail!("`csv_preserve_empty_strings` requires a CSV {side} boundary");
    }
    if options.csv_utf8_bom
        && !matches!(
            extension_for_dispatch(path, options)?.as_str(),
            "csv" | "txt"
        )
    {
        bail!("`csv_utf8_bom` requires a CSV {side} boundary");
    }
    Ok(())
}

fn reject_fixed_width_csv_options(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
    if options.delimiter.is_some()
        || options.csv_quote.is_some()
        || options.csv_quote_disabled
        || options.csv_utf8_bom
        || options.csv_preserve_empty_strings
        || options.has_header_row.is_some()
        || options.xml_document
        || options.local_xml_file_set
    {
        bail!(
            "`fixed_width` cannot be combined with `delimiter`, `csv_quote`, `has_header_row`, \
             `xml_document`, or `local_xml_file_set` for {side}"
        );
    }
    Ok(())
}

fn reject_idoc_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
    if options
        .edi_kind
        .is_some_and(|kind| kind != EdiBoundaryKind::Idoc)
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
        || options.json5
        || options.json_lines
        || options.pdf.is_some()
        || options.protobuf.is_some()
        || options.xbrl.is_some()
        || options.swift_mt.is_some()
        || has_any_xlsx_layout(options)
    {
        bail!("`idoc` cannot be combined with another format's options for {side}");
    }
    Ok(())
}

pub(super) fn reject_unsupported_edi_source_constraints(
    options: &FormatOptions,
) -> anyhow::Result<()> {
    if (options.swift_mt.is_some() || options.edi_kind == Some(EdiBoundaryKind::SwiftMt))
        && !options.edi_value_constraints.is_empty()
    {
        bail!(
            "SWIFT MT input uses its embedded field grammar and does not support `edi_value_constraints`"
        );
    }
    Ok(())
}

fn reject_swift_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
    if side == "input" {
        reject_unsupported_edi_source_constraints(options)?;
    }
    if options
        .edi_kind
        .is_some_and(|kind| kind != EdiBoundaryKind::SwiftMt)
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
        || options.idoc.is_some()
        || options.xml_document
        || options.local_xml_file_set
        || options.json_document
        || options.json5
        || options.json_lines
        || options.pdf.is_some()
        || options.protobuf.is_some()
        || options.xbrl.is_some()
        || has_any_xlsx_layout(options)
    {
        bail!("`swift_mt` cannot be combined with another format's options for {side}");
    }
    Ok(())
}

fn reject_xbrl_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
    if options.lenient_segments
        || options.edi_kind.is_some()
        || options.delimiter.is_some()
        || options.csv_quote.is_some()
        || options.csv_quote_disabled
        || options.csv_utf8_bom
        || options.csv_preserve_empty_strings
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.xml_document
        || options.local_xml_file_set
        || options.json_document
        || options.json5
        || options.json_lines
        || options.pdf.is_some()
        || options.protobuf.is_some()
        || has_any_xlsx_layout(options)
    {
        bail!("`xbrl` cannot be combined with another format's options for {side}");
    }
    Ok(())
}

fn reject_protobuf_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
    if options.lenient_segments
        || options.edi_kind.is_some()
        || options.delimiter.is_some()
        || options.csv_quote.is_some()
        || options.csv_quote_disabled
        || options.csv_utf8_bom
        || options.csv_preserve_empty_strings
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.xml_document
        || options.local_xml_file_set
        || options.json_document
        || options.json5
        || options.json_lines
        || options.pdf.is_some()
        || options.xbrl.is_some()
        || has_any_xlsx_layout(options)
    {
        bail!("`protobuf` cannot be combined with another format's options for {side}");
    }
    Ok(())
}

fn reject_flextext_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
    if options.lenient_segments
        || options.edi_kind.is_some()
        || options.delimiter.is_some()
        || options.csv_quote.is_some()
        || options.csv_quote_disabled
        || options.csv_utf8_bom
        || options.csv_preserve_empty_strings
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.xml_document
        || options.local_xml_file_set
        || options.json_document
        || options.json5
        || options.json_lines
        || options.pdf.is_some()
        || options.protobuf.is_some()
        || options.xbrl.is_some()
        || has_any_xlsx_layout(options)
    {
        bail!("`flextext` cannot be combined with another format's options for {side}");
    }
    Ok(())
}

fn reject_pdf_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
    if options.lenient_segments
        || options.edi_kind.is_some()
        || options.delimiter.is_some()
        || options.csv_quote.is_some()
        || options.csv_quote_disabled
        || options.csv_utf8_bom
        || options.csv_preserve_empty_strings
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.xml_document
        || options.local_xml_file_set
        || options.json_document
        || options.json5
        || options.json_lines
        || options.protobuf.is_some()
        || options.xbrl.is_some()
        || has_any_xlsx_layout(options)
    {
        bail!("`pdf` cannot be combined with another format's options for {side}");
    }
    Ok(())
}

fn reject_edi_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
    match options.edi_config_reference.as_ref() {
        Some(EdiConfigDependency::ExternalReference(reference)) => {
            bail!(
                "{side} requires unresolved external EDI configuration `{reference}`; \
                 import from a package containing the exact configuration or a matching release pack"
            );
        }
        Some(EdiConfigDependency::MissingConfiguration) => {
            bail!(
                "{side} requires an EDI configuration because its imported entry-tree schema is \
                 untyped; supply a complete EDI configuration and re-import the mapping"
            );
        }
        None => {}
    }
    let kind = options
        .edi_kind
        .context("missing EDI boundary kind during runtime dispatch")?;
    if matches!(kind, EdiBoundaryKind::Idoc | EdiBoundaryKind::SwiftMt) {
        bail!("`edi_kind` `{kind:?}` requires its embedded runtime layout for {side}");
    }
    if options.x12_separators.is_some() && kind != EdiBoundaryKind::X12 {
        bail!("X12 separator metadata requires `edi_kind` `X12` for {side}");
    }
    if options.x12_interchange_version.is_some() && kind != EdiBoundaryKind::X12 {
        bail!("X12 interchange version requires `edi_kind` `X12` for {side}");
    }
    if options.idoc.is_some()
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
        || options.json5
        || options.json_lines
        || options.pdf.is_some()
        || options.protobuf.is_some()
        || options.xbrl.is_some()
        || has_any_xlsx_layout(options)
    {
        bail!("`edi_kind` cannot be combined with another format's options for {side}");
    }
    Ok(())
}

fn reject_json_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
    if options.lenient_segments
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
        || options.pdf.is_some()
        || options.protobuf.is_some()
        || options.xbrl.is_some()
        || has_any_xlsx_layout(options)
    {
        bail!(
            "`json_document`/`json5`/`json_lines` cannot be combined with another format's options for {side}"
        );
    }
    Ok(())
}

pub(super) fn json5_selected(path: &Path, options: &FormatOptions) -> anyhow::Result<bool> {
    let selected = options.json5
        || path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json5"));
    if selected && options.json_lines {
        bail!("JSON5 cannot be combined with JSON Lines");
    }
    Ok(selected)
}

pub(super) fn reject_inactive_root_xml_read_options(
    path: &Path,
    schema: &SchemaNode,
    options: &FormatOptions,
    side: &str,
) -> anyhow::Result<()> {
    if !options.xml_allow_inactive_root_type_members && !options.xml_root_view_read_policy {
        return Ok(());
    }
    let accepted_options = FormatOptions {
        xml_document: true,
        xml_allow_inactive_root_type_members: true,
        xml_root_view_read_policy: options.xml_root_view_read_policy,
        ..Default::default()
    };
    if side != "input"
        || !options.xml_document
        || !options.xml_allow_inactive_root_type_members
        || *options != accepted_options
        || http_url(path).is_some()
        || !ir::xml_inactive_root_type_members_are_supported(schema)
        || (options.xml_root_view_read_policy
            && !ir::xml_root_view_read_policy_is_supported(schema))
    {
        bail!(
            "`xml_allow_inactive_root_type_members` requires a closed flat typed local XML input with no other format options"
        );
    }
    Ok(())
}

pub(super) fn xml_read_options(options: &FormatOptions) -> format_xml::XmlReadOptions {
    format_xml::XmlReadOptions {
        allow_inactive_root_type_members: options.xml_allow_inactive_root_type_members,
        root_view_policy: options.xml_root_view_read_policy,
    }
}

fn reject_xml_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
    let external_xml = options
        .external_source
        .as_ref()
        .is_some_and(|boundary| boundary.payload() == ExternalPayloadFormat::Xml);
    if options.lenient_segments
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
        || options.pdf.is_some()
        || options.json_document
        || options.json5
        || options.json_lines
        || options.protobuf.is_some()
        || options.xbrl.is_some()
        || has_any_xlsx_layout(options)
        || (options.external_source.is_some() && !external_xml)
        || (options.local_xml_file_set && side == "output")
        || (options.local_xml_file_set
            && (options.http_get.is_some() || options.external_source.is_some()))
        || (side == "output" && options.http_get.is_some())
        || (options.http_get.is_some() && options.external_source.is_some())
    {
        bail!("`xml_document` cannot be combined with another format's options for {side}");
    }
    Ok(())
}

fn reject_external_source_conflicts(options: &FormatOptions, side: &str) -> anyhow::Result<()> {
    let boundary = options
        .external_source
        .as_ref()
        .context("missing captured external source metadata")?;
    let owns_identity = match boundary.payload() {
        ExternalPayloadFormat::Json => !options.xml_document,
        ExternalPayloadFormat::Xml => {
            !options.json_document && !options.json5 && !options.json_lines
        }
    };
    if !owns_identity
        || options.lenient_segments
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
        || options.local_xml_file_set
        || options.pdf.is_some()
        || options.protobuf.is_some()
        || options.xbrl.is_some()
        || has_any_xlsx_layout(options)
    {
        bail!(
            "captured external source metadata conflicts with another format's options for {side}"
        );
    }
    Ok(())
}
