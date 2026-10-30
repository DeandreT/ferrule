use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use ir::{Instance, SchemaNode};
use mapping::{EdiAutocomplete, EdiBoundaryKind, FormatOptions};

use super::super as cli;
use super::super::format_selection::{self, Direction, JsonMode, Origin, SelectedFormat};
use super::super::{formatted_edi_output, has_flat_xlsx_layout, protobuf_layout, x12_separators};
use super::profile::{self, BoundaryProfile};
use super::{
    MAX_PAYLOAD_DOCUMENT_BYTES, MAX_PAYLOAD_RUN_BYTES, PayloadArtifact, PayloadDestination,
    visit_target_documents,
};

#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub(super) document: usize,
    pub(super) run: usize,
}

impl Limits {
    pub(super) const PRODUCTION: Self = Self {
        document: MAX_PAYLOAD_DOCUMENT_BYTES,
        run: MAX_PAYLOAD_RUN_BYTES,
    };
}

pub(super) struct TargetSerialization<'a> {
    pub(super) current_datetime: &'a str,
    pub(super) limits: Limits,
    pub(super) profile: BoundaryProfile<'a>,
}

enum PayloadBytes {
    Ready(Vec<u8>, usize),
    CodecRefusal {
        error: anyhow::Error,
        records_written: usize,
    },
    CsvOversize {
        maximum: usize,
        records_written: usize,
    },
}

enum ArtifactBytes {
    Ready(Vec<u8>),
    CodecRefusal(anyhow::Error),
    CsvOversize { maximum: usize },
}

pub(super) struct RenderedArtifact {
    target: String,
    path: PathBuf,
    pub(super) records_written: usize,
    bytes: ArtifactBytes,
}

// Preview retains its unbounded serializer and its own incremental budget order.
pub(crate) fn render_payload(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    current_datetime: &str,
) -> anyhow::Result<(Vec<u8>, usize)> {
    match render_payload_with_limit(
        path,
        schema,
        instance,
        options,
        current_datetime,
        None,
        BoundaryProfile::Compatibility,
    )? {
        PayloadBytes::Ready(bytes, records) => Ok((bytes, records)),
        PayloadBytes::CodecRefusal { .. } => unreachable!("compatibility has no bounded codecs"),
        PayloadBytes::CsvOversize { .. } => unreachable!("unbounded CSV has no byte ceiling"),
    }
}

pub(super) fn render_target(
    name: &str,
    destination: &PayloadDestination,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    settings: TargetSerialization<'_>,
) -> anyhow::Result<Vec<RenderedArtifact>> {
    let mut artifacts = Vec::new();
    visit_target_documents(destination, instance, |path, instance| {
        let rendered = render_payload_with_limit(
            &path,
            schema,
            instance,
            options,
            settings.current_datetime,
            Some(settings.limits.document),
            settings.profile,
        )
        .with_context(|| format!("rendering target payload {}", path.display()))?;
        let (bytes, records_written) = match rendered {
            PayloadBytes::Ready(bytes, records_written) => {
                (ArtifactBytes::Ready(bytes), records_written)
            }
            PayloadBytes::CodecRefusal {
                error,
                records_written,
            } => (
                ArtifactBytes::CodecRefusal(error.context(format!(
                    "rendering target `{name}` payload {}",
                    path.display()
                ))),
                records_written,
            ),
            PayloadBytes::CsvOversize {
                maximum,
                records_written,
            } => (ArtifactBytes::CsvOversize { maximum }, records_written),
        };
        artifacts.push(RenderedArtifact {
            target: name.to_string(),
            path,
            records_written,
            bytes,
        });
        Ok(())
    })?;
    Ok(artifacts)
}

pub(super) fn finalize(
    artifacts: Vec<RenderedArtifact>,
    limits: Limits,
) -> anyhow::Result<Vec<PayloadArtifact>> {
    let mut total = 0usize;
    let mut ready = Vec::with_capacity(artifacts.len());
    for artifact in artifacts {
        let bytes = match artifact.bytes {
            ArtifactBytes::Ready(bytes) => {
                if bytes.len() > limits.document {
                    bail!(
                        "output artifact `{}` exceeds the {} MiB per-document limit",
                        artifact.path.display(),
                        limits.document / (1024 * 1024)
                    );
                }
                total = total
                    .checked_add(bytes.len())
                    .context("payload output byte count overflowed")?;
                bytes
            }
            ArtifactBytes::CodecRefusal(error) => return Err(error),
            ArtifactBytes::CsvOversize { maximum } => {
                bail!(
                    "output artifact `{}` exceeds the {} MiB per-document limit",
                    artifact.path.display(),
                    maximum / (1024 * 1024)
                );
            }
        };
        ready.push(PayloadArtifact {
            target: artifact.target,
            path: artifact.path,
            records_written: artifact.records_written,
            bytes,
        });
    }
    if total > limits.run {
        bail!(
            "payload outputs exceed the {} MiB total limit",
            limits.run / (1024 * 1024)
        );
    }
    super::validate_artifact_paths(&ready)?;
    Ok(ready)
}

