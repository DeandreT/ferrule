use std::path::Path;
use std::sync::LazyLock;

use anyhow::bail;
use mapping::{EdiAutocomplete, EdiBoundaryKind, FormatOptions, TabularBoundaryKind};

#[derive(Clone, Copy, Default)]
pub(super) enum PayloadProfile {
    #[default]
    Compatibility,
    BoundedTabular,
    BoundedDocuments,
}

// One immutable native limit set for every boundary in a run. These codecs'
// defaults are finite and below the payload route's outer document ceiling.
pub(super) static XLSX_LIMITS: LazyLock<format_xlsx::XlsxLimits> = LazyLock::new(Default::default);
pub(super) static XBRL_LIMITS: LazyLock<format_xbrl::XbrlLimits> = LazyLock::new(Default::default);
pub(super) static PROTOBUF_LIMITS: LazyLock<format_protobuf::EncodeLimits> =
    LazyLock::new(Default::default);

/// The opt-in profile takes format identity from the declaration, never from an
/// uploaded filename or a host's output destination override. An extensionless
/// identity permits explicit format options but cannot guess a missing format.
#[derive(Clone, Copy, Default)]
pub(super) enum BoundaryProfile<'a> {
    #[default]
    Compatibility,
    BoundedTabular {
        declared_path: Option<&'a str>,
    },
    BoundedDocuments,
}

impl<'a> BoundaryProfile<'a> {
    pub(super) fn new(profile: PayloadProfile, declared_path: Option<&'a str>) -> Self {
        match profile {
            PayloadProfile::Compatibility => Self::Compatibility,
            PayloadProfile::BoundedTabular => Self::BoundedTabular { declared_path },
            PayloadProfile::BoundedDocuments => Self::BoundedDocuments,
        }
    }

    pub(super) fn identity(
        self,
        logical_path: &'a Path,
        options: &FormatOptions,
    ) -> anyhow::Result<&'a Path> {
        Ok(match self {
            Self::Compatibility => logical_path,
            Self::BoundedTabular { declared_path } => {
                declared_path.map_or(Path::new("declared-payload-boundary"), Path::new)
            }
            Self::BoundedDocuments => document_identity(options)?,
        })
    }

    pub(super) fn bounded(self) -> bool {
        !matches!(self, Self::Compatibility)
    }

    pub(super) fn documents(self) -> bool {
        matches!(self, Self::BoundedDocuments)
    }
}

pub(super) fn validate_static_route(
    project: &mapping::Project,
    selection: Option<engine::TargetSelection<'_>>,
    required: Option<&engine::RequiredTargetSources<'_>>,
) -> anyhow::Result<()> {
    let dynamic_source = match required {
        Some(required) => required.dynamic_sources.first().copied(),
        None => project
            .extra_sources
            .iter()
            .find(|source| source.dynamic_path.is_some()),
    };
    if let Some(source) = dynamic_source {
        bail!(
            "bounded document profile does not support dynamic source `{}`",
            source.name
        );
    }
    let primary = !matches!(selection, Some(engine::TargetSelection::Named(_)));
    if primary && project.root.produces_documents() {
        bail!("bounded document profile does not support dynamic primary output");
    }
    for target in &project.extra_targets {
        if selection.is_none_or(|selected| {
            matches!(selected, engine::TargetSelection::Named(name) if name == target.name)
        }) && target.root.produces_documents() {
            bail!("bounded document profile does not support dynamic output `{}`", target.name);
        }
    }
    Ok(())
}

