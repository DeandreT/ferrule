//! `.mfd` -> `mapping::Project` conversion.
//!
//! Ordinary import preserves recoverable partial designs and records warnings
//! for unsupported constructs. Callers requiring an immediately executable
//! project can select [`ImportProfile::Executable`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ir::SchemaKind;
use mapping::{
    Graph, NamedSource, NamedTarget, Pipeline, PipelineInput, PipelineNamedInput, PipelineStage,
    Project, Scope, ScopeIteration, ScopeSequence, SequenceExpr,
};

use crate::{
    MfdError, canonical_function,
    resource::{PackageManifest, ResourceResolver},
};

mod aggregate;
mod alternatives;
mod compatibility;
mod constructed_count;
mod constructed_parse_date;
mod constructed_parser_scalar;
mod constructed_runtime_clock;
mod constructed_variable_driver;
mod csv_declaration_proof;
mod csv_document;
mod csv_document_output;
mod db_query;
mod db_where;
mod document_collections;
mod document_paths;
mod dynamic_json;
mod dynamic_xml_variable;
mod exception;
mod external_scalar;
mod external_udf;
mod external_xslt;
mod feed;
mod filtered_document_filename;
mod first_presence;
mod flextext_parser;
mod function;
mod generated_occurrence;
mod graph;
mod group_by_key;
mod group_projection;
pub(crate) mod instance_path;
mod iteration;
mod join;
mod json_parser;
mod json_parser_object;
mod json_serializer;
mod json_variable;
mod lazy_string_substitution;
mod materialize;
mod mixed_content;
mod optional_parser_input;
mod options;
mod output_parameter;
mod pipeline;
mod pipeline_exception;
pub(crate) mod presence;
mod protobuf_target;
mod recursive;
mod scalar_anchor;
mod scalar_function;
mod scalar_root;
mod schema;
mod scope;
mod sequence_scalar;
mod source;
mod source_node_function;
mod target_float_cast;
mod target_iteration;
mod target_mixed_content;
mod target_node_default;
mod target_node_function;
mod target_scope;
mod target_type_cast;
mod transposed_target;
mod udf;
mod unused_xml_source;
mod value_producer;
mod value_refusal;
mod xml_root_view;
mod xml_scalar_output;
mod xml_serializer;

pub use csv_document::{ImportedDocument, import_csv_document, import_csv_document_with_options};
pub use document_paths::{import_filename_document, import_filename_document_with_options};
pub use options::ImportOptions;
pub(crate) use pipeline::validate_pipeline_export_graph;
pub use pipeline::{ImportedPipeline, import_pipeline, import_pipeline_with_options};
pub use schema::{MAX_FLEXTEXT_CONFIGURATION_BYTES, import_flextext_configuration};

use db_query::is_routine_catalog;
use function::{
    is_db_function_component, is_isbn_converter_component, is_xbrl_measure_component,
    map_name as map_function_name, read as read_fn_component, read_isbn_converter_component,
};
use graph::{GraphBuilder, read_copy_all_targets, read_edge_inputs};
use pipeline::{TargetInputCoverage, entry_keys, trace_xml_sinks};
use schema::{
    ComponentFormat, SchemaComponent, enrich_unresolved_edi_source_schemas, note_skipped_library,
    read_csv_component, read_db_component_in_package, read_edi_component,
    read_fixed_width_component, read_flextext_component, read_http_get_component,
    read_json_component_in_package, read_pdf_component, read_protobuf_component,
    read_schema_component_in_package, read_schema_component_in_package_with_provenance,
    read_wsdl_component, read_xbrl_component, read_xlsx_component,
    refine_copied_fallback_source_shapes, refine_copied_fallback_target_groups,
    refine_wsdl_target_schemas, schema_node_at,
};

pub(crate) fn parse_native_pdf_template_text(
    source: &str,
    expected_root: &str,
) -> Result<mapping::PdfLayout, String> {
    schema::parse_native_pdf_template_text(source, expected_root)
}
use scope::{BindingOccurrence, ScopeBuilder, TargetLeaf};
use source::{SourcePath, primary_index, runtime_names};
use target_scope::{build_target_scope, restore_scope_source_hints, scope_source_hints};
use udf::{Call as UdfCall, Registry as UdfRegistry};

pub use compatibility::{
    ImportIssue, ImportIssueKind, ImportOutcome, ImportProfile, ImportReport, assess_import,
    import_with_profile,
};

pub struct Imported {
    pub project: Project,
    pub warnings: Vec<String>,
    /// Canonical path used as the base for imported relative instance paths.
    pub mapping_path: PathBuf,
}

#[derive(Clone, Copy)]
enum StageSelection<'a> {
    Ordinary,
    Into {
        intermediate_key: u32,
        label: &'a str,
        target_inputs: &'a TargetInputCoverage,
    },
    Between {
        source_key: u32,
        source_label: &'a str,
        target_key: u32,
        target_inputs: &'a TargetInputCoverage,
    },
    OutOf {
        intermediate_key: u32,
        earlier_source_key: Option<u32>,
        final_key: u32,
        label: &'a str,
        final_inputs: &'a TargetInputCoverage,
    },
}

struct SourceIdentity {
    name: String,
    key: u32,
}

struct LoweredStage {
    imported: Imported,
    primary_filename: Option<mapping::NodeId>,
    output_filename: Option<mapping::NodeId>,
    /// Original component identities in primary-then-secondary source order.
    source_components: Vec<SourceIdentity>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FilenameImport {
    None,
    CsvRetention,
    Endpoints,
}

struct PrimarySourceHint {
    name: String,
    output_keys: BTreeSet<u32>,
}

fn primary_source_hint(
    mapping: &roxmltree::Node<'_, '_>,
    structure: &roxmltree::Node<'_, '_>,
) -> Option<PrimarySourceHint> {
    let uid = mapping.attribute("ferrule-primary-source")?;
    let component = structure
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("uid") == Some(uid))?;
    Some(PrimarySourceHint {
        name: component.attribute("name").unwrap_or_default().to_string(),
        output_keys: component
            .descendants()
            .filter_map(|node| node.attribute("outkey"))
            .filter_map(|key| key.parse().ok())
            .collect(),
    })
}

fn hinted_primary_index(sources: &[&SchemaComponent], hint: &PrimarySourceHint) -> Option<usize> {
    let mut matching = sources.iter().enumerate().filter(|(_, source)| {
        source.name == hint.name
            && (hint.output_keys.is_empty() || source.output_keys == hint.output_keys)
    });
    let (index, _) = matching.next()?;
    matching.next().is_none().then_some(index)
}

pub fn import(path: &Path) -> Result<Imported, MfdError> {
    import_with_options(path, &ImportOptions::default())
}

pub(crate) fn import_package(package: &crate::MfdPackage) -> Result<Imported, MfdError> {
    let resources = ResourceResolver::from_package(package)?;
    let result = import_resolved(
        &resources,
        StageSelection::Ordinary,
        false,
        FilenameImport::None,
    );
    resources.finish_bindings()?;
    Ok(result?.imported)
}

pub fn import_with_options(path: &Path, options: &ImportOptions) -> Result<Imported, MfdError> {
    let resources = resolved_resources(path, options)?;
    let result = import_resolved(
        &resources,
        StageSelection::Ordinary,
        options.item_ordered_exceptions,
        FilenameImport::None,
    );
    resources.finish_bindings()?;
    Ok(result?.imported)
}

fn resolved_resources(path: &Path, options: &ImportOptions) -> Result<ResourceResolver, MfdError> {
    let discovered_manifest =
        if options.package_root.is_none() && options.package_manifest.is_none() {
            discover_package_manifest(path)?
        } else {
            None
        };
    let manifest_path = options
        .package_manifest
        .as_deref()
        .or(discovered_manifest.as_deref());
    let manifest = manifest_path.map(PackageManifest::load).transpose()?;
    if let (Some(selected_path), Some(selected_root), Some(manifest)) = (
        options.package_manifest.as_deref(),
        options.package_root.as_deref(),
        manifest.as_ref(),
    ) && (manifest.path() != selected_path || manifest.package_root() != selected_root)
    {
        return Err(MfdError::Resource(format!(
            "selected package manifest identity changed from `{}` under `{}` to `{}` under `{}`",
            selected_path.display(),
            selected_root.display(),
            manifest.path().display(),
            manifest.package_root().display()
        )));
    }
    let package_root = manifest
        .as_ref()
        .map(PackageManifest::package_root)
        .or_else(|| options.package_root());
    let mut resources = ResourceResolver::new(path, package_root)?
        .with_edi_catalog_roots(options.edi_catalog_roots())?
        .with_json_schema_catalog_roots(options.json_schema_catalog_roots())?;
    if let Some(manifest) = &manifest {
        resources = resources
            .with_package_edi_catalog_roots(manifest.edi_catalog_roots())?
            .with_package_json_schema_catalog_roots(manifest.json_schema_catalog_roots())?;
    }
    resources.with_bindings(
        &options.resource_bindings_v1,
        options.resource_readset_v1.clone(),
    )
}

