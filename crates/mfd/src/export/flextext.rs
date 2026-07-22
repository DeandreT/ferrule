use std::fmt::Write as _;

use ir::{ScalarType, SchemaNode};
use mapping::{
    FlexCommand, FlexLineEnding, FlexTextLayout, FormatOptions, ManySplitter, OnceSplitter,
    SwitchMode, TrimSide,
};

use crate::MfdError;

pub(super) fn validate_side(
    schema: &SchemaNode,
    options: &FormatOptions,
    side: &str,
) -> Result<(), MfdError> {
    let Some(layout) = options.flextext.as_ref() else {
        return Ok(());
    };
    if has_conflicting_options(options) {
        return Err(unsupported(format!(
            "the {side} FlexText layout conflicts with options for another format"
        )));
    }
    if layout.schema() != *schema {
        return Err(unsupported(format!(
            "the {side} schema does not exactly match its embedded FlexText layout schema"
        )));
    }
    validate_command(layout.command(), side)
}

pub(super) fn render_config(
    layout: &FlexTextLayout,
    instance_path: Option<&str>,
    side: &str,
) -> Result<String, MfdError> {
    validate_command(layout.command(), side)?;
    let file_name = instance_path
        .map(|path| format!(" FileName=\"{}\"", encode_value(path)))
        .unwrap_or_default();
    let line_ending = match layout.output_line_ending() {
        FlexLineEnding::Lf => "LF",
        FlexLineEnding::Crlf => "CRLF",
    };
    let mut output = String::new();
    let _ = writeln!(output, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    let _ = writeln!(output, "<FlexText>");
    let _ = writeln!(output, "  <Commands>");
    let _ = writeln!(
        output,
        "    <Project{file_name} ByteOrderMark=\"{}\" LineEnding=\"{line_ending}\">",
        u8::from(layout.write_bom())
    );
    value_element(&mut output, 6, "RootName", layout.root_name());
    let _ = writeln!(output, "      <Connections>");
    let _ = writeln!(output, "        <Connection>");
    render_command(&mut output, layout.command(), 10, side)?;
    let _ = writeln!(output, "        </Connection>");
    let _ = writeln!(output, "      </Connections>");
    let _ = writeln!(output, "    </Project>");
    let _ = writeln!(output, "  </Commands>");
    let _ = writeln!(output, "  <Functions/>");
    let _ = writeln!(output, "</FlexText>");
    Ok(output)
}

fn validate_command(command: &FlexCommand, side: &str) -> Result<(), MfdError> {
    match command {
        FlexCommand::SplitOnce {
            splitter,
            first,
            second,
            ..
        } => {
            if matches!(splitter, OnceSplitter::LineStartingWith(_)) {
                return Err(unsupported(format!(
                    "the {side} FlexText layout uses a line-starting single split, which has no lossless canonical .mft representation"
                )));
            }
            validate_command(first, side)?;
            validate_command(second, side)
        }
        FlexCommand::SplitMany { child, .. } => validate_command(child, side),
        FlexCommand::Switch { arms, default, .. } => {
            for arm in arms {
                validate_command(arm.command(), side)?;
            }
            if let Some(default) = default {
                validate_command(default, side)?;
            }
            Ok(())
        }
        FlexCommand::Store { .. }
        | FlexCommand::Ignore
        | FlexCommand::FixedWidthRecords { .. }
        | FlexCommand::DelimitedRecords { .. } => Ok(()),
    }
}

fn has_conflicting_options(options: &FormatOptions) -> bool {
    options.lenient_segments
        || options.edi_kind.is_some()
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.delimiter.is_some()
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.pdf.is_some()
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

fn render_command(
    output: &mut String,
    command: &FlexCommand,
    indent: usize,
    side: &str,
) -> Result<(), MfdError> {
    let pad = " ".repeat(indent);
    match command {
        FlexCommand::SplitOnce {
            name,
            splitter,
            first,
            second,
        } => {
            match splitter {
                OnceSplitter::FixedLines(lines) => {
                    let _ = writeln!(output, "{pad}<SplitSingle>");
                    let _ = writeln!(output, "{pad}  <Upper Offset=\"{}\"/>", lines.get());
                    let _ = writeln!(output, "{pad}  <Lower/>");
                }
                OnceSplitter::FixedColumns(width) => {
                    let _ = writeln!(output, "{pad}<SplitSingle Orientation=\"Vertical\">");
                    let _ = writeln!(output, "{pad}  <Upper Offset=\"1\"/>");
                    let _ = writeln!(output, "{pad}  <Lower Offset=\"{}\"/>", width.get());
                }
                OnceSplitter::Delimiter(separator) => {
                    let _ = writeln!(output, "{pad}<SplitSingle Mode=\"DynF\">");
                    value_element(output, indent + 2, "Separator", separator);
                }
                OnceSplitter::LineContaining(separator) => {
                    let _ = writeln!(
                        output,
                        "{pad}<SplitSingle Mode=\"DynL\" Behavior=\"LineBased\">"
                    );
                    value_element(output, indent + 2, "Separator", separator);
                }
                OnceSplitter::LineStartingWith(_) => {
                    return Err(unsupported(format!(
                        "the {side} FlexText layout uses a line-starting single split, which has no lossless canonical .mft representation"
                    )));
                }
            }
            value_element(output, indent + 2, "Name", name);
            render_connections(output, [first.as_ref(), second.as_ref()], indent + 2, side)?;
            let _ = writeln!(output, "{pad}</SplitSingle>");
        }
        FlexCommand::SplitMany {
            name,
            splitter,
            child,
        } => {
            match splitter {
                ManySplitter::Delimiter(separator) => {
                    let _ = writeln!(output, "{pad}<SplitMultiple>");
                    value_element(output, indent + 2, "Separator", separator);
                    let _ = writeln!(output, "{pad}  <RegexPattern/>");
                }
                ManySplitter::FixedLines(lines) => {
                    let _ = writeln!(
                        output,
                        "{pad}<SplitMultiple Mode=\"Fix\" Offset=\"{}\">",
                        lines.get()
                    );
                }
                ManySplitter::LinesStartingWith(separator) => {
                    let _ = writeln!(
                        output,
                        "{pad}<SplitMultiple Mode=\"DynLS\" Behavior=\"LineStartsWith\">"
                    );
                    value_element(output, indent + 2, "Separator", separator);
                }
            }
            value_element(output, indent + 2, "Name", name);
            render_connections(output, [child.as_ref()], indent + 2, side)?;
            let _ = writeln!(output, "{pad}</SplitMultiple>");
        }
        FlexCommand::Store { name, ty, trim } => {
            let trim = trim
                .as_ref()
                .map(|trim| {
                    let side = match trim.side() {
                        TrimSide::Left => "Left",
                        TrimSide::Right => "Right",
                        TrimSide::Both => "Both",
                    };
                    format!(
                        " TrimSide=\"{side}\" TrimCharSet=\"{}\"",
                        encode_value(trim.characters())
                    )
                })
                .unwrap_or_default();
            let _ = writeln!(output, "{pad}<Store Type=\"{}\"{trim}>", scalar_type(*ty));
            value_element(output, indent + 2, "Name", name);
            let _ = writeln!(output, "{pad}</Store>");
        }
        FlexCommand::Ignore => {
            let _ = writeln!(output, "{pad}<Ignore/>");
        }
        FlexCommand::FixedWidthRecords {
            name,
            fields,
            fill_char,
            record_delimiters,
            treat_empty_as_absent,
        } => {
            if *fill_char != ' ' || !record_delimiters || !treat_empty_as_absent {
                return Err(unsupported(format!(
                    "the {side} FlexText layout uses non-default fixed-width record settings, which have no lossless canonical .mft representation"
                )));
            }
            let _ = writeln!(output, "{pad}<FLF>");
            value_element(output, indent + 2, "RecordName", name);
            let _ = writeln!(output, "{pad}  <Fields>");
            for field in fields {
                let _ = writeln!(
                    output,
                    "{pad}    <Field Type=\"{}\" Size=\"{}\">",
                    scalar_type(field.ty()),
                    field.width().get()
                );
                value_element(output, indent + 6, "Name", field.name());
                let _ = writeln!(output, "{pad}    </Field>");
            }
            let _ = writeln!(output, "{pad}  </Fields>");
            empty_connection(output, indent + 2);
            let _ = writeln!(output, "{pad}</FLF>");
        }
        FlexCommand::DelimitedRecords {
            name,
            dialect,
            fields,
        } => {
            let _ = writeln!(
                output,
                "{pad}<CSV QuoteCharacter=\"{}\" EscapeCharacter=\"{}\">",
                super::schema::xml_escape(&dialect.quote().to_string()),
                super::schema::xml_escape(&dialect.escape().to_string())
            );
            value_element(
                output,
                indent + 2,
                "RecordSeparator",
                dialect.record_separator(),
            );
            value_element(
                output,
                indent + 2,
                "FieldSeparator",
                dialect.field_separator(),
            );
            value_element(output, indent + 2, "RecordName", name);
            let _ = writeln!(output, "{pad}  <Fields>");
            for field in fields {
                let _ = writeln!(
                    output,
                    "{pad}    <Field Type=\"{}\">",
                    scalar_type(field.ty())
                );
                value_element(output, indent + 6, "Name", field.name());
                let _ = writeln!(output, "{pad}    </Field>");
            }
            let _ = writeln!(output, "{pad}  </Fields>");
            empty_connection(output, indent + 2);
            let _ = writeln!(output, "{pad}</CSV>");
        }
        FlexCommand::Switch {
            name,
            mode,
            arms,
            default,
        } => {
            let mode = if *mode == SwitchMode::AllPossible {
                " Mode=\"AllPossible\""
            } else {
                ""
            };
            let _ = writeln!(output, "{pad}<Switch{mode}>");
            value_element(output, indent + 2, "Name", name);
            let _ = writeln!(output, "{pad}  <Conditions>");
            for arm in arms {
                let mode = if arm.contains_regex() {
                    "ContentContainsRegex"
                } else {
                    "ContentStartsWith"
                };
                let _ = writeln!(output, "{pad}    <Condition Mode=\"{mode}\">");
                value_element(output, indent + 6, "Value", arm.prefix());
                render_connections(output, [arm.command()], indent + 6, side)?;
                let _ = writeln!(output, "{pad}    </Condition>");
            }
            if let Some(default) = default {
                let _ = writeln!(output, "{pad}    <Condition Mode=\"Default\">");
                let _ = writeln!(output, "{pad}      <Value/>");
                render_connections(output, [default.as_ref()], indent + 6, side)?;
                let _ = writeln!(output, "{pad}    </Condition>");
            }
            let _ = writeln!(output, "{pad}  </Conditions>");
            empty_connection(output, indent + 2);
            let _ = writeln!(output, "{pad}</Switch>");
        }
    }
    Ok(())
}

fn render_connections<'a>(
    output: &mut String,
    commands: impl IntoIterator<Item = &'a FlexCommand>,
    indent: usize,
    side: &str,
) -> Result<(), MfdError> {
    let pad = " ".repeat(indent);
    let _ = writeln!(output, "{pad}<Connections>");
    for command in commands {
        let _ = writeln!(output, "{pad}  <Connection>");
        render_command(output, command, indent + 4, side)?;
        let _ = writeln!(output, "{pad}  </Connection>");
    }
    let _ = writeln!(output, "{pad}</Connections>");
    Ok(())
}

fn empty_connection(output: &mut String, indent: usize) {
    let pad = " ".repeat(indent);
    let _ = writeln!(output, "{pad}<Connections><Connection/></Connections>");
}

fn value_element(output: &mut String, indent: usize, name: &str, value: &str) {
    let pad = " ".repeat(indent);
    let _ = writeln!(output, "{pad}<{name} Value=\"{}\"/>", encode_value(value));
}

fn scalar_type(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "decimal",
        ScalarType::Bool => "boolean",
    }
}

fn encode_value(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn unsupported(message: String) -> MfdError {
    MfdError::Unsupported(message)
}