fn render_payload_with_limit(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    current_datetime: &str,
    csv_maximum: Option<usize>,
    profile: BoundaryProfile<'_>,
) -> anyhow::Result<PayloadBytes> {
    let path = profile.identity(path, options)?;
    cli::validate_xml_scalar_output_format(path, schema, options)?;
    options
        .validate_xml_schema_hint_options(true)
        .map_err(anyhow::Error::msg)?;
    cli::validate_csv_metadata_identity(path, options, "output")?;
    cli::reject_inactive_root_xml_read_options(path, schema, options, "output")?;
    if options.local_xml_file_set {
        bail!("`local_xml_file_set` is input-only");
    }
    let selected = format_selection::select(path, options, Direction::Output, false)?;
    if let SelectedFormat::Xbrl(xbrl) = selected {
        if profile.documents() {
            return bounded_codec_result(
                format_xbrl::to_bytes_bounded(schema, instance, xbrl, &profile::XBRL_LIMITS),
                1,
                "rendering XBRL output payload",
            );
        }
        let text = format_xbrl::to_string(schema, instance, xbrl)
            .context("rendering XBRL output payload")?;
        return Ok(PayloadBytes::Ready(text.into_bytes(), 1));
    }
    if let SelectedFormat::Idoc(layout) = selected {
        let formatted = formatted_edi_output(instance, options)?;
        let bytes = format_edi::idoc::to_bytes(schema, &formatted, layout)
            .context("rendering SAP IDoc output payload")?;
        return Ok(PayloadBytes::Ready(bytes, 1));
    }
    if let SelectedFormat::Swift(_) = selected {
        let formatted = formatted_edi_output(instance, options)?;
        let bytes = format_edi::swift::to_bytes_with_options(schema, &formatted, options)
            .context("rendering SWIFT MT output payload")?;
        return Ok(PayloadBytes::Ready(bytes, 1));
    }
    if let SelectedFormat::Pdf(_) = selected {
        bail!("PDF output is not supported; `pdf` is input-only");
    }
    if let SelectedFormat::FlexText(layout) = selected {
        if profile.documents() {
            return bounded_codec_result(
                format_flextext::to_string(schema, instance, layout).map(String::into_bytes),
                1,
                "rendering FlexText output payload",
            );
        }
        let text = format_flextext::to_string(schema, instance, layout)
            .context("rendering FlexText output payload")?;
        return Ok(PayloadBytes::Ready(text.into_bytes(), 1));
    }
    if let SelectedFormat::Protobuf(protobuf) = selected {
        let layout =
            protobuf_layout(protobuf).context("parsing embedded Protocol Buffers schema")?;
        if profile.documents() {
            return bounded_codec_result(
                format_protobuf::to_vec_bounded(
                    &layout,
                    &protobuf.root_message,
                    instance,
                    &profile::PROTOBUF_LIMITS,
                ),
                1,
                "rendering Protocol Buffers output payload",
            );
        }
        let bytes = format_protobuf::to_vec(&layout, &protobuf.root_message, instance)
            .context("rendering Protocol Buffers output payload")?;
        return Ok(PayloadBytes::Ready(bytes, 1));
    }
    if let SelectedFormat::Edi(Some(kind)) = selected {
        let formatted = formatted_edi_output(instance, options)?;
        let text = render_edi_payload(schema, &formatted, options, kind, current_datetime)?;
        return Ok(PayloadBytes::Ready(text.into_bytes(), 1));
    }
    if let SelectedFormat::Xml(Origin::Explicit) = selected {
        return format_xml::to_string_with_options(
            schema,
            instance,
            &cli::xml_write_options(options),
        )
        .map(|text| PayloadBytes::Ready(text.into_bytes(), 1))
        .context("rendering XML output payload");
    }
    if let SelectedFormat::Json(mode, Origin::Explicit) = selected {
        let text = if mode == JsonMode::Lines {
            format_json::to_lines(schema, instance)
        } else if mode == JsonMode::Json5 {
            format_json::to_json5_string(schema, instance)
        } else {
            format_json::to_string(schema, instance)
        }
        .context("rendering JSON output payload")?;
        return Ok(PayloadBytes::Ready(
            text.into_bytes(),
            instance.as_repeated().map_or(1, <[Instance]>::len),
        ));
    }
    if let SelectedFormat::FixedWidth(layout) = selected {
        let rows = instance
            .as_repeated()
            .context("mapping did not produce a repeating row set for a fixed-width output")?;
        if profile.bounded()
            && let Some(maximum) = csv_maximum
        {
            return match format_csv::to_bytes_fixed_width_validated_bounded(
                schema, rows, layout, maximum,
            ) {
                Ok(bytes) => Ok(PayloadBytes::Ready(bytes, rows.len())),
                Err(format_csv::CsvBoundedError::Format(error)) => {
                    Err(anyhow::Error::new(error)).context("rendering fixed-width output payload")
                }
                Err(format_csv::CsvBoundedError::OutputTooLarge { maximum }) => {
                    Ok(PayloadBytes::CsvOversize {
                        maximum,
                        records_written: rows.len(),
                    })
                }
            };
        }
        let text = format_csv::to_string_fixed_width(schema, rows, layout)
            .context("rendering fixed-width output payload")?;
        return Ok(PayloadBytes::Ready(text.into_bytes(), rows.len()));
    }

    match selected {
        SelectedFormat::Csv => {
            let rows = instance
                .as_repeated()
                .context("mapping did not produce a repeating row set for a CSV output")?;
            if let Some(maximum) = csv_maximum {
                return match format_csv::to_bytes_with_options_bounded(
                    schema,
                    rows,
                    &format_csv::CsvWriteOptions::from(options),
                    maximum,
                ) {
                    Ok(bytes) => Ok(PayloadBytes::Ready(bytes, rows.len())),
                    Err(format_csv::CsvBoundedError::Format(error)) => {
                        Err(anyhow::Error::new(error)).context("rendering CSV output payload")
                    }
                    Err(format_csv::CsvBoundedError::OutputTooLarge { maximum }) => {
                        Ok(PayloadBytes::CsvOversize {
                            maximum,
                            records_written: rows.len(),
                        })
                    }
                };
            }
            let text = format_csv::to_string_with_options(
                schema,
                rows,
                &format_csv::CsvWriteOptions::from(options),
            )
            .context("rendering CSV output payload")?;
            Ok(PayloadBytes::Ready(text.into_bytes(), rows.len()))
        }
        SelectedFormat::Xlsx if profile.documents() => {
            let rows = instance
                .as_repeated()
                .context("mapping did not produce a repeating row set for an XLSX output")?;
            bounded_codec_result(
                format_xlsx::to_bytes_with_options_bounded(
                    schema,
                    rows,
                    format_xlsx::FlatTableWriteOptions {
                        sheet: options.xlsx_sheet.as_deref(),
                        start_row: options.xlsx_start_row.unwrap_or(1),
                        columns: &options.xlsx_columns,
                        headers: &options.xlsx_headers,
                        has_header: options.has_header_row.unwrap_or(true),
                    },
                    &profile::XLSX_LIMITS,
                ),
                rows.len(),
                "rendering XLSX output payload",
            )
        }
        SelectedFormat::Xlsx => render_xlsx_payload(schema, instance, options)
            .map(|(bytes, records)| PayloadBytes::Ready(bytes, records)),
        SelectedFormat::Xml(Origin::Fallback) => {
            format_xml::to_string_with_options(schema, instance, &cli::xml_write_options(options))
                .map(|text| PayloadBytes::Ready(text.into_bytes(), 1))
                .context("rendering XML output payload")
        }
        SelectedFormat::Json(mode, Origin::Fallback) => {
            let text = if mode == JsonMode::Lines {
                format_json::to_lines(schema, instance)
            } else if mode == JsonMode::Json5 {
                format_json::to_json5_string(schema, instance)
            } else {
                format_json::to_string(schema, instance)
            }
            .context("rendering JSON output payload")?;
            Ok(PayloadBytes::Ready(
                text.into_bytes(),
                instance.as_repeated().map_or(1, <[Instance]>::len),
            ))
        }
        SelectedFormat::Sqlite => {
            bail!("SQLite output requires a persistent database and is unavailable as a payload")
        }
        SelectedFormat::Edi(None) => {
            let formatted = formatted_edi_output(instance, options)?;
            let text = match format_edi::dialect_of(schema)? {
                format_edi::Dialect::X12 => render_edi_payload(
                    schema,
                    &formatted,
                    options,
                    EdiBoundaryKind::X12,
                    current_datetime,
                ),
                format_edi::Dialect::Edifact => render_edi_payload(
                    schema,
                    &formatted,
                    options,
                    EdiBoundaryKind::Edifact,
                    current_datetime,
                ),
                format_edi::Dialect::Hl7 => {
                    format_edi::hl7::to_string(schema, &formatted).map_err(anyhow::Error::new)
                }
                format_edi::Dialect::Tradacoms => {
                    format_edi::tradacoms::to_string(schema, &formatted).map_err(anyhow::Error::new)
                }
            }?;
            Ok(PayloadBytes::Ready(text.into_bytes(), 1))
        }
        SelectedFormat::MissingPdf => bail!("PDF output is not supported; PDF is input-only"),
        SelectedFormat::MissingIdoc => bail!("unsupported output payload extension: .idoc"),
        SelectedFormat::MissingSwift => {
            bail!("SWIFT MT output requires an embedded `swift_mt` layout and message headers")
        }
        SelectedFormat::Unsupported(other) => {
            bail!("unsupported output payload extension: .{other}")
        }
        _ => unreachable!("explicit formats returned above"),
    }
}