fn discover_package_manifest(path: &Path) -> Result<Option<PathBuf>, MfdError> {
    let mapping_path = std::fs::canonicalize(path).map_err(|error| {
        MfdError::Resource(format!(
            "could not canonicalize mapping `{}` ({error})",
            path.display()
        ))
    })?;
    let mut directory = mapping_path
        .parent()
        .ok_or_else(|| MfdError::Resource("mapping has no parent directory".to_string()))?;
    loop {
        let manifest = directory.join("ferrule-package.json");
        if manifest.is_file() {
            return Ok(Some(manifest));
        }
        let Some(parent) = directory.parent() else {
            return Ok(None);
        };
        directory = parent;
    }
}

/// The first child element of `node` with the local name `tag`.
fn child_element<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    tag: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    node.children().find(|child| child.has_tag_name(tag))
}

fn import_resolved(
    resources: &ResourceResolver,
    selection: StageSelection<'_>,
    item_ordered_exceptions: bool,
    filename_import: FilenameImport,
) -> Result<LoweredStage, MfdError> {
    let retain_primary_filename = filename_import != FilenameImport::None;
    let path = resources.mapping_path();
    let text = resources.read_design()?;
    let doc = crate::design::parse(&text)?;
    let mapping_el = doc.root_element();
    if mapping_el.tag_name().name() != "mapping" {
        return Err(MfdError::NotMfd("root element is not <mapping>"));
    }
    let csv_declarations = matches!(selection, StageSelection::Ordinary)
        .then(|| csv_declaration_proof::inspect(mapping_el, &text));
    schema::validate_xml_optional_metadata(&mapping_el)?;
    schema::validate_xml_attribute_required_metadata(&mapping_el)?;
    let wrapper = mapping_el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "component")
        .ok_or(MfdError::NotMfd("no wrapper component"))?;
    let structure = wrapper
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "structure")
        .ok_or(MfdError::NotMfd("wrapper has no structure"))?;
    let primary_source_hint = primary_source_hint(&mapping_el, &structure);
    if mapping_el.descendants().any(|node| {
        xml_scalar_output::xml_request_owner(node).is_some()
            && node
                .ancestors()
                .find(|ancestor| ancestor.has_tag_name("structure"))
                != Some(structure)
    }) {
        return Err(MfdError::Unsupported("XML scalar output inside a user function or outside the active structure cannot be retained safely".into()));
    }
    let selected_language = wrapper
        .children()
        .find(|node| node.has_tag_name("properties"))
        .and_then(|properties| properties.attribute("SelectedLanguage"))
        .unwrap_or_default()
        .to_ascii_lowercase();

    let count_headers =
        aggregate::filtered_count::routing::Headers::read(structure).map_err(MfdError::Resource)?;
    let (edges, count_routing) =
        graph::read_edge_inputs_with_probe(&structure, Some(&wrapper), |claims| {
            aggregate::filtered_count::routing::Routing::prepare(count_headers, structure, claims)
        });
    let count_routing = count_routing.map_err(MfdError::Resource)?;
    if retain_primary_filename && (!edges.complete || !edges.conflicts.is_empty()) {
        return Err(MfdError::UnsupportedImport(
            if filename_import == FilenameImport::CsvRetention {
                "CSV document filename retention requires complete, unambiguous input feeds"
            } else {
                "filename endpoints require complete, unambiguous input feeds"
            }
            .into(),
        ));
    }
    let mut warnings = edges.conflicts.clone();
    if let Some(assessment) = &csv_declarations {
        assessment.append_warnings(&mut warnings);
    }
    let filename_endpoints = if filename_import == FilenameImport::Endpoints {
        Some(document_paths::Endpoints::read(structure, &edges.feeds)?)
    } else {
        None
    };
    let csv_occurrence_raw_authority =
        edges.complete && target_iteration::csv_raw_authority(&structure, &edges.feeds);
    let mut udf_registry = UdfRegistry::read_in_package(&mapping_el, resources, &mut warnings);
    use udf::certificate::{CountConstruction, OriginCapture, RegisteredVec};
    // Negative routing is complete under its separate input-derived policy;
    // only provisionally demanded owners receive optional positive origins.
    let count_construction = count_routing
        .as_ref()
        .map(|routing| CountConstruction::default().with_routing(routing.clone()));
    let count_capture = count_construction
        .as_ref()
        .map_or(OriginCapture::Existing, |b| OriginCapture::Count(b.clone()));
    let mut schema_components = match &count_capture {
        OriginCapture::Existing => RegisteredVec::new(&udf_registry),
        _ => RegisteredVec::with_origin_capture(&udf_registry, count_capture.clone()),
    };
    let mut fn_components =
        RegisteredVec::with_origin_capture(&udf_registry, count_capture.clone());
    let mut json_serializers =
        RegisteredVec::with_origin_capture(&udf_registry, count_capture.clone());
    let mut xml_serializers =
        RegisteredVec::with_origin_capture(&udf_registry, count_capture.clone());
    let mut json_parsers = RegisteredVec::with_origin_capture(&udf_registry, count_capture.clone());
    let mut flextext_parsers =
        RegisteredVec::with_origin_capture(&udf_registry, count_capture.clone());
    let mut output_parameters = Vec::new();
    if resources.package().is_some() && udf_registry.has_declared_mapping_libraries() {
        return Err(MfdError::UnsupportedImport("declared mapping libraries are unsupported in immutable byte packages; no filesystem fallback".into()));
    }
    udf_registry
        .finish_construction()
        .map_err(MfdError::Resource)?;
    let mut udf_calls = RegisteredVec::with_origin_capture(&udf_registry, count_capture.clone());
    let mut external_udf_candidates = Vec::new();
    let mut external_scalar_recipes =
        RegisteredVec::with_origin_capture(&udf_registry, count_capture.clone());
    let mut external_xslt_aggregates =
        RegisteredVec::with_origin_capture(&udf_registry, count_capture);
    let pipeline_exceptions = if matches!(selection, StageSelection::Ordinary) {
        None
    } else {
        Some(pipeline_exception::read_for_stages(&structure, &wrapper)?)
    };
    let mut exception_recipes = Vec::new();
    let mut pending_joins = join::PendingJoins::default();
    let mut skipped_libraries: Vec<String> = Vec::new();
    // Components ferrule can import in general but refused in this design,
    // with the reasons their readers gave.
    let mut skipped_reasons: Vec<String> = Vec::new();
    let mut json5_string_dependencies = Vec::new();
    let inspect_unused_input = unused_xml_source::may_contain_unused_input(structure);
    let root_view_inventory = xml_root_view::Inventory::read(structure);
    if !retain_primary_filename
        && !csv_declarations
            .as_ref()
            .is_some_and(|assessment| assessment.blocks_executable())
        && !structure
            .descendants()
            .any(|node| xml_scalar_output::xml_request_owner(node).is_some())
        && matches!(selection, StageSelection::Ordinary)
        && !udf_registry.has_declared_mapping_libraries()
        && edges.conflicts.is_empty()
        && !(item_ordered_exceptions && exception::item_ordered::has_exception(structure))
        && let Some((project, key)) =
            crate::export::qualified_root_view::import_project(&text, structure, resources)?
    {
        let source_components = vec![SourceIdentity {
            name: project.source.name.clone(),
            key,
        }];
        let mut resource_warnings = Vec::new();
        resources.append_refusals(&mut resource_warnings);
        return Ok(LoweredStage {
            primary_filename: None,
            output_filename: None,
            imported: Imported {
                project,
                warnings: resource_warnings,
                mapping_path: path.to_path_buf(),
            },
            source_components,
        });
    }
    let mut root_view_diagnostics = Vec::new();
    let mut unsupported_root_view_role = false;
    let mut xml_boundary_witnesses = Vec::new();
    let mut fallback_xml_source_outputs = BTreeSet::new();
    let mut fallback_xml_target_inputs = BTreeSet::new();
    let source_node_functions = source_node_function::read(&mapping_el, &mut warnings);

    if let Some(children) = structure
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "children")
    {
        for component in children
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "component")
        {
            udf_registry
                .finish_construction()
                .map_err(MfdError::Resource)?;
            let library = component.attribute("library").unwrap_or_default();
            let name = component.attribute("name").unwrap_or_default().to_string();
            schema_components.set_active(component);
            fn_components.set_active(component);
            json_serializers.set_active(component);
            xml_serializers.set_active(component);
            json_parsers.set_active(component);
            flextext_parsers.set_active(component);
            udf_calls.set_active(component);
            external_scalar_recipes.set_active(component);
            external_xslt_aggregates.set_active(component);
            if udf_registry.refuses_noncanonical_call(component.attribute("kind")) {
                let kind = component.attribute("kind").unwrap_or_default();
                warnings.push(format!("declared kind-19 function `{name}` uses noncanonical kind `{kind}`; refused before built-in/schema/HTTP/module dispatch"));
                continue;
            }
            if udf_registry.refuses_reserved_call(library, component.attribute("kind")) {
                warnings.push(format!("declared kind-19 function `{name}` has reserved owner `{library}`; refused before built-in/schema/HTTP dispatch"));
                continue;
            }
            match library {
                "xml" => {
                    let root_view_findings = root_view_inventory.inspect(component);
                    let root_view_role = root_view_inventory
                        .diagnostic_role(component, !root_view_findings.is_empty());
                    match read_schema_component_in_package_with_provenance(
                        &component,
                        resources,
                        root_view_role,
                        &mut warnings,
                    ) {
                        Some(read) => {
                            let mut sc = read.component;
                            xml_scalar_output::read(
                                component,
                                &mut sc,
                                read.schema_file_authoritative,
                                read.authoritative_schema_bytes.as_deref(),
                                resources,
                                &mut warnings,
                            );
                            xml_root_view::evidence(
                                &root_view_findings,
                                Some(&sc.schema),
                                &mut root_view_diagnostics,
                            );
                            if root_view_role.is_some() && read.no_port_warning_index.is_some() {
                                unsupported_root_view_role = true;
                                continue;
                            }
                            xml_root_view::warn(
                                root_view_findings,
                                Some(&sc.schema),
                                &mut warnings,
                            );
                            let mut retain = |sc: SchemaComponent| {
                                if inspect_unused_input {
                                    xml_boundary_witnesses.push(
                                        unused_xml_source::BoundaryWitness {
                                            component,
                                            schema: sc.schema.clone(),
                                            schema_file_authoritative: read
                                                .schema_file_authoritative,
                                            authoritative_schema_path: read
                                                .authoritative_schema_path
                                                .clone(),
                                            authoritative_schema_bytes: read
                                                .authoritative_schema_bytes
                                                .clone(),
                                            no_port_warning_index: read.no_port_warning_index,
                                        },
                                    );
                                }
                                if read.entry_tree_fallback {
                                    if sc.is_source {
                                        fallback_xml_source_outputs
                                            .extend(sc.output_keys.iter().copied());
                                    } else {
                                        fallback_xml_target_inputs
                                            .extend(sc.input_keys.iter().copied());
                                    }
                                }
                                schema_components.push(sc);
                            };
                            match xml_serializer::read(&component, &sc) {
                                Ok(Some(serializer)) => xml_serializers.push(serializer),
                                Ok(None) => retain(sc),
                                Err(reason) => {
                                    warnings.push(format!(
                                        "XML string serializer `{name}` is unsupported: {reason}"
                                    ));
                                    retain(sc);
                                }
                            }
                        }
                        None => {
                            if xml_scalar_output::requested(component) {
                                return Err(crate::MfdError::Unsupported(
                                    "XML cast request has no safely identifiable schema owner"
                                        .into(),
                                ));
                            }
                            xml_root_view::evidence(
                                &root_view_findings,
                                None,
                                &mut root_view_diagnostics,
                            );
                            xml_root_view::warn(root_view_findings, None, &mut warnings);
                            warnings.push(format!("skipped xml component `{name}`"));
                        }
                    }
                }
                "json" => {
                    match read_json_component_in_package(&component, resources, &mut warnings) {
                        Some(sc) => {
                            // String adapters cease to own FormatOptions when lowered.
                            // Retain their original refusal independently of the recipe.
                            if let Some(dependency) = &sc.options.json5_interchange_dependency {
                                // At most one copy per role, even if malformed input
                                // repeats parameters beside a large setting value.
                                for (usage, kind) in [
                                    ("stringparse", mapping::Json5StringComponentKind::Parser),
                                    (
                                        "stringserialize",
                                        mapping::Json5StringComponentKind::Serializer,
                                    ),
                                ] {
                                    if !component.descendants().any(|node| {
                                        node.has_tag_name("parameter")
                                            && node.attribute("usageKind") == Some(usage)
                                    }) {
                                        continue;
                                    }
                                    json5_string_dependencies.push(
                                        mapping::Json5StringInterchangeDependency {
                                            component: sc.name.clone(),
                                            kind,
                                            dependency: dependency.clone(),
                                            input_instance: sc.input_instance.clone(),
                                            output_instance: sc.output_instance.clone(),
                                        },
                                    );
                                }
                            }
                            match json_serializer::read(&component, &sc) {
                                Ok(Some(serializer)) => json_serializers.push(serializer),
                                Ok(None) => match json_parser::read(
                                    &component,
                                    &sc,
                                    &edges.feeds.values().copied().collect(),
                                ) {
                                    Ok(Some(parser)) => json_parsers.push(parser),
                                    Ok(None) => {
                                        if !sc.is_variable {
                                            document_paths::warn_with_admitted_outputs(
                                                component,
                                                &edges.feeds,
                                                "JSON",
                                                &filename_endpoints
                                                    .as_ref()
                                                    .map(|endpoints| endpoints.inputs())
                                                    .unwrap_or_default(),
                                                &mut warnings,
                                            );
                                        }
                                        schema_components.push(sc);
                                    }
                                    Err(reason) => warnings.push(format!(
                                        "JSON string parser `{name}` is unsupported: {reason}"
                                    )),
                                },
                                Err(reason) => {
                                    warnings.push(format!(
                                        "JSON string serializer `{name}` is unsupported: {reason}"
                                    ));
                                    schema_components.push(sc);
                                }
                            }
                        }
                        None => warnings.push(format!("skipped json component `{name}`")),
                    }
                }
                "xlsx" if component.attribute("kind") == Some("26") => {
                    let first_warning = warnings.len();
                    match read_xlsx_component(&component, &mut warnings) {
                        Some(sc) => schema_components.push(sc),
                        None => {
                            let reason = warnings[first_warning..].join("; ");
                            if reason.is_empty() {
                                note_skipped_library(&mut skipped_libraries, "xlsx");
                            } else {
                                skipped_reasons
                                    .push(format!("skipped xlsx component `{name}`: {reason}"));
                            }
                            warnings.push(format!("skipped xlsx component `{name}`"));
                        }
                    }
                }
                "text" => {
                    let text_el = component
                        .children()
                        .find(|n| n.is_element() && n.tag_name().name() == "data")
                        .and_then(|d| {
                            d.children()
                                .find(|n| n.is_element() && n.tag_name().name() == "text")
                        });
                    let flavor = text_el.and_then(|t| t.attribute("type")).unwrap_or("");
                    if flavor == "csv" {
                        match read_csv_component(&component, &mut warnings) {
                            Some(sc) => schema_components.push(sc),
                            None => warnings.push(format!("skipped csv component `{name}`")),
                        }
                    } else if flavor == "edi" {
                        match read_edi_component(&component, resources, &mut warnings) {
                            Some(sc) => {
                                if !sc.is_variable {
                                    document_paths::warn_with_admitted_outputs(
                                        component,
                                        &edges.feeds,
                                        "EDI",
                                        &filename_endpoints
                                            .as_ref()
                                            .map(|endpoints| endpoints.inputs())
                                            .unwrap_or_default(),
                                        &mut warnings,
                                    );
                                }
                                schema_components.push(sc);
                            }
                            None => warnings.push(format!("skipped edi component `{name}`")),
                        }
                    } else if flavor == "flf" {
                        let string_parse = text_el.is_some_and(|text| {
                            text.parent().is_some_and(|data| {
                                data.children().any(|node| {
                                    node.has_tag_name("parameter")
                                        && node.attribute("usageKind") == Some("stringparse")
                                })
                            })
                        });
                        if string_parse {
                            let warning_count = warnings.len();
                            match read_fixed_width_component(&component, &mut warnings)
                                .ok_or_else(|| {
                                    if warnings.len() == warning_count {
                                        "component could not be compiled".to_string()
                                    } else {
                                        "component has invalid inline settings".to_string()
                                    }
                                })
                                .and_then(|schema| {
                                    flextext_parser::read_fixed_width(&component, schema)
                                }) {
                                Ok(parser) => flextext_parsers.push(parser),
                                Err(reason) => {
                                    note_skipped_library(
                                        &mut skipped_libraries,
                                        "text/flf-stringparse",
                                    );
                                    warnings.push(format!(
                                        "skipped fixed-length string parser `{name}`: {reason}"
                                    ));
                                }
                            }
                        } else {
                            let warning_count = warnings.len();
                            match read_fixed_width_component(&component, &mut warnings) {
                                Some(sc) => schema_components.push(sc),
                                None if warnings.len() == warning_count => warnings
                                    .push(format!("skipped fixed-length component `{name}`")),
                                None => {}
                            }
                        }
                    } else if flavor == "txt"
                        && text_el.and_then(|text| text.attribute("config")).is_some()
                    {
                        let string_parse = text_el.is_some_and(|text| {
                            text.parent().is_some_and(|data| {
                                data.children().any(|node| {
                                    node.has_tag_name("parameter")
                                        && node.attribute("usageKind") == Some("stringparse")
                                })
                            })
                        });
                        if string_parse {
                            match read_flextext_component(&component, resources)
                                .and_then(|schema| flextext_parser::read(&component, schema))
                            {
                                Ok(parser) => flextext_parsers.push(parser),
                                Err(reason) => {
                                    note_skipped_library(
                                        &mut skipped_libraries,
                                        "text/flextext-stringparse",
                                    );
                                    warnings.push(format!(
                                        "skipped FlexText component `{name}`: {reason}"
                                    ));
                                }
                            }
                        } else {
                            match read_flextext_component(&component, resources) {
                                Ok(schema) => schema_components.push(schema),
                                Err(reason) => {
                                    note_skipped_library(&mut skipped_libraries, "text/flextext");
                                    warnings.push(format!(
                                        "skipped FlexText component `{name}`: {reason}"
                                    ));
                                }
                            }
                        }
                    } else {
                        let label = if flavor.is_empty() {
                            "text".to_string()
                        } else {
                            format!("text/{flavor}")
                        };
                        note_skipped_library(&mut skipped_libraries, &label);
                        warnings.push(format!(
                            "skipped component `{name}`: text flavor `{flavor}` is \
                             not supported yet (inline csv, fixed-length, and edi text components import)"
                        ));
                    }
                }
                "db" if is_db_function_component(&component) => {
                    fn_components.push(read_fn_component(&component));
                }
                "db" if is_routine_catalog(&component, &children) => {}
                "db" => match read_db_component_in_package(
                    &component,
                    &mapping_el,
                    resources,
                    &mut warnings,
                ) {
                    Some(sc) => schema_components.push(sc),
                    None => note_skipped_library(&mut skipped_libraries, "db"),
                },
                "webservice" => match read_http_get_component(&component, resources, &mut warnings)
                {
                    Ok(component) => schema_components.push(component),
                    Err(reason) => {
                        note_skipped_library(&mut skipped_libraries, "webservice");
                        warnings.push(format!("skipped web-service component `{name}`: {reason}"));
                    }
                },
                "wsdl" => match read_wsdl_component(&component, resources, &mut warnings) {
                    Ok(component) => schema_components.push(component),
                    Err(reason) => {
                        note_skipped_library(&mut skipped_libraries, "wsdl");
                        warnings.push(format!("skipped WSDL message component `{name}`: {reason}"));
                    }
                },
                "binary" if component.attribute("kind") == Some("33") => {
                    match read_protobuf_component(&component, resources, &mut warnings) {
                        Ok(component) => schema_components.push(component),
                        Err(reason) => {
                            note_skipped_library(&mut skipped_libraries, "binary/protobuf");
                            warnings.push(format!("skipped protobuf component `{name}`: {reason}"));
                        }
                    }
                }
                "pdf" if component.attribute("kind") == Some("34") => {
                    match read_pdf_component(&component, resources, &mut warnings) {
                        Ok(component) => schema_components.push(component),
                        Err(reason) => {
                            note_skipped_library(&mut skipped_libraries, "pdf");
                            warnings.push(format!("skipped PDF component `{name}`: {reason}"));
                        }
                    }
                }
                "xbrl" if component.attribute("kind") == Some("27") => {
                    match read_xbrl_component(&component, resources, &mut warnings) {
                        Ok(component) => schema_components.push(component),
                        Err(reason) => {
                            note_skipped_library(&mut skipped_libraries, "xbrl");
                            warnings.push(format!("skipped XBRL component `{name}`: {reason}"));
                        }
                    }
                }
                "xbrl" if is_xbrl_measure_component(&component) => {
                    fn_components.push(read_fn_component(&component));
                }
                "core" if component.attribute("kind") == Some("7") => {
                    output_parameters.push(output_parameter::read(&component));
                }
                "core" if component.attribute("kind") == Some("32") => {
                    pending_joins.read(component, &mut warnings);
                }
                "core" if component.attribute("kind") == Some("18") => {
                    if pipeline_exceptions
                        .as_ref()
                        .is_none_or(|terminals| terminals.applies_to(&component, selection))
                    {
                        exception_recipes.push(exception::read(&component));
                    }
                }
                "core"
                    if component.attribute("kind") == Some("29")
                        && component.descendants().any(|node| {
                            node.has_tag_name("parameter")
                                && node.attribute("usageKind") == Some("variable")
                        }) =>
                {
                    match read_schema_component_in_package(&component, resources, &mut warnings) {
                        Some(variable) => schema_components.push(variable),
                        None => warnings.push(format!(
                            "skipped core structure variable `{name}`: missing entry tree"
                        )),
                    }
                }
                "core" | "lang" => fn_components.push(read_fn_component(&component)),
                "ferrule"
                    if component.attribute("kind") == Some("5")
                        && canonical_function::is_internal(&name) =>
                {
                    fn_components.push(read_fn_component(&component));
                }
                "ferrule" if recursive::is_component(&component) => {
                    match recursive::read_component(&component) {
                        Ok(function) => fn_components.push(function),
                        Err(reason) => {
                            warnings.push(format!(
                                "skipped ferrule recursive component `{name}`: {reason}"
                            ));
                            let mut function = read_fn_component(&component);
                            function.recursive = Some(function::RecursiveComponent::Invalid);
                            fn_components.push(function);
                        }
                    }
                }
                "edifact" if name == "to-datetime" => {
                    fn_components.push(read_fn_component(&component));
                }
                "xpath2" if map_function_name(&name).is_some() => {
                    fn_components.push(read_fn_component(&component));
                }
                "xpath2"
                    if name == "current-dateTime" && component.attribute("kind") == Some("5") =>
                {
                    fn_components.push(read_fn_component(&component));
                }
                "IsbnConverterService" if is_isbn_converter_component(&component) => {
                    match read_isbn_converter_component(&component) {
                        Ok(function) => fn_components.push(function),
                        Err(reason) => warnings.push(format!(
                            "skipped ISBN converter component `{name}`: {reason}"
                        )),
                    }
                }
                other => {
                    if let Some(definition) = udf_registry.supported(other, &name) {
                        if let Some(shape) = udf_registry.definition(definition) {
                            match UdfCall::read(&component, definition, shape) {
                                Ok(call) => udf_calls.push(call),
                                Err(reason) => warnings.push(format!(
                                    "skipped user-defined function `{name}`: {reason}"
                                )),
                            }
                        }
                    } else if udf_registry.is_declared_call(component.attribute("kind")) {
                        warnings.push(format!("unresolved declared user-defined function `{name}` ({other}): {}; no module discovery or name fallback is permitted", udf_registry.unsupported_reason(other, &name).unwrap_or("required declaring library resource/producer is unavailable or symbol is not directly exported")));
                    } else {
                        match external_scalar::read(&component, path, &selected_language, resources)
                        {
                            Ok(Some(recipe)) => external_scalar_recipes.push(recipe),
                            Err(reason) => {
                                note_skipped_library(&mut skipped_libraries, other);
                                warnings.push(format!(
                                    "external {selected_language} function `{name}` is unsupported: {reason}"
                                ));
                            }
                            Ok(None) => match external_xslt::read(&component, path, resources) {
                                Ok(Some(recipe)) => external_xslt_aggregates.push(recipe),
                                Err(reason) => {
                                    note_skipped_library(&mut skipped_libraries, other);
                                    warnings.push(format!(
                                        "external XSLT function `{name}` is unsupported: {reason}"
                                    ));
                                }
                                Ok(None) => {
                                    note_skipped_library(&mut skipped_libraries, other);
                                    if !external_udf::capture_or_warn(
                                        &component,
                                        udf_registry.unsupported_reason(other, &name),
                                        &mut external_udf_candidates,
                                        &mut warnings,
                                    ) {
                                        warnings.push(format!(
                                            "skipped component `{name}`: unsupported library `{other}` \
                                             (only xml/json/csv/fixed-length/flextext/edi/db/xlsx/protobuf/pdf-source, requestless HTTP GET XML, scalar user-defined functions, and \
                                             core/lang function components and supported XPath 2 functions import)"
                                        ));
                                    }
                                }
                            },
                        }
                    }
                }
            }
        }
    }
    if unsupported_root_view_role {
        return Err(MfdError::UnsupportedImport(format!(
            "connected XML document-root direction was recovered for a zero-port first-entry projection; root-view ownership and condition/construction remain unsupported; no project is published: {}",
            root_view_diagnostics.join(" | ")
        )));
    }
    let user_functions = udf_registry.user_functions();
    udf_registry
        .finish_construction()
        .map_err(MfdError::Resource)?;
    // UDF-owned static catalogs are secondary to ordinary mapping sources.
    // Keeping them last also preserves the document source as the default in
    // scalar-only mappings, where repetition-based primary scoring is tied.
    for source in udf_registry.take_sources() {
        schema_components.push_origin(source.component, source.origin);
    }
    udf_registry
        .finish_construction()
        .map_err(MfdError::Resource)?;

    match selection {
        StageSelection::Ordinary => {}
        StageSelection::Into {
            intermediate_key,
            label,
            ..
        } => {
            let component = schema_components
                .iter_mut()
                .find(|component| {
                    component.input_keys.contains(&intermediate_key) && component.is_pass_through
                })
                .ok_or_else(|| {
                    MfdError::UnsupportedImport(format!(
                        "intermediate target `{label}` could not be imported"
                    ))
                })?;
            component.is_variable = false;
            component.is_source = false;
        }
        StageSelection::Between {
            source_key: intermediate_key,
            source_label: label,
            ..
        }
        | StageSelection::OutOf {
            intermediate_key,
            label,
            ..
        } => {
            let component = schema_components
                .iter_mut()
                .find(|component| {
                    component.input_keys.contains(&intermediate_key) && component.is_pass_through
                })
                .ok_or_else(|| {
                    MfdError::UnsupportedImport(format!(
                        "intermediate source `{label}` could not be imported"
                    ))
                })?;
            component.is_variable = false;
            component.is_source = true;
            component.is_pass_through = false;
        }
    }
    if let StageSelection::OutOf {
        earlier_source_key: Some(earlier_source_key),
        ..
    } = selection
    {
        let component = schema_components
            .iter_mut()
            .find(|component| {
                component.input_keys.contains(&earlier_source_key) && component.is_pass_through
            })
            .ok_or_else(|| {
                MfdError::UnsupportedImport(
                    "earlier intermediate named source could not be imported".into(),
                )
            })?;
        component.is_variable = false;
        component.is_source = true;
        component.is_pass_through = false;
    }
    if let StageSelection::Between { target_key, .. } = selection {
        let component = schema_components
            .iter_mut()
            .find(|component| {
                component.input_keys.contains(&target_key) && component.is_pass_through
            })
            .ok_or_else(|| {
                MfdError::UnsupportedImport("next intermediate target could not be imported".into())
            })?;
        component.is_variable = false;
        component.is_source = false;
    }

    // Only inputs with one distinct physical feed enter the lowering map.
    let edge_from = &edges.feeds;
    let connected_outputs = edge_from.values().copied().collect::<BTreeSet<_>>();
    for component in &schema_components {
        if matches!(selection, StageSelection::Ordinary)
            && component.is_pass_through
            && component
                .input_keys
                .iter()
                .any(|key| edge_from.contains_key(key))
            && component
                .output_keys
                .iter()
                .any(|key| connected_outputs.contains(key))
        {
            warnings.push(format!(
                "chained target `{}` feeds a later mapping, but single-project import cannot make its computed output the later mapping's input; import the chain as a pipeline to preserve this behavior",
                component.name
            ));
        }
    }
    refine_database_roles(&mut schema_components, edge_from);
    udf::refine_source_schemas(&mut schema_components, &udf_calls, &udf_registry, edge_from);
    refine_wsdl_target_schemas(&mut schema_components, &fn_components, edge_from);
    let copy_all_targets = read_copy_all_targets(&structure, Some(&wrapper));
    refine_copied_fallback_source_shapes(
        &mut schema_components,
        edge_from,
        &copy_all_targets,
        &fallback_xml_source_outputs,
        &fallback_xml_target_inputs,
        &mut warnings,
    );
    refine_copied_fallback_target_groups(
        &mut schema_components,
        edge_from,
        &copy_all_targets,
        &fallback_xml_source_outputs,
        &fallback_xml_target_inputs,
        &mut warnings,
    );
    schema::restore_connected_structural_ports(&mut schema_components, edge_from);
    refine_copied_json_root_schemas(&mut schema_components, edge_from, &copy_all_targets);

    let output_failed = output_parameter::install_fallback(
        &mut schema_components,
        output_parameters,
        edge_from,
        &mut warnings,
    );

    let target_inputs =
        external_udf::selected_target_inputs(&schema_components).ok_or_else(|| {
            output_parameter::missing_error(
                "target",
                &skipped_libraries,
                &skipped_reasons,
                output_failed,
            )
        })?;
    external_udf::install_fallback(
        &mut schema_components,
        external_udf_candidates,
        &target_inputs,
        edge_from,
        &fn_components,
        &mut warnings,
    );
    udf_registry
        .finish_construction()
        .map_err(MfdError::Resource)?;

    let mut sources: Vec<&SchemaComponent> = schema_components
        .iter()
        .filter(|c| {
            !c.is_variable
                && (c.is_source
                    || c.format == ComponentFormat::Db
                        && c.output_keys
                            .iter()
                            .any(|key| connected_outputs.contains(key)))
        })
        .collect();
    let targets: Vec<&SchemaComponent> = schema_components
        .iter()
        .filter(|component| component.is_target())
        .collect();
    let intermediates: Vec<&SchemaComponent> =
        schema_components.iter().filter(|c| c.is_variable).collect();
    let unsupported = |side: &str| {
        output_parameter::missing_error(side, &skipped_libraries, &skipped_reasons, output_failed)
    };
    let default_target = targets
        .iter()
        .copied()
        .find(|component| component.is_default_output);
    let target = match selection {
        StageSelection::Ordinary => default_target
            .or_else(|| {
                targets
                    .iter()
                    .copied()
                    .find(|component| !component.is_pass_through)
            })
            .or_else(|| targets.first().copied()),
        StageSelection::Into {
            intermediate_key, ..
        } => targets
            .iter()
            .copied()
            .find(|component| component.input_keys.contains(&intermediate_key)),
        StageSelection::Between { target_key, .. } => targets
            .iter()
            .copied()
            .find(|component| component.input_keys.contains(&target_key)),
        StageSelection::OutOf { final_key, .. } => targets.iter().copied().find(|component| {
            component.input_keys.contains(&final_key)
                || matches!(
                    component.format,
                    ComponentFormat::Csv | ComponentFormat::Json | ComponentFormat::Xlsx
                ) && component.ports.contains_key(&final_key)
        }),
    }
    .ok_or_else(|| unsupported("target"))?;
    let unrepresented_input = match selection {
        StageSelection::Into { target_inputs, .. }
        | StageSelection::Between { target_inputs, .. } => target_inputs
            .unrepresented(&target.ports.keys().copied().collect())
            .map(|key| ("intermediate target", key)),
        StageSelection::OutOf { final_inputs, .. } => final_inputs
            .unrepresented(
                &targets
                    .iter()
                    .flat_map(|candidate| candidate.ports.keys().copied())
                    .collect(),
            )
            .map(|key| ("final targets", key)),
        StageSelection::Ordinary => None,
    };
    if let Some((boundary, key)) = unrepresented_input {
        return Err(MfdError::UnsupportedImport(format!(
            "{boundary}: unrepresented connected input port {key}; importing it would lose a mapping"
        )));
    }
    let connected_targets = if matches!(
        selection,
        StageSelection::Ordinary | StageSelection::OutOf { .. }
    ) {
        std::iter::once(target)
            .chain(targets.iter().copied().filter(|component| {
                !std::ptr::eq(*component, target)
                    && (matches!(selection, StageSelection::Ordinary) || !component.is_pass_through)
                    && component
                        .ports
                        .keys()
                        .any(|key| edge_from.contains_key(key))
            }))
            .collect::<Vec<_>>()
    } else {
        vec![target]
    };
    let target_names = runtime_names(&connected_targets);
    if sources.is_empty() {
        return Err(unsupported("source"));
    }
    let primary_source = match selection {
        StageSelection::Between {
            source_key: intermediate_key,
            source_label: label,
            ..
        }
        | StageSelection::OutOf {
            intermediate_key,
            label,
            ..
        } => sources
            .iter()
            .position(|source| source.input_keys.contains(&intermediate_key))
            .ok_or_else(|| {
                MfdError::UnsupportedImport(format!("intermediate source `{label}` is unavailable"))
            })?,
        _ => primary_source_hint
            .as_ref()
            .and_then(|hint| hinted_primary_index(&sources, hint))
            .unwrap_or_else(|| primary_index(&sources, target, edge_from, &fn_components)),
    };
    sources.swap(0, primary_source);
    let source_components = sources
        .iter()
        .map(|source| SourceIdentity {
            name: source.name.clone(),
            key: source.output_keys.first().copied().unwrap_or_default(),
        })
        .collect();
    let source_names = runtime_names(&sources);
    let primary = sources[0];
    let joins = pending_joins.resolve(
        &join::KeyContext {
            edge_from,
            sources: &sources,
            source_names: &source_names,
            fn_components: &fn_components,
            copy_all_targets: &copy_all_targets,
        },
        &mut warnings,
    );

    // The complete normalized producer inventory is now fixed. Certify before
    // any graph value is materialized or keyed lookup can select a winner.
    let sources = sources;
    let fn_components = fn_components;
    let udf_calls = udf_calls;
    let external_scalar_recipes = external_scalar_recipes;
    let external_xslt_aggregates = external_xslt_aggregates;
    let json_serializers = json_serializers;
    let xml_serializers = xml_serializers;
    let json_parsers = json_parsers;
    let flextext_parsers = flextext_parsers;
    let normalized = udf::certificate::Normalized {
        schemas: &schema_components,
        sources: &sources,
        source_names: &source_names,
        intermediates: &intermediates,
        targets: &targets,
        functions: &fn_components,
        calls: &udf_calls,
        external_scalar: &external_scalar_recipes,
        external_xslt: &external_xslt_aggregates,
        json_serializers: &json_serializers,
        xml_serializers: &xml_serializers,
        json_parsers: &json_parsers,
        flextext_parsers: &flextext_parsers,
        joins: &joins,
        rules: &source_node_functions,
    };
    let count_demand =
        aggregate::filtered_count::routing::finish(count_routing.as_ref(), &normalized).map_err(
            |reason| {
                if reason.starts_with("physical filtered count:") {
                    MfdError::UnsupportedImport(reason)
                } else {
                    MfdError::Resource(reason)
                }
            },
        )?;
    let filtered_counts = aggregate::filtered_count::CountProofTable::compile(
        structure,
        &edges,
        &normalized,
        count_construction.as_ref(),
        count_demand,
        matches!(selection, StageSelection::Ordinary) && filename_import == FilenameImport::None,
    );
    let certificate = udf::certificate::certify(&udf_registry, &udf_calls, normalized, structure);
    udf_registry
        .finish_construction()
        .map_err(MfdError::Resource)?;
    let mut context_certificate = certificate.map_err(MfdError::UnsupportedImport)?;
    context_certificate
        .seal_functions(&user_functions)
        .map_err(MfdError::UnsupportedImport)?;
    // In the declared profile, raw affected hints have now been checked before
    // the legacy parser's last-wins collapse. Ordinary behavior stays unchanged.
    let scope_source_hints = scope_source_hints(&structure);

    let xml_type_conditions = alternatives::conditioned_port_types(&structure);
    let xml_condition_ports = structure
        .descendants()
        .filter(|entry| {
            entry.has_tag_name("entry")
                && entry.children().any(|node| node.has_tag_name("condition"))
        })
        // A wrapper can carry a condition without having a port itself.
        // Preserve that condition on every descendant endpoint of this exact
        // entry branch rather than guessing from keyed entries alone.
        .flat_map(|entry| {
            entry
                .descendants()
                .filter(|node| node.has_tag_name("entry"))
        })
        .flat_map(|entry| [entry.attribute("outkey"), entry.attribute("inpkey")])
        .flatten()
        .filter_map(|key| key.parse::<u32>().ok())
        .collect();
    let mut builder = GraphBuilder {
        graph: Graph::default(),
        native_decimal_input_names: BTreeMap::new(),
        optional_parser_input_names: BTreeMap::new(),
        parser_input_nodes: BTreeMap::new(),
        next_id: 0,
        fn_nodes: BTreeMap::new(),
        filter_nodes: BTreeMap::new(),
        refused_value_feeds: BTreeSet::new(),
        refused_filter_projections: BTreeSet::new(),
        scope_projection_feeds: BTreeSet::new(),
        sequence_items: BTreeMap::new(),
        sequence_scope_components: BTreeSet::new(),
        sequence_predicate_components: BTreeSet::new(),
        warned_sequence_uses: BTreeSet::new(),
        warned_scalar_filters: BTreeSet::new(),
        warned_join_controls: BTreeSet::new(),
        warned_variable_constructions: BTreeSet::new(),
        rejected_join_paths: BTreeSet::new(),
        source_fields: BTreeMap::new(),
        materialized_arguments: Default::default(),
        filtered_counts: &filtered_counts,
        materialized_counts: Default::default(),
        json_serializer_nodes: BTreeMap::new(),
        xml_serializer_nodes: BTreeMap::new(),
        external_scalar_nodes: BTreeMap::new(),
        external_xslt_nodes: BTreeMap::new(),
        json_parser_nodes: BTreeMap::new(),
        flextext_parser_nodes: BTreeMap::new(),
        source_node_function_nodes: BTreeMap::new(),
        claimed_dynamic_ports: BTreeSet::new(),
        query_scope_sources: BTreeSet::new(),
        warned_unscoped_queries: BTreeSet::new(),
        xml_type_conditions,
        xml_condition_ports,
        edge_from,
        csv_occurrence_raw_authority,
        sources: &sources,
        source_names: &source_names,
        intermediates: &intermediates,
        json_serializers: &json_serializers,
        xml_serializers: &xml_serializers,
        external_scalar_recipes: &external_scalar_recipes,
        external_xslt_aggregates: &external_xslt_aggregates,
        json_parsers: &json_parsers,
        flextext_parsers: &flextext_parsers,
        source_node_functions: &source_node_functions,
        fn_components: &fn_components,
        fn_by_output: BTreeMap::new(),
        udf_nodes: BTreeMap::new(),
        udf_by_output: BTreeMap::new(),
        udf_calls: &udf_calls,
        udf_registry: &udf_registry,
        joins,
        framed: std::collections::BTreeSet::new(),
        warnings: Vec::new(),
    };
    for (i, fc) in fn_components.iter().enumerate() {
        for &out in &fc.outputs {
            builder.fn_by_output.insert(out, i);
        }
    }
    for (call_idx, call) in udf_calls.iter().enumerate() {
        for (&output, &component_id) in &call.outputs {
            builder
                .udf_by_output
                .insert(output, (call_idx, component_id));
        }
    }

    let dormant_parser_parameters = builder.dormant_json_parser_parameters();
    #[cfg(test)]
    udf::certificate::probe_speculative_builder(&builder);

    // Dynamic document inputs must establish their driver frames before any
    // target expression is materialized. A filename expression can also feed
    // an ordinary target binding, and SourceField nodes created there need the
    // same framed suffix as the loader path expression.
    let mut dynamic_source_inputs: Vec<Option<(u32, SourcePath)>> = vec![None; sources.len()];
    for (index, extra) in sources.iter().enumerate().skip(1) {
        if matches!(selection, StageSelection::OutOf { earlier_source_key: Some(key), .. } if extra.input_keys.contains(&key))
        {
            // These inputs computed the earlier stage result; they are not
            // run-time loader paths for this stage's named source.
            continue;
        }
        if extra.format == ComponentFormat::Db
            || !extra.db_queries.is_empty()
            || extra.options.external_source.is_some()
        {
            continue;
        }
        let connected = extra
            .input_keys
            .iter()
            .filter_map(|key| edge_from.get(key).copied())
            .collect::<Vec<_>>();
        match connected.as_slice() {
            [] => {}
            [feed] => {
                if let Some(driver) = builder.computed_iteration_source(*feed) {
                    builder.note_framed_prefixes(&driver);
                    dynamic_source_inputs[index] = Some((*feed, driver));
                } else {
                    builder.warnings.push(format!(
                        "extra source `{}` has a connected run-time path that does not have one representable source iteration; the stored instance path is used",
                        source_names[index]
                    ));
                }
            }
            _ => builder.warnings.push(format!(
                "extra source `{}` has multiple connected run-time paths; the stored instance path is used",
                source_names[index]
            )),
        }
    }

    // Other pipeline stages have different source boundaries. Eagerly lower only
    // functions demanded by this stage's targets and owned failure rules.
    let eager_functions = (!matches!(selection, StageSelection::Ordinary)).then(|| {
        materialize::stage_functions(
            &builder,
            connected_targets
                .iter()
                .flat_map(|target| target.ports.keys().copied())
                .chain(
                    exception_recipes
                        .iter()
                        .flat_map(exception::Recipe::input_pins),
                ),
        )
    });
    let csv_documents = (connected_targets.len() == 1)
        .then(|| csv_document_output::build(target, structure, &copy_all_targets, &mut builder))
        .flatten();
    let admitted_csv_outputs = csv_documents
        .as_ref()
        .map(|(_, key)| *key)
        .into_iter()
        .collect();
    let mut root = if let Some((root, _)) = csv_documents {
        root
    } else {
        build_target_scope(
            &mapping_el,
            target,
            &structure,
            resources,
            &copy_all_targets,
            eager_functions.as_ref(),
            &mut builder,
        )
    };
    restore_scope_source_hints(&mut root, target, &scope_source_hints);
    let mut extra_targets = Vec::new();
    for (index, extra) in connected_targets.iter().copied().enumerate().skip(1) {
        let mut extra_root = build_target_scope(
            &mapping_el,
            extra,
            &structure,
            resources,
            &copy_all_targets,
            eager_functions.as_ref(),
            &mut builder,
        );
        restore_scope_source_hints(&mut extra_root, extra, &scope_source_hints);
        let extra_path = if extra_root.produces_documents() {
            None
        } else {
            target_instance_path(extra).or_else(|| default_pass_through_output_path(extra))
        };
        extra_targets.push(NamedTarget {
            name: target_names[index].clone(),
            path: extra_path,
            schema: runtime_target_schema(extra, edge_from),
            options: target_format_options(extra, edge_from),
            root: extra_root,
        });
    }

    let mut extra_sources = Vec::new();
    for (index, extra) in sources.iter().enumerate().skip(1) {
        let dynamic_path = dynamic_source_inputs[index]
            .as_ref()
            .and_then(|(feed, driver)| {
                builder.binding_node(*feed, &[]).map(|node| {
                    mapping::DynamicSourcePath::legacy(node, builder.context_path(driver))
                })
            });
        if dynamic_path.is_none() && extra.input_instance.is_none() {
            builder.warnings.push(format!(
                "extra source `{}` has no input instance path; the imported project needs one \
                 before it can run",
                source_names[index]
            ));
        }
        extra_sources.push(NamedSource {
            name: source_names[index].clone(),
            path: extra
                .input_instance
                .as_deref()
                .map(|stored| {
                    if dynamic_path.is_none() {
                        resources.static_input(stored)
                    } else {
                        stored.to_string()
                    }
                })
                .unwrap_or_default(),
            schema: extra.schema.clone(),
            options: extra.options.clone(),
            dynamic_path,
        });
    }

    for (input, path) in &primary.ports {
        if !matches!(
            selection,
            StageSelection::OutOf { .. } | StageSelection::Between { .. }
        ) && path.as_slice() == [schema::SOURCE_INPUT_DOCUMENT_PATH_PORT]
            && let Some(feed) = builder.edge_from.get(input)
            && builder.static_string_feed(*feed).is_none()
        {
            builder.warnings.push(format!(
                "primary source `{}` has connected FileInstance input port `{input}`; runtime filename acquisition is unsupported",
                primary.name
            ));
        }
    }
    let source_path = primary
        .input_instance
        .clone()
        .or_else(|| {
            if matches!(
                selection,
                StageSelection::OutOf { .. } | StageSelection::Between { .. }
            ) {
                None
            } else {
                builder.static_component_input_path(primary)
            }
        })
        .map(|stored| resources.static_input(&stored));
    let target_path = if root.produces_documents() {
        None
    } else if matches!(
        selection,
        StageSelection::Into { .. } | StageSelection::Between { .. }
    ) {
        // A chained component's input instance is only a source preview for
        // the next stage; an absent output instance does not name a file.
        target
            .output_instance
            .clone()
            .or_else(|| builder.static_target_document_path(target))
    } else {
        target_instance_path(target)
            .or_else(|| builder.static_target_document_path(target))
            .or_else(|| default_pass_through_output_path(target))
    };
    let (mut failure_rules, item_exception) = if item_ordered_exceptions {
        let pending = exception::item_ordered::attach(
            exception_recipes,
            &mut builder,
            &mut root,
            target,
            structure,
            matches!(selection, StageSelection::Ordinary)
                && extra_sources.is_empty()
                && extra_targets.is_empty()
                && user_functions.is_empty(),
        );
        (Vec::new(), pending)
    } else {
        (exception::lower(exception_recipes, &mut builder), None)
    };
    failure_rules.extend(builder.explicit_lookup_input_checks());
    let (primary_filename, output_filename) = if let Some(endpoints) = &filename_endpoints {
        let (input, output) = endpoints.retain(primary, target, &mut builder)?;
        (Some(input), Some(output))
    } else if retain_primary_filename {
        (
            Some(csv_document::retain_filename(
                structure,
                primary,
                &mut builder,
            )?),
            None,
        )
    } else {
        (None, None)
    };
    let global_auto_number_nodes = builder
        .fn_nodes
        .iter()
        .filter_map(|(&index, &node)| {
            let function = &fn_components[index];
            (function.kind == 5
                && function.name == "auto-number"
                && builder.input_feed(index, 0).is_some())
            .then_some(node)
        })
        .collect::<Vec<_>>();
    for component in structure.descendants().filter(|node| {
        node.has_tag_name("component")
            && node.attribute("library") == Some("text")
            && node
                .children()
                .filter(|child| child.has_tag_name("data"))
                .flat_map(|data| data.children())
                .any(|text| text.has_tag_name("text") && text.attribute("type") == Some("csv"))
    }) {
        document_paths::warn_with_admitted_outputs(
            component,
            edge_from,
            "CSV",
            &admitted_csv_outputs,
            &mut warnings,
        );
    }
    warnings.extend(builder.warnings);
    let mut source_options = primary.options.clone();
    source_options.mfd_decimal_input_names =
        std::mem::take(&mut builder.native_decimal_input_names);
    source_options.mfd_optional_parser_input_names =
        std::mem::take(&mut builder.optional_parser_input_names);
    source_options.mfd_json5_string_dependencies = json5_string_dependencies;
    let declared_call_nodes = std::mem::take(&mut builder.udf_nodes);
    let materialized_arguments = std::mem::take(&mut builder.materialized_arguments);
    let materialized_counts = std::mem::take(&mut builder.materialized_counts);
    let mut project = Project {
        execution_contract: None,
        source: primary.schema.clone(),
        target: runtime_target_schema(target, edge_from),
        source_path,
        target_path,
        source_options,
        target_options: target_format_options(target, edge_from),
        extra_sources,
        extra_targets,
        failure_rules,
        user_functions,
        graph: builder.graph,
        root,
    };
    let requests = structure
        .descendants()
        .filter_map(xml_scalar_output::xml_request_owner)
        .map(|node| node.range().start)
        .collect::<BTreeSet<_>>()
        .len();
    let retained = std::iter::once(&project.source_options)
        .chain(std::iter::once(&project.target_options))
        .chain(project.extra_sources.iter().map(|source| &source.options))
        .chain(project.extra_targets.iter().map(|target| &target.options))
        .filter(|options| options.xml_scalar_output.is_some())
        .count();
    if requests > retained {
        return Err(crate::MfdError::Unsupported("XML scalar output request belongs to a transformed or unidentified component and cannot be retained safely".into()));
    }
    exception::item_ordered::finish(&mut project, item_exception, &mut warnings);
    if let Some(filename) = primary_filename {
        let mut document =
            mapping::DocumentProject::new(project, mapping::PrimarySourcePlan::new(filename));
        if let Some(output) = output_filename {
            document = document.with_output_filename(output);
        }
        document.prune_unreachable_nodes();
        project = document.project().clone();
    } else {
        project.prune_unreachable_nodes();
    }
    // These declarations are retained for host inspection, not as executable
    // roots. Their dormant parser introduces no evaluation dependency.
    project.graph.nodes.extend(dormant_parser_parameters);
    // Global-ID counters still lower to Null, including through unanchored
    // scalar consumers. Only retained placeholders are import blockers; an
    // unused eager placeholder must not reject an otherwise supported project.
    for node in global_auto_number_nodes {
        if project.graph.nodes.contains_key(&node) {
            warnings.push(
                "auto-number with a global identifier is unsupported; imported as Null".to_string(),
            );
        }
    }
    enrich_unresolved_edi_source_schemas(&mut project);
    // A flat request whose construction exceeds the root-only profile keeps
    // its retained meaning through the document-wide profile when that admits it.
    if matches!(
        project.target_options.xml_scalar_output,
        Some(ir::XmlScalarOutputPolicy::SuppressCastInSubtreeV1 { .. })
    ) && project.validate_xml_scalar_output().is_err()
    {
        let flat = project
            .target_options
            .xml_scalar_output
            .replace(ir::XmlScalarOutputPolicy::SuppressCastInDocumentV1 {});
        if project.validate_xml_scalar_output().is_err() {
            project.target_options.xml_scalar_output = flat;
        }
    }
    if matches!(
        project.target_options.xml_scalar_output,
        Some(
            ir::XmlScalarOutputPolicy::SuppressCastInSubtreeV1 { .. }
                | ir::XmlScalarOutputPolicy::SuppressCastInDocumentV1 {}
        )
    ) && let Err(error) = project.validate_xml_scalar_output()
        && error.owner == "target"
    {
        project.target_options.xml_scalar_output = Some(xml_scalar_output::unresolved(
            ir::XmlScalarOutputUnresolvedReason::UnsupportedShape,
            "suppressed target requires ordinary constructed output with no named endpoints",
        ));
        warnings.push(
            "XML scalar output is unresolved: unsupported target construction or endpoints".into(),
        );
    }
    if matches!(selection, StageSelection::Ordinary) {
        unused_xml_source::classify(
            &mapping_el,
            &structure,
            &xml_boundary_witnesses,
            &project,
            &mut warnings,
        );
    }
    let context_audit: Result<(), String> = (|| {
        udf_registry.audit_materialized_arguments(&materialized_arguments, &project.graph)?;
        context_certificate.audit_functions(&project.user_functions)?;
        context_certificate.audit_target_count(
            connected_targets.len().saturating_sub(1),
            project.extra_targets.len(),
        )?;
        context_certificate.audit(target, &project.root, &project.graph, &declared_call_nodes)?;
        for (descriptor, actual) in connected_targets.iter().skip(1).zip(&project.extra_targets) {
            context_certificate.audit(
                descriptor,
                &actual.root,
                &project.graph,
                &declared_call_nodes,
            )?;
        }
        Ok(())
    })();
    udf_registry
        .finish_construction()
        .map_err(MfdError::Resource)?;
    context_audit.map_err(MfdError::UnsupportedImport)?;
    filtered_counts
        .audit(
            &materialized_counts,
            &project,
            &connected_targets,
            context_certificate.enabled(),
        )
        .map_err(|reason| {
            MfdError::UnsupportedImport(format!("physical filtered count: {reason}"))
        })?;
    if context_certificate.enabled() {
        project.execution_contract = Some(mapping::ExecutionContract::ClosedPrimarySourceV1);
        project.validate_execution_contract().map_err(|error| MfdError::UnsupportedImport(format!("declared-library primary-source safety guard refused the materialized model: {error}")))?;
    }
    resources.append_refusals(&mut warnings);
    Ok(LoweredStage {
        primary_filename,
        output_filename,
        imported: Imported {
            project,
            warnings,
            mapping_path: path.to_path_buf(),
        },
        source_components,
    })
}