/// Select exclusively from saved options. Constructing the accepted option bag
/// also rejects unrelated, conflicting and future options rather than silently
/// ignoring them. Synthetic paths never escape into artifacts or diagnostics.
fn document_identity(options: &FormatOptions) -> anyhow::Result<&'static Path> {
    format_csv::require_executable_dependency(options.csv_text_repair_dependency)?;
    if options.edi_config_reference.is_some() || options.json_schema_unresolved_reference.is_some()
    {
        bail!("bounded document profile refuses unresolved format dependencies");
    }
    if options.sqlite_read.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.local_xml_file_set
        || options.pdf.is_some()
        || options.idoc.is_some()
        || options.idoc_native_config.is_some()
        || options.idoc_native_text_settings.is_some()
        || options.swift_mt.is_some()
    {
        bail!("bounded document profile requires a supported pure document boundary");
    }
    if options.xlsx_update_existing
        || !options.xlsx_rows.is_empty()
        || options.xlsx_composite.is_some()
        || options.xlsx_grid.is_some()
        || options.xlsx_worksheet_set.is_some()
        || options.xlsx_hierarchical.is_some()
    {
        bail!("bounded document profile supports only new flat XLSX workbooks");
    }
    let mut accepted = FormatOptions {
        // Import provenance has no codec or acquisition behavior.
        mfd_decimal_input_names: options.mfd_decimal_input_names.clone(),
        mfd_optional_parser_input_names: options.mfd_optional_parser_input_names.clone(),
        ..Default::default()
    };
    let identity = if options.xml_document {
        accepted.xml_document = true;
        accepted.xml_schema_hints = options.xml_schema_hints.clone();
        accepted.xml_scalar_output = options.xml_scalar_output.clone();
        accepted.xml_allow_inactive_root_type_members =
            options.xml_allow_inactive_root_type_members;
        accepted.xml_root_view_read_policy = options.xml_root_view_read_policy;
        accepted.wsdl = options.wsdl.clone();
        "document.xml"
    } else if options.json_document || options.json5 || options.json_lines {
        accepted.json_document = options.json_document;
        accepted.json5 = options.json5;
        accepted.json_lines = options.json_lines;
        "document.json"
    } else if let Some(kind) = options.edi_kind {
        if !matches!(kind, EdiBoundaryKind::X12 | EdiBoundaryKind::Edifact) {
            bail!("bounded document profile does not support EDI kind {kind:?}");
        }
        if !matches!(
            (kind, options.edi_autocomplete.as_ref()),
            (_, None)
                | (EdiBoundaryKind::X12, Some(EdiAutocomplete::X12(_)))
                | (EdiBoundaryKind::Edifact, Some(EdiAutocomplete::Edifact(_)))
        ) {
            bail!("bounded document profile requires `edi_autocomplete` to match `edi_kind`");
        }
        accepted.edi_kind = Some(kind);
        accepted.lenient_segments = options.lenient_segments;
        accepted.edi_implied_decimals = options.edi_implied_decimals.clone();
        accepted.edi_lexical_formats = options.edi_lexical_formats.clone();
        accepted.edi_value_constraints = options.edi_value_constraints.clone();
        accepted.edi_autocomplete = options.edi_autocomplete.clone();
        accepted.x12_separators = options.x12_separators;
        accepted.x12_interchange_version = options.x12_interchange_version.clone();
        "document.edi"
    } else if let Some(layout) = &options.fixed_width {
        accepted.fixed_width = Some(layout.clone());
        "document.fixed"
    } else if let Some(layout) = &options.flextext {
        accepted.flextext = Some(layout.clone());
        "document.flex"
    } else if let Some(protobuf) = &options.protobuf {
        accepted.protobuf = Some(protobuf.clone());
        "document.pb"
    } else if let Some(xbrl) = &options.xbrl {
        accepted.xbrl = Some(xbrl.clone());
        "document.xbrl"
    } else {
        accepted.tabular_kind = options.tabular_kind;
        accepted.has_header_row = options.has_header_row;
        match options.tabular_kind {
            Some(TabularBoundaryKind::Csv) => {
                accepted.delimiter = options.delimiter;
                accepted.csv_quote = options.csv_quote;
                accepted.csv_quote_disabled = options.csv_quote_disabled;
                accepted.csv_utf8_bom = options.csv_utf8_bom;
                accepted.csv_preserve_empty_strings = options.csv_preserve_empty_strings;
                "document.csv"
            }
            Some(TabularBoundaryKind::Xlsx) => {
                accepted.xlsx_sheet = options.xlsx_sheet.clone();
                accepted.xlsx_start_row = options.xlsx_start_row;
                accepted.xlsx_columns = options.xlsx_columns.clone();
                accepted.xlsx_headers = options.xlsx_headers.clone();
                "document.xlsx"
            }
            None => bail!("bounded document profile requires explicit saved format options"),
        }
    };
    if *options != accepted {
        bail!("bounded document profile refuses conflicting or unsupported format options");
    }
    Ok(Path::new(identity))
}