// Preserve the existing render-all-before-size-refusal ordering. A genuine
// bounded writer refusal owns no partial bytes, but retains its typed cause.
// A later format/destination error must still win over this size refusal.
fn bounded_codec_result<E>(
    result: Result<Vec<u8>, E>,
    records_written: usize,
    context: &'static str,
) -> anyhow::Result<PayloadBytes>
where
    E: std::error::Error + Send + Sync + 'static,
{
    match result {
        Ok(bytes) => Ok(PayloadBytes::Ready(bytes, records_written)),
        Err(error) => {
            let error = anyhow::Error::new(error);
            let limit = matches!(
                error.downcast_ref::<format_xlsx::XlsxFormatError>(),
                Some(format_xlsx::XlsxFormatError::LimitExceeded { .. })
            ) || matches!(
                error.downcast_ref::<format_xbrl::XbrlFormatError>(),
                Some(format_xbrl::XbrlFormatError::LimitExceeded { .. })
            ) || matches!(
                error.downcast_ref::<format_protobuf::BoundedEncodeError>(),
                Some(format_protobuf::BoundedEncodeError::LimitExceeded { .. })
            ) || matches!(
                error.downcast_ref::<format_flextext::FlexTextError>(),
                Some(
                    format_flextext::FlexTextError::OutputTooLarge
                        | format_flextext::FlexTextError::ValueTooLarge { .. }
                        | format_flextext::FlexTextError::InstanceTooDeep
                        | format_flextext::FlexTextError::TooManyNodes
                        | format_flextext::FlexTextError::TooManyRecords
                )
            );
            let error = error.context(context);
            if limit {
                Ok(PayloadBytes::CodecRefusal {
                    error,
                    records_written,
                })
            } else {
                Err(error)
            }
        }
    }
}

