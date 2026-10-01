use super::{copied_ancestor_at_path, scope_at, target_scope_for_path};
use egui::Ui;
use ir::{SchemaKind, SchemaNode, XML_ATTRIBUTES_FIELD, XML_ELEMENTS_FIELD};
use mapping::{FormatOptions, IterationOutput, Scope};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScopeOutputProfile {
    XmlElements,
    ReadOnly(&'static str),
}

/// Resolve the owning target before its scope is borrowed for editing. Scalar
/// field labels do not carry the target's cardinality or adapter identity.
pub(crate) fn output_profile(
    root: &Scope,
    target: &SchemaNode,
    path: &[usize],
    options: &FormatOptions,
    output_path: Option<&str>,
) -> ScopeOutputProfile {
    let unavailable = ScopeOutputProfile::ReadOnly;
    if path.is_empty() {
        return unavailable("XML element output selection is available on child scopes");
    }
    let Some(scope) = scope_at(root, path) else {
        return unavailable("The selected scope no longer exists");
    };
    let Ok((target_node, _)) = target_scope_for_path(root, target, path) else {
        return unavailable("The selected scope needs a resolved target group");
    };
    if target_node.repeating {
        return unavailable("This target group repeats according to its schema");
    }
    if !scope.iterates() {
        return unavailable("Select source iteration before choosing element output");
    }
    if scope.concatenated().is_some() {
        return unavailable("Ordered source segments keep their saved output mode");
    }
    if scope.merge_dynamic_fields || scope.output_path().is_some() {
        return unavailable("Dynamic object or document output keeps its saved output mode");
    }
    if copied_ancestor_at_path(root, &path[..path.len() - 1]).is_some() {
        return unavailable("This scope belongs to a whole source group copy");
    }
    let mut schema = target;
    let mut parent = root;
    for &index in path {
        if !regular_xml_group(schema) {
            return unavailable(
                "Generic, alternative or recursive XML groups keep their saved output mode",
            );
        }
        let Some(child) = parent.children.get(index) else {
            return unavailable("The selected scope no longer exists");
        };
        let Some(child_schema) = schema.child(&child.target_field) else {
            return unavailable("The selected scope needs a resolved target group");
        };
        parent = child;
        schema = child_schema;
    }
    if !regular_xml_group(target_node) {
        return unavailable(
            "Generic, alternative or recursive XML groups keep their saved output mode",
        );
    }
    if !uses_xml_output(options, output_path) {
        return unavailable("Element output selection requires an XML target format");
    }
    ScopeOutputProfile::XmlElements
}

fn regular_xml_group(schema: &SchemaNode) -> bool {
    matches!(schema.kind, SchemaKind::Group { .. })
        && !schema.attribute
        && !schema.text
        && !matches!(
            schema.name.as_str(),
            XML_ELEMENTS_FIELD | XML_ATTRIBUTES_FIELD
        )
        && schema.recursive_ref.is_none()
        && schema.alternatives().is_empty()
        && schema.xml_name_alternatives.is_empty()
}

/// Match output adapter precedence: explicit layouts and JSON identity win
/// over filenames; an explicit XML identity can use any filename. Reject
/// conflicting settings rather than offering a mode that cannot be written.
fn uses_xml_output(options: &FormatOptions, path: Option<&str>) -> bool {
    if options.xbrl.is_some()
        || options.idoc.is_some()
        || options.idoc_native_config.is_some()
        || options.idoc_native_text_settings.is_some()
        || options.swift_mt.is_some()
        || options.pdf.is_some()
        || options.flextext.is_some()
        || options.protobuf.is_some()
        || options.edi_kind.is_some()
        || options.edi_config_reference.is_some()
        || options.external_source.is_some()
        || options.local_xml_file_set
        || options.http_get.is_some()
        || options.csv_text_repair_dependency.is_some()
        || options.json_document
        || options.json5
        || options.json_lines
        || options.fixed_width.is_some()
    {
        return false;
    }
    if options.xml_document {
        return !options.lenient_segments
            && options.delimiter.is_none()
            && options.csv_quote.is_none()
            && !options.csv_quote_disabled
            && !options.csv_utf8_bom
            && !options.csv_preserve_empty_strings
            && options.has_header_row.is_none()
            && options.xlsx_sheet.is_none()
            && options.xlsx_start_row.is_none()
            && options.xlsx_columns.is_empty()
            && options.xlsx_headers.is_empty()
            && !options.xlsx_update_existing
            && options.xlsx_rows.is_empty()
            && options.xlsx_composite.is_none()
            && options.xlsx_worksheet_set.is_none()
            && options.xlsx_grid.is_none()
            && options.xlsx_hierarchical.is_none();
    }
    // Automatic .xml dispatch is established by a recognized instance suffix;
    // a tabular fallback cannot override that suffix.
    !options.csv_utf8_bom
        && !options.csv_preserve_empty_strings
        && path
            .and_then(|path| std::path::Path::new(path).extension())
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("xml"))
}

pub(super) fn show(ui: &mut Ui, scope: &mut Scope, profile: ScopeOutputProfile) {
    if !scope.iterates() && scope.iteration_output == IterationOutput::Repeated {
        return;
    }
    ui.horizontal(|ui| {
        ui.label("output:");
        let reason = match profile {
            ScopeOutputProfile::XmlElements => "",
            ScopeOutputProfile::ReadOnly(reason) => reason,
        };
        ui.add_enabled_ui(profile == ScopeOutputProfile::XmlElements, |ui| {
            egui::ComboBox::from_id_salt("scope_iteration_output")
                .selected_text(output_label(scope.iteration_output))
                .show_ui(ui, |ui| {
                    for output in [IterationOutput::First, IterationOutput::MappedSequence] {
                        ui.selectable_value(
                            &mut scope.iteration_output,
                            output,
                            output_label(output),
                        );
                    }
                });
        })
        .response
        .on_disabled_hover_text(reason);
    });
}

fn output_label(output: IterationOutput) -> &'static str {
    match output {
        IterationOutput::Repeated => "Repeated output",
        IterationOutput::First => "First selected element",
        IterationOutput::MappedSequence => "Every selected element",
    }
}