/// A component's format options in its target role, including the output
/// schema-location hint its settings derive. A mapping that feeds the root's
/// attribute wildcard, or the whole root, can write `xsi` attributes at run
/// time; the mapped value then wins and no automatic hint is added.
fn target_format_options(
    component: &SchemaComponent,
    edge_from: &BTreeMap<u32, u32>,
) -> mapping::FormatOptions {
    let mut options = component.options.clone();
    // An input instance is read only in a source role, so a pass-through
    // component's file set never applies to its output.
    options.local_xml_file_set = false;
    let feeds_root_wildcard = component.schema.child(ir::XML_ATTRIBUTES_FIELD).is_some()
        && component.input_keys.iter().any(|key| {
            edge_from.contains_key(key)
                && component.ports.get(key).is_some_and(|path| {
                    path.first()
                        .is_none_or(|segment| segment == ir::XML_ATTRIBUTES_FIELD)
                })
        });
    if options.xml_schema_hints.is_none() && !feeds_root_wildcard {
        options.xml_schema_hints = component.target_schema_hints.clone();
    }
    options
}

/// The output document a target's own instance paths name. Its input instance
/// is only a preview, so it counts only when there is no output instance.
fn target_instance_path(component: &SchemaComponent) -> Option<String> {
    component
        .output_instance
        .clone()
        .or_else(|| target_preview_path(component))
}