fn render_xlsx_payload(
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
) -> anyhow::Result<(Vec<u8>, usize)> {
    if options.xlsx_update_existing {
        bail!(
            "update-existing XLSX output requires a persistent workbook and is unavailable as a payload"
        );
    }
    if let Some(layout) = &options.xlsx_hierarchical {
        if options.xlsx_grid.is_some()
            || options.xlsx_composite.is_some()
            || options.xlsx_worksheet_set.is_some()
            || has_flat_xlsx_layout(options)
        {
            bail!("`xlsx_hierarchical` cannot be combined with other XLSX layout options");
        }
        let (bytes, worksheets) = format_xlsx::to_bytes_hierarchical(schema, instance, layout)
            .context("rendering hierarchical XLSX output payload")?;
        return Ok((bytes, worksheets));
    }
    if options.xlsx_grid.is_some() {
        bail!("grid XLSX output is not supported; `xlsx_grid` is input-only");
    }
    if options.xlsx_worksheet_set.is_some() {
        bail!("worksheet-set XLSX output is not supported; `xlsx_worksheet_set` is input-only");
    }
    if options.xlsx_composite.is_some() {
        bail!("composite XLSX output is not supported; `xlsx_composite` is input-only");
    }
    if !options.xlsx_rows.is_empty() {
        bail!("transposed XLSX output is not supported; `xlsx_rows` is input-only");
    }
    let rows = instance
        .as_repeated()
        .context("mapping did not produce a repeating row set for an XLSX output")?;
    let bytes = format_xlsx::to_bytes_with_options(
        schema,
        rows,
        format_xlsx::FlatTableWriteOptions {
            sheet: options.xlsx_sheet.as_deref(),
            start_row: options.xlsx_start_row.unwrap_or(1),
            columns: &options.xlsx_columns,
            headers: &options.xlsx_headers,
            has_header: options.has_header_row.unwrap_or(true),
        },
    )
    .context("rendering XLSX output payload")?;
    Ok((bytes, rows.len()))
}