/// A target without an output instance writes to its input instance, unless
/// that names a file set, which no single output document can be, whatever
/// the target's format.
fn target_preview_path(component: &SchemaComponent) -> Option<String> {
    component
        .input_instance
        .clone()
        .filter(|path| !schema::is_local_file_set_pattern(path))
}

fn default_pass_through_output_path(component: &SchemaComponent) -> Option<String> {
    if !component.is_pass_through {
        return None;
    }
    let stem = if component.name.trim().is_empty() {
        &component.schema.name
    } else {
        &component.name
    };
    Some(format!("{stem}.xml"))
}

/// Database components can expose read and write ports in one visual component.
/// Their entry counts alone cannot determine the role when both sides have the
/// same shape, so connected table inputs decide target ownership once the graph
/// is available. A connected output still admits the same component as a source.
fn refine_database_roles(components: &mut [SchemaComponent], edge_from: &BTreeMap<u32, u32>) {
    for component in components {
        if component.format == ComponentFormat::Db
            && component.db_queries.is_empty()
            && component
                .input_keys
                .iter()
                .any(|key| edge_from.contains_key(key))
        {
            component.is_source = false;
        }
    }
}

fn runtime_target_schema(
    component: &SchemaComponent,
    edge_from: &BTreeMap<u32, u32>,
) -> ir::SchemaNode {
    if component.format != ComponentFormat::Db || component.schema.repeating {
        return component.schema.clone();
    }
    let selected_tables = component
        .input_keys
        .iter()
        .filter(|key| edge_from.contains_key(key))
        .filter_map(|key| component.ports.get(key).and_then(|path| path.first()))
        .cloned()
        .collect::<BTreeSet<_>>();
    if selected_tables.is_empty() {
        return component.schema.clone();
    }
    let mut schema = component.schema.clone();
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        return schema;
    };
    let mut retained = BTreeSet::new();
    children.retain(|child| {
        selected_tables.contains(&child.name) && retained.insert(child.name.clone())
    });
    schema
}

fn refine_copied_json_root_schemas(
    components: &mut [SchemaComponent],
    edge_from: &BTreeMap<u32, u32>,
    copy_all_targets: &BTreeSet<u32>,
) {
    let replacements = components
        .iter()
        .enumerate()
        .filter(|(_, target)| {
            !target.is_source && target.format == ComponentFormat::Json && target.schema.is_scalar()
        })
        .filter_map(|(target_index, target)| {
            let feed = target
                .ports
                .iter()
                .find(|(input, path)| path.is_empty() && copy_all_targets.contains(input))
                .and_then(|(input, _)| edge_from.get(input))?;
            let source_schema = components.iter().find_map(|source| {
                let path = source.ports.get(feed)?;
                let node = schema_node_at(&source.schema, path)?;
                (!node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
                    .then(|| node.clone())
            })?;
            Some((target_index, source_schema))
        })
        .collect::<Vec<_>>();
    for (target_index, schema) in replacements {
        components[target_index].schema = schema;
    }
}

impl GraphBuilder<'_> {
    fn sequence_expr(&mut self, idx: usize) -> Option<SequenceExpr> {
        let item = self.sequence_item(idx);
        if let Some(function::RecursiveComponent::Collect {
            collection,
            children,
            descent_value,
            values,
            value,
        }) = self
            .fn_components
            .get(idx)
            .and_then(|component| component.recursive.clone())
        {
            let source = self
                .input_feed(idx, 0)
                .and_then(|feed| self.source_abs_path(feed))?;
            if self.context_path(&source) != collection {
                return None;
            }
            let prefix = self
                .input_feed(idx, 1)
                .and_then(|feed| self.sequence_scalar_input(feed))?;
            let separator = self
                .input_feed(idx, 2)
                .and_then(|feed| self.sequence_scalar_input(feed))?;
            return Some(SequenceExpr::RecursiveCollect {
                collection,
                children,
                descent_value,
                values,
                value,
                prefix,
                separator,
                item,
            });
        }
        Some(match self.fn_components[idx].name.as_str() {
            "tokenize" => {
                let input = self
                    .input_feed(idx, 0)
                    .and_then(|feed| self.sequence_scalar_input(feed))?;
                let delimiter = self
                    .input_feed(idx, 1)
                    .and_then(|feed| self.sequence_scalar_input(feed))?;
                SequenceExpr::Tokenize {
                    input,
                    delimiter,
                    item,
                }
            }
            "tokenize-by-length" => {
                let input = self
                    .input_feed(idx, 0)
                    .and_then(|feed| self.sequence_scalar_input(feed))?;
                let length = self
                    .input_feed(idx, 1)
                    .and_then(|feed| self.sequence_scalar_input(feed))?;
                SequenceExpr::TokenizeByLength {
                    input,
                    length,
                    item,
                }
            }
            "tokenize-regexp" => {
                let input = self
                    .input_feed(idx, 0)
                    .and_then(|feed| self.sequence_scalar_input(feed))?;
                let pattern = self
                    .input_feed(idx, 1)
                    .and_then(|feed| self.sequence_scalar_input(feed))?;
                let flags = self
                    .input_feed(idx, 2)
                    .and_then(|feed| self.sequence_scalar_input(feed));
                SequenceExpr::TokenizeRegex {
                    input,
                    pattern,
                    flags,
                    item,
                }
            }
            "generate-sequence" => {
                let from = self
                    .input_feed(idx, 0)
                    .and_then(|feed| self.sequence_scalar_input(feed));
                let to = self
                    .input_feed(idx, 1)
                    .and_then(|feed| self.sequence_scalar_input(feed))?;
                SequenceExpr::Generate { from, to, item }
            }
            _ => return None,
        })
    }
}