fn render_edi_payload(
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    kind: EdiBoundaryKind,
    current_datetime: &str,
) -> anyhow::Result<String> {
    match kind {
        EdiBoundaryKind::X12 => {
            let separators = options
                .x12_separators
                .map(x12_separators)
                .unwrap_or_default();
            let version = options.x12_interchange_version.as_deref();
            match options.edi_autocomplete.as_ref() {
                Some(EdiAutocomplete::X12(config)) => {
                    format_edi::x12::to_string_with_syntax_and_autocomplete(
                        schema,
                        instance,
                        separators,
                        version,
                        format_edi::x12::Autocomplete {
                            current_datetime,
                            request_acknowledgement: config.request_acknowledgement,
                            transaction_set: config.transaction_set.as_deref(),
                        },
                    )
                }
                _ => format_edi::x12::to_string_with_syntax(schema, instance, separators, version),
            }
        }
        EdiBoundaryKind::Edifact => {
            if let Some(EdiAutocomplete::Edifact(config)) = options.edi_autocomplete.as_ref() {
                format_edi::edifact::to_string_with_autocomplete(
                    schema,
                    instance,
                    format_edi::edifact::Autocomplete {
                        current_datetime,
                        syntax_level: config.syntax_level.as_deref(),
                        syntax_version: config.syntax_version.as_deref(),
                        controlling_agency: config.controlling_agency.as_deref(),
                        message_type: config.message_type.as_deref(),
                    },
                )
            } else {
                format_edi::edifact::to_string(schema, instance)
            }
        }
        EdiBoundaryKind::Hl7 => format_edi::hl7::to_string(schema, instance),
        EdiBoundaryKind::Tradacoms => format_edi::tradacoms::to_string(schema, instance),
        EdiBoundaryKind::Idoc => {
            bail!("SAP IDoc output requires an embedded runtime layout")
        }
        EdiBoundaryKind::SwiftMt => {
            bail!("SWIFT MT output requires an embedded `swift_mt` layout")
        }
    }
    .context("rendering EDI output payload")
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use ir::{ScalarType, Value, XmlNil};
    use mapping::{Binding, Graph, NamedTarget, Node, Project, Scope, ScopeIteration};

    use super::*;

    struct Directory(PathBuf, Cell<bool>);

    impl Directory {
        fn new() -> anyhow::Result<Self> {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "ferrule-deferred-csv-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path)?;
            Ok(Self(path, Cell::new(false)))
        }

        fn error(&self, error: &anyhow::Error) -> anyhow::Result<()> {
            std::fs::write(self.0.join("error-debug.txt"), format!("{error:?}"))?;
            std::fs::write(
                self.0.join("error-chain.json"),
                serde_json::to_vec_pretty(
                    &error.chain().map(ToString::to_string).collect::<Vec<_>>(),
                )?,
            )?;
            Ok(())
        }
    }

    impl Drop for Directory {
        fn drop(&mut self) {
            if !self.1.get()
                || std::thread::panicking()
                || std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                    == Some(std::ffi::OsStr::new("1"))
            {
                eprintln!("retained deferred CSV originals: {}", self.0.display());
            } else {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    fn csv_options() -> FormatOptions {
        FormatOptions {
            has_header_row: Some(false),
            ..FormatOptions::default()
        }
    }

    fn scope(node: u32) -> Scope {
        Scope {
            iteration: ScopeIteration::Source(vec!["Rows".into()]),
            bindings: vec![Binding {
                target_field: "Value".into(),
                node,
            }],
            ..Scope::default()
        }
    }

    fn project(first: &str, second: &str) -> Project {
        let schema =
            SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)]);
        Project {
            source: SchemaNode::group(
                "Input",
                vec![
                    SchemaNode::group("Rows", vec![SchemaNode::scalar("File", ScalarType::String)])
                        .repeating(),
                ],
            ),
            target: schema.clone(),
            source_path: None,
            target_path: Some("first.csv".into()),
            source_options: FormatOptions {
                json_document: true,
                ..FormatOptions::default()
            },
            target_options: csv_options(),
            extra_sources: Vec::new(),
            extra_targets: vec![NamedTarget {
                name: "later".into(),
                path: Some("later.csv".into()),
                schema,
                options: csv_options(),
                root: scope(1),
            }],
            failure_rules: Vec::new(),
            user_functions: Default::default(),
            graph: Graph {
                nodes: BTreeMap::from([
                    (
                        0,
                        Node::Const {
                            value: Value::String(first.into()),
                        },
                    ),
                    (
                        1,
                        Node::Const {
                            value: Value::String(second.into()),
                        },
                    ),
                    (
                        2,
                        Node::SourceField {
                            frame: Some(vec!["Rows".into()]),
                            path: vec!["File".into()],
                        },
                    ),
                ]),
            },
            root: scope(0),
        }
    }

    fn source(files: &[&str]) -> Instance {
        Instance::Group(
            vec![(
                "Rows".into(),
                Instance::Repeated(
                    files
                        .iter()
                        .map(|file| {
                            Instance::Group(
                                vec![(
                                    "File".into(),
                                    Instance::Scalar(Value::String((*file).into())),
                                )]
                                .into(),
                            )
                        })
                        .collect(),
                ),
            )]
            .into(),
        )
    }

    fn run(
        directory: &Directory,
        project: &Project,
        files: &[&str],
        limits: Limits,
    ) -> anyhow::Result<(usize, Vec<PayloadArtifact>)> {
        let input = source(files);
        std::fs::write(
            directory.0.join("project.json"),
            mapping::project_file::encode_pretty(project)?,
        )?;
        std::fs::write(
            directory.0.join("input.json"),
            serde_json::to_vec_pretty(&input)?,
        )?;
        let validation = cli::require_valid(project);
        std::fs::write(
            directory.0.join("validation.txt"),
            format!("{validation:?}"),
        )?;
        validation?;
        let output = engine::run_outputs(project, &input);
        std::fs::write(directory.0.join("engine-result.txt"), format!("{output:?}"))?;
        let output = output?;
        let rendered = super::super::render_all_targets(
            project,
            &directory.0.join("project.json"),
            None,
            &output,
            "2026-01-02T03:04:05Z",
            limits,
            profile::PayloadProfile::Compatibility,
        );
        if let Err(error) = &rendered {
            directory.error(error)?;
        }
        let (records, artifacts) = rendered?;
        let mut metadata = Vec::new();
        for (index, artifact) in artifacts.iter().enumerate() {
            let body = match &artifact.bytes {
                ArtifactBytes::Ready(bytes) => {
                    std::fs::write(directory.0.join(format!("slot-{index}.bin")), bytes)?;
                    serde_json::json!({ "bytes": bytes.len() })
                }
                ArtifactBytes::CsvOversize { maximum } => serde_json::json!({
                    "genuine_csv_refusal_maximum": maximum,
                }),
                ArtifactBytes::CodecRefusal(error) => serde_json::json!({
                    "bounded_codec_refusal": error.to_string(),
                }),
            };
            metadata.push(serde_json::json!({
                "target": artifact.target, "path": artifact.path,
                "records": artifact.records_written, "body": body,
            }));
        }
        std::fs::write(
            directory.0.join("rendered-slots.json"),
            serde_json::to_vec_pretty(&metadata)?,
        )?;
        let result = finalize(artifacts, limits);
        match &result {
            Ok(artifacts) => {
                for (index, artifact) in artifacts.iter().enumerate() {
                    std::fs::write(
                        directory.0.join(format!("returned-{index}.bin")),
                        &artifact.bytes,
                    )?;
                }
                std::fs::write(directory.0.join("outcome.json"), serde_json::to_vec_pretty(
                    &artifacts.iter().map(|artifact| serde_json::json!({
                        "target": artifact.target, "path": artifact.path,
                        "bytes": artifact.bytes.len(), "records": artifact.records_written,
                    })).collect::<Vec<_>>()
                )?)?;
            }
            Err(error) => directory.error(error)?,
        }
        result.map(|artifacts| (records, artifacts))
    }

    const SMALL: Limits = Limits {
        document: 16,
        run: 24,
    };

    #[test]
    fn later_csv_format_causes_precede_earlier_genuine_oversize() -> anyhow::Result<()> {
        for nil in [false, true] {
            let directory = Directory::new()?;
            let mut project = project(&"x".repeat(16), "a,b");
            if nil {
                project.graph.nodes.insert(
                    1,
                    Node::Const {
                        value: Value::XmlNil(XmlNil),
                    },
                );
            } else {
                project.extra_targets[0].options.csv_quote_disabled = true;
            }
            let error = run(&directory, &project, &["a.json"], SMALL).unwrap_err();
            let cause = error.downcast_ref::<format_csv::CsvFormatError>();
            if nil {
                assert!(matches!(cause, Some(format_csv::CsvFormatError::ValueType {
                    row: 0, field, expected: ScalarType::String, got: "xml nil",
                }) if field == "Value"));
            } else {
                assert!(
                    matches!(cause, Some(format_csv::CsvFormatError::UnquotedFieldBoundary {
                    row: 0, field,
                }) if field == "Value")
                );
            }
            assert!(
                error
                    .downcast_ref::<format_csv::CsvBoundedError>()
                    .is_none()
            );
            assert_eq!(
                error
                    .chain()
                    .take(3)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                vec![
                    "rendering extra target `later`".to_string(),
                    format!(
                        "rendering target payload {}",
                        directory.0.join("later.csv").display()
                    ),
                    "rendering CSV output payload".to_string(),
                ]
            );
            directory.1.set(true);
        }
        Ok(())
    }

    #[test]
    fn later_format_and_destination_errors_precede_earlier_oversize() -> anyhow::Result<()> {
        for case in 0..3 {
            let directory = Directory::new()?;
            let mut project = project(&"x".repeat(16), "ok");
            project.extra_targets[0].path = match case {
                0 => Some("later.unsupported".into()),
                1 => None,
                _ => Some(format!(
                    "{}.csv",
                    "x".repeat(super::super::MAX_PAYLOAD_PATH_BYTES)
                )),
            };
            let error = run(&directory, &project, &["a.json"], SMALL).unwrap_err();
            let text = format!("{error:#}");
            match case {
                0 => assert!(
                    text.contains("unsupported output payload extension: .unsupported"),
                    "{text}"
                ),
                1 => assert!(
                    text.contains("resolving extra target `later` payload path"),
                    "{text}"
                ),
                _ => assert!(text.contains("4096-byte UTF-8 limit"), "{text}"),
            }
            assert!(!text.contains("per-document limit"), "{text}");
            directory.1.set(true);
        }
        Ok(())
    }

    #[test]
    fn per_slot_document_then_total_then_global_paths_keep_their_order() -> anyhow::Result<()> {
        for (first, second, run_limit, expected) in [
            ("xxxxxxxxxxxxxxxx", "ok", 1, "per-document limit"),
            ("xxxxxxxxxxx", "yyyyyyyyyyy", 20, "total limit"),
            ("x", "y", 24, "multiple output artifacts use path"),
        ] {
            let directory = Directory::new()?;
            let mut project = project(first, second);
            project.extra_targets[0].path = project.target_path.clone();
            let error = run(
                &directory,
                &project,
                &["a.json"],
                Limits {
                    document: 16,
                    run: run_limit,
                },
            )
            .unwrap_err();
            assert!(error.to_string().contains(expected), "{error:#}");
            assert_eq!(
                error.chain().count(),
                1,
                "final budget/path failures have no render contexts"
            );
            directory.1.set(true);
        }
        let directory = Directory::new()?;
        let mut overlapping = project("x", "y");
        overlapping.extra_targets[0].path = Some("first.csv/child.csv".into());
        let error = run(&directory, &overlapping, &["a.json"], SMALL).unwrap_err();
        assert!(
            error.to_string().contains("overlap as file and directory"),
            "{error:#}"
        );
        assert_eq!(error.chain().count(), 1);
        directory.1.set(true);
        let directory = Directory::new()?;
        let mut project = project(&"x".repeat(16), &"y".repeat(16));
        project.target_options = FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        };
        project.target_path = Some("first.json".into());
        let error = run(&directory, &project, &["a.json"], SMALL).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "output artifact `{}` exceeds the 0 MiB per-document limit",
                directory.0.join("first.json").display(),
            )
        );
        directory.1.set(true);
        Ok(())
    }

    #[test]
    fn whole_dynamic_member_checks_and_joined_length_precede_deferred_budget() -> anyhow::Result<()>
    {
        for files in [
            vec!["same.json", "same.json"],
            vec!["a.json", "a.json/b.json"],
            vec!["../escape.json"],
        ] {
            let directory = Directory::new()?;
            let mut project = project(&"x".repeat(16), "ok");
            project.extra_targets[0].options = FormatOptions {
                json_document: true,
                ..FormatOptions::default()
            };
            project.extra_targets[0].path = None;
            project.extra_targets[0].root.iteration = ScopeIteration::DynamicDocuments {
                source: vec!["Rows".into()],
                output_path: 2,
            };
            let error = run(&directory, &project, &files, SMALL).unwrap_err();
            let text = format!("{error:#}");
            assert!(text.contains("dynamic output path"), "{text}");
            assert!(!text.contains("per-document limit"), "{text}");
            directory.1.set(true);
        }
        let directory = Directory::new()?;
        let mut project = project(&"x".repeat(16), "ok");
        project.extra_targets[0].options = FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        };
        project.extra_targets[0].path = None;
        project.extra_targets[0].root.iteration = ScopeIteration::DynamicDocuments {
            source: vec!["Rows".into()],
            output_path: 2,
        };
        let long = format!("{}.json", "x".repeat(super::super::MAX_PAYLOAD_PATH_BYTES));
        let error = run(&directory, &project, &[&long], SMALL).unwrap_err();
        assert!(
            format!("{error:#}").contains("4096-byte UTF-8 limit"),
            "{error:#}"
        );
        directory.1.set(true);
        Ok(())
    }

    #[test]
    fn unbounded_preview_compatibility_is_separate_from_deferred_csv() -> anyhow::Result<()> {
        let directory = Directory::new()?;
        let schema =
            SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)]);
        let rows = Instance::Repeated(vec![Instance::Group(
            vec![(
                "Value".into(),
                Instance::Scalar(Value::String("x".repeat(16))),
            )]
            .into(),
        )]);
        let ordinary = render_payload(
            Path::new("preview.csv"),
            &schema,
            &rows,
            &csv_options(),
            "2026-01-02T03:04:05Z",
        );
        std::fs::write(
            directory.0.join("preview-outcome.txt"),
            format!(
                "{:?}",
                ordinary.as_ref().map(|(bytes, rows)| (bytes.len(), rows))
            ),
        )?;
        let (bytes, records) = ordinary?;
        std::fs::write(directory.0.join("preview.bin"), &bytes)?;
        assert_eq!(bytes, b"xxxxxxxxxxxxxxxx\n");
        assert_eq!(records, 1);
        directory.1.set(true);
        Ok(())
    }

    #[test]
    fn bounded_fixed_width_preserves_deferred_document_and_aggregate_budgets() -> anyhow::Result<()>
    {
        for (width, limit, expected) in [(16, 100, "per-document limit"), (11, 20, "total limit")] {
            let mut project = project("x", "y");
            for options in [
                &mut project.target_options,
                &mut project.extra_targets[0].options,
            ] {
                *options = FormatOptions {
                    fixed_width: Some(mapping::FixedWidthLayout::new(
                        vec![mapping::FixedFieldWidth::new(width).unwrap()],
                        ' ',
                        true,
                        true,
                    )?),
                    ..Default::default()
                };
            }
            let output = engine::run_outputs(&project, &source(&["a.json"]))?;
            let limits = Limits {
                document: 16,
                run: limit,
            };
            let (_, artifacts) = super::super::render_all_targets(
                &project,
                Path::new("/virtual/project.json"),
                None,
                &output,
                "2026-01-02T03:04:05Z",
                limits,
                profile::PayloadProfile::BoundedTabular,
            )?;
            assert!(
                finalize(artifacts, limits)
                    .unwrap_err()
                    .to_string()
                    .contains(expected)
            );
        }
        Ok(())
    }
}
