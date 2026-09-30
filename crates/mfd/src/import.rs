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
mod db_query;
mod db_where;
mod dynamic_json;
mod dynamic_xml_variable;
mod exception;
mod external_scalar;
mod external_udf;
mod external_xslt;
mod feed;
mod flextext_parser;
mod function;
mod generated_occurrence;
mod graph;
mod group_projection;
mod instance_path;
mod iteration;
mod join;
mod json_parser;
mod json_serializer;
mod materialize;
mod mixed_content;
mod output_parameter;
mod protobuf_target;
mod recursive;
mod scalar_anchor;
mod scalar_function;
mod schema;
mod scope;
mod sequence_scalar;
mod source;
mod source_node_function;
mod target_iteration;
mod target_mixed_content;
mod target_node_default;
mod target_node_function;
mod target_type_cast;
mod udf;
mod xml_serializer;

use db_query::is_routine_catalog;
use function::{
    is_db_function_component, is_isbn_converter_component, is_xbrl_measure_component,
    map_name as map_function_name, read as read_fn_component, read_isbn_converter_component,
};
use graph::{GraphBuilder, read_copy_all_targets, read_edges};
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
use scope::{ScopeBuilder, TargetLeaf};
use source::{SourcePath, primary_index, runtime_names};
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

/// A connected staged design imported without flattening its intermediate
/// outputs into independent targets.
pub struct ImportedPipeline {
    pub pipeline: Pipeline,
    pub warnings: Vec<String>,
    /// Canonical path used as the active mapping identity for every stage.
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
    /// Original component identities in primary-then-secondary source order.
    source_components: Vec<SourceIdentity>,
}

/// Filesystem policy for importing one mapping package.
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    package_root: Option<PathBuf>,
    package_manifest: Option<PathBuf>,
    edi_catalog_roots: Vec<PathBuf>,
    json_schema_catalog_roots: Vec<PathBuf>,
}

impl ImportOptions {
    /// Confines all mapping resources to this trusted directory.
    pub fn with_package_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.package_root = Some(root.into());
        self.package_manifest = None;
        self
    }

    pub fn package_root(&self) -> Option<&Path> {
        self.package_root.as_deref()
    }

    /// Applies roots from an explicitly trusted mapping package manifest.
    ///
    /// Directly configured catalog roots keep search precedence. The
    /// manifest's directory becomes the package root.
    pub fn with_package_manifest(mut self, path: impl AsRef<Path>) -> Result<Self, MfdError> {
        let manifest = PackageManifest::load(path.as_ref())?;
        self.package_root = Some(manifest.package_root().to_path_buf());
        self.package_manifest = Some(manifest.path().to_path_buf());
        Ok(self)
    }

    /// Adds an explicitly trusted EDI configuration catalog.
    ///
    /// Catalogs are searched in declaration order after package-contained
    /// resources. Each resolved configuration remains confined to its
    /// canonical catalog directory.
    pub fn with_edi_catalog_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.edi_catalog_roots.push(root.into());
        self
    }

    /// Adds explicitly trusted EDI configuration catalogs in search order.
    pub fn with_edi_catalog_roots<I, P>(mut self, roots: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.edi_catalog_roots
            .extend(roots.into_iter().map(Into::into));
        self
    }

    pub fn edi_catalog_roots(&self) -> &[PathBuf] {
        &self.edi_catalog_roots
    }

    /// Adds an explicitly trusted JSON Schema catalog.
    ///
    /// Catalogs are searched in declaration order after package-contained
    /// resources. Each resolved schema and its local-file references remain
    /// confined to the matched canonical catalog directory.
    pub fn with_json_schema_catalog_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.json_schema_catalog_roots.push(root.into());
        self
    }

    /// Adds explicitly trusted JSON Schema catalogs in search order.
    pub fn with_json_schema_catalog_roots<I, P>(mut self, roots: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.json_schema_catalog_roots
            .extend(roots.into_iter().map(Into::into));
        self
    }

    pub fn json_schema_catalog_roots(&self) -> &[PathBuf] {
        &self.json_schema_catalog_roots
    }
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

fn scope_source_hints(structure: &roxmltree::Node<'_, '_>) -> BTreeMap<u32, Vec<String>> {
    structure
        .children()
        .find(|node| {
            node.has_tag_name("ferrule-scope-sources") && node.attribute("version") == Some("1")
        })
        .into_iter()
        .flat_map(|metadata| {
            metadata
                .children()
                .filter(|node| node.has_tag_name("scope"))
        })
        .filter_map(|scope| {
            let target = scope.attribute("targetkey")?.parse().ok()?;
            let source = serde_json::from_str(scope.attribute("source")?).ok()?;
            Some((target, source))
        })
        .collect()
}

fn restore_scope_source_hints(
    root: &mut Scope,
    target: &SchemaComponent,
    hints: &BTreeMap<u32, Vec<String>>,
) {
    fn scope_at_mut<'a>(mut scope: &'a mut Scope, path: &[String]) -> Option<&'a mut Scope> {
        for segment in path {
            scope = scope
                .children
                .iter_mut()
                .find(|child| child.target_field == *segment)?;
        }
        Some(scope)
    }

    for (target_key, source) in hints {
        let Some(path) = target.ports.get(target_key) else {
            continue;
        };
        let Some(scope) = scope_at_mut(root, path) else {
            continue;
        };
        if matches!(scope.iteration, ScopeIteration::Source(_)) {
            scope.iteration = ScopeIteration::Source(source.clone());
        }
    }
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

pub fn import_with_options(path: &Path, options: &ImportOptions) -> Result<Imported, MfdError> {
    let resources = resolved_resources(path, options)?;
    Ok(import_resolved(&resources, StageSelection::Ordinary)?.imported)
}

/// Import a connected, file-based design as a typed pipeline.
///
/// This profile accepts a bounded serial XML pass-through chain whose final
/// primary target may be XML, CSV, fixed-width text, FlexText, JSON, Protocol
/// Buffers, XBRL without presentation metadata, or a new XLSX workbook. Connected
/// final named targets may be XML, or one CSV or JSON target when the primary is XML.
/// Other stage graph shapes reject.
pub fn import_pipeline(path: &Path) -> Result<ImportedPipeline, MfdError> {
    import_pipeline_with_options(path, &ImportOptions::default())
}

pub fn import_pipeline_with_options(
    path: &Path,
    options: &ImportOptions,
) -> Result<ImportedPipeline, MfdError> {
    let resources = resolved_resources(path, options)?;
    let chain = discover_pipeline_chain(resources.mapping_path())?;
    let first_intermediate = &chain.intermediates[0];
    let mut lowered = vec![import_resolved(
        &resources,
        StageSelection::Into {
            intermediate_key: first_intermediate.key,
            label: &first_intermediate.label,
            target_inputs: &first_intermediate.inputs,
        },
    )?];
    for pair in chain.intermediates.windows(2) {
        let [first_intermediate, second_intermediate] = pair else {
            unreachable!("adjacent intermediate window has two entries")
        };
        lowered.push(import_resolved(
            &resources,
            StageSelection::Between {
                source_key: first_intermediate.key,
                source_label: &first_intermediate.label,
                target_key: second_intermediate.key,
                target_inputs: &second_intermediate.inputs,
            },
        )?);
    }
    let last_intermediate = chain
        .intermediates
        .last()
        .expect("chain has an intermediate");
    lowered.push(import_resolved(
        &resources,
        StageSelection::OutOf {
            intermediate_key: last_intermediate.key,
            final_key: chain.final_key,
            label: &last_intermediate.label,
            final_inputs: &chain.final_inputs,
        },
    )?);

    let warnings = lowered
        .iter()
        .flat_map(|stage| stage.imported.warnings.iter().cloned())
        .collect::<Vec<_>>();
    if !warnings.is_empty() {
        return Err(MfdError::UnsupportedImport(format!(
            "chained design has unsupported stage behavior: {}",
            warnings.join("; ")
        )));
    }
    let names = lowered[0]
        .source_components
        .iter()
        .fold(BTreeMap::new(), |mut counts, source| {
            *counts.entry(source.name.as_str()).or_insert(0usize) += 1;
            counts
        });
    let mut used_host_names = BTreeSet::new();
    let mut host_names = BTreeMap::new();
    for source in &lowered[0].source_components {
        let preferred = if !source.name.is_empty()
            && source.name.len() <= 256
            && names.get(source.name.as_str()) == Some(&1)
        {
            source.name.clone()
        } else {
            format!("mfd-source-{}", source.key)
        };
        let name = if used_host_names.insert(preferred.clone()) {
            preferred
        } else {
            let fallback = format!("mfd-source-{}", source.key);
            if !used_host_names.insert(fallback.clone()) {
                return Err(MfdError::UnsupportedImport(
                    "pipeline source port identities are not unique".into(),
                ));
            }
            fallback
        };
        if host_names.insert(source.key, name).is_some() {
            return Err(MfdError::UnsupportedImport(
                "pipeline source port identities are not unique".into(),
            ));
        }
    }
    if lowered.iter().any(|stage| {
        stage.imported.project.extra_sources.len() + 1 != stage.source_components.len()
    }) {
        return Err(MfdError::UnsupportedImport(
            "pipeline source boundaries do not match imported stages".into(),
        ));
    }
    let mapping_path = resources.mapping_path().to_path_buf();
    let mapping_identity = mapping_path.to_string_lossy().into_owned();
    let mut stages = Vec::with_capacity(lowered.len());
    for (index, stage) in lowered.into_iter().enumerate() {
        let id = format!("mfd-stage-{}", index + 1);
        let primary = if index == 0 {
            let first = stage.source_components.first().ok_or_else(|| {
                MfdError::UnsupportedImport("first pipeline stage has no source".into())
            })?;
            PipelineInput::Host {
                name: host_names[&first.key].clone(),
            }
        } else {
            PipelineInput::StageTarget {
                stage: format!("mfd-stage-{index}"),
                target: None,
            }
        };
        let mut extra_sources = Vec::new();
        for (source, identity) in stage
            .imported
            .project
            .extra_sources
            .iter()
            .zip(stage.source_components.iter().skip(1))
        {
            if source.dynamic_path.is_some() {
                continue;
            }
            let Some(host_name) = host_names.get(&identity.key) else {
                return Err(MfdError::UnsupportedImport(format!(
                    "pipeline stage {} reads an unbound source `{}`",
                    index + 1,
                    identity.name
                )));
            };
            extra_sources.push(PipelineNamedInput {
                name: source.name.clone(),
                from: PipelineInput::Host {
                    name: host_name.clone(),
                },
            });
        }
        stages.push(PipelineStage {
            id,
            mapping_path: Some(mapping_identity.clone()),
            project: stage.imported.project,
            source: primary,
            extra_sources,
        });
    }
    let pipeline = Pipeline {
        main_mapping_path: Some(mapping_identity),
        stages,
    };
    let issues = engine::validate_pipeline(&pipeline);
    if !issues.is_empty() {
        return Err(MfdError::UnsupportedImport(format!(
            "chained design does not form an executable pipeline: {}",
            issues
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }
    Ok(ImportedPipeline {
        pipeline,
        warnings,
        mapping_path,
    })
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
    Ok(resources)
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

struct PipelineIntermediate {
    label: String,
    key: u32,
    inputs: TargetInputCoverage,
}

struct DiscoveredPipelineChain {
    intermediates: Vec<PipelineIntermediate>,
    final_key: u32,
    final_inputs: TargetInputCoverage,
}

/// Raw connected target pins must survive boundary parsing. JSON's redundant
/// `null` alternative is the one intentional exception when a typed sibling
/// is also connected and represented by the imported target.
#[derive(Default)]
struct TargetInputCoverage {
    connected: BTreeSet<u32>,
    json_null_siblings: BTreeMap<u32, BTreeSet<u32>>,
}

impl TargetInputCoverage {
    fn unrepresented(&self, represented: &BTreeSet<u32>) -> Option<u32> {
        self.connected.iter().copied().find(|key| {
            !represented.contains(key)
                && !self.json_null_siblings.get(key).is_some_and(|siblings| {
                    siblings.iter().any(|sibling| {
                        self.connected.contains(sibling) && represented.contains(sibling)
                    })
                })
        })
    }
}

// Every stage reimports the design to retain the original component semantics.
// Bound that repeated work before lowering begins.
const MAX_IMPORTED_PIPELINE_INTERMEDIATES: usize = 64;

fn discover_pipeline_chain(path: &Path) -> Result<DiscoveredPipelineChain, MfdError> {
    let text = std::fs::read_to_string(path)?;
    discover_pipeline_chain_text(&text)
}

pub(crate) fn validate_pipeline_export_graph(xml: &str) -> Result<(), MfdError> {
    discover_pipeline_chain_text(xml)
        .map(|_| ())
        .map_err(|error| {
            MfdError::Unsupported(format!("pipeline export graph is not supported: {error}"))
        })
}

fn discover_pipeline_chain_text(text: &str) -> Result<DiscoveredPipelineChain, MfdError> {
    let doc = roxmltree::Document::parse(text)?;
    let mapping = doc.root_element();
    if !mapping.has_tag_name("mapping") {
        return Err(MfdError::NotMfd("root element is not <mapping>"));
    }
    let wrapper = mapping
        .children()
        .find(|node| node.has_tag_name("component"))
        .ok_or(MfdError::NotMfd("no wrapper component"))?;
    let structure = wrapper
        .children()
        .find(|node| node.has_tag_name("structure"))
        .ok_or(MfdError::NotMfd("wrapper has no structure"))?;
    let components = structure
        .children()
        .find(|node| node.has_tag_name("children"))
        .into_iter()
        .flat_map(|children| {
            children
                .children()
                .filter(|node| node.has_tag_name("component"))
        })
        .collect::<Vec<_>>();
    if let Some(component) = components.iter().find(|component| {
        !matches!(
            component.attribute("library"),
            Some("xml" | "core" | "lang" | "xpath2" | "text" | "json" | "xlsx")
        ) && !(component.attribute("library") == Some("binary")
            && is_protobuf_terminal_component(component))
            && !(component.attribute("library") == Some("xbrl")
                && is_xbrl_terminal_component(component))
    }) {
        return Err(MfdError::UnsupportedImport(format!(
            "pipeline import does not yet support `{}` components",
            component.attribute("library").unwrap_or_default()
        )));
    }
    let edge_from = read_edges(&structure, Some(&wrapper));
    let connected_outputs = edge_from.values().copied().collect::<BTreeSet<_>>();
    let connected_inputs = |component: &roxmltree::Node<'_, '_>| {
        component.descendants().any(|node| {
            node.has_tag_name("entry")
                && schema::parse_u32(node.attribute("inpkey"))
                    .is_some_and(|key| edge_from.contains_key(&key))
        })
    };
    let connected_component_outputs = |component: &roxmltree::Node<'_, '_>| {
        component.descendants().any(|node| {
            node.has_tag_name("entry")
                && schema::parse_u32(node.attribute("outkey"))
                    .is_some_and(|key| connected_outputs.contains(&key))
        })
    };
    let pass_through = |component: &&roxmltree::Node<'_, '_>| {
        component.attribute("library") == Some("xml")
            && component.children().any(|node| {
                node.has_tag_name("properties") && node.attribute("PassThrough") == Some("1")
            })
            && connected_inputs(component)
            && connected_component_outputs(component)
    };
    let intermediates = components.iter().filter(pass_through).collect::<Vec<_>>();
    if intermediates.is_empty() {
        return Err(MfdError::UnsupportedImport(
            "pipeline import currently needs one connected XML pass-through target".into(),
        ));
    }
    if intermediates.len() > MAX_IMPORTED_PIPELINE_INTERMEDIATES {
        return Err(MfdError::UnsupportedImport(format!(
            "pipeline import supports at most {MAX_IMPORTED_PIPELINE_INTERMEDIATES} intermediate targets"
        )));
    }
    let final_outputs = components
        .iter()
        .filter(|component| {
            (component.attribute("library") == Some("xml")
                || is_csv_terminal_component(component)
                || is_fixed_width_terminal_component(component)
                || is_flextext_terminal_component(component)
                || is_json_terminal_component(component)
                || is_protobuf_terminal_component(component)
                || is_xbrl_terminal_component(component)
                || is_xlsx_terminal_component(component))
                && component.children().any(|node| {
                    node.has_tag_name("properties")
                        && node.attribute("XSLTDefaultOutput") == Some("1")
                })
                && connected_inputs(component)
        })
        .collect::<Vec<_>>();
    let [final_target] = final_outputs.as_slice() else {
        return Err(MfdError::UnsupportedImport(
            "pipeline import currently needs one connected XML, CSV, fixed-width, FlexText, JSON, Protocol Buffers, XBRL, or XLSX final target"
                .into(),
        ));
    };
    let named_csv_targets = components
        .iter()
        .filter(|component| {
            final_target.attribute("library") == Some("xml")
                && component.id() != final_target.id()
                && is_csv_terminal_component(component)
                && connected_inputs(component)
                && !connected_component_outputs(component)
                && !component.children().any(|node| {
                    node.has_tag_name("properties")
                        && node.attribute("XSLTDefaultOutput") == Some("1")
                })
        })
        .collect::<Vec<_>>();
    let named_json_targets = components
        .iter()
        .filter(|component| {
            final_target.attribute("library") == Some("xml")
                && component.id() != final_target.id()
                && is_json_terminal_component(component)
                && component
                    .children()
                    .find(|node| node.has_tag_name("data"))
                    .and_then(|data| data.children().find(|node| node.has_tag_name("json")))
                    .is_some_and(|json| json.attribute("jsonlines").is_none())
                && connected_inputs(component)
                && !connected_component_outputs(component)
                && !component.children().any(|node| {
                    node.has_tag_name("properties")
                        && node.attribute("XSLTDefaultOutput") == Some("1")
                })
        })
        .collect::<Vec<_>>();
    if named_csv_targets.len() > 1 {
        return Err(MfdError::UnsupportedImport(
            "pipeline import supports at most one connected named CSV final target".into(),
        ));
    }
    if named_json_targets.len() > 1 {
        return Err(MfdError::UnsupportedImport(
            "pipeline import supports at most one connected named JSON final target".into(),
        ));
    }
    if !named_csv_targets.is_empty() && !named_json_targets.is_empty() {
        return Err(MfdError::UnsupportedImport(
            "pipeline import supports at most one non-XML named final target".into(),
        ));
    }
    if is_protobuf_terminal_component(final_target)
        && !protobuf_connected_inputs_match_message_boundary(final_target, &edge_from)
    {
        return Err(MfdError::UnsupportedImport(
            "Protocol Buffer final target has a connected input outside its message boundary"
                .into(),
        ));
    }
    if is_xlsx_terminal_component(final_target)
        && final_target.descendants().any(|node| {
            node.has_tag_name("excel") && node.attribute("updateexistingfile") == Some("1")
        })
    {
        return Err(MfdError::UnsupportedImport(
            "pipeline import requires a new-workbook XLSX final target".into(),
        ));
    }
    if components.iter().any(|component| {
        component.attribute("library") == Some("text")
            && (component.id() != final_target.id() && !named_csv_targets.contains(&component)
                || connected_component_outputs(component))
    }) {
        return Err(MfdError::UnsupportedImport(
            "pipeline import supports CSV, fixed-width text, and FlexText components only as the final primary target, except one named CSV target beside a primary XML target".into(),
        ));
    }
    if components.iter().any(|component| {
        component.attribute("library") == Some("json")
            && (component.id() != final_target.id() && !named_json_targets.contains(&component)
                || connected_component_outputs(component))
    }) {
        return Err(MfdError::UnsupportedImport(
            "pipeline import supports JSON components only as the final primary target, except one named JSON target beside a primary XML target".into(),
        ));
    }
    if components.iter().any(|component| {
        component.attribute("library") == Some("binary")
            && (component.id() != final_target.id() || connected_component_outputs(component))
    }) {
        return Err(MfdError::UnsupportedImport(
            "pipeline import supports Protocol Buffer components only as the final primary target"
                .into(),
        ));
    }
    if components.iter().any(|component| {
        component.attribute("library") == Some("xbrl")
            && (component.id() != final_target.id() || connected_component_outputs(component))
    }) {
        return Err(MfdError::UnsupportedImport(
            "pipeline import supports XBRL components only as the final primary target".into(),
        ));
    }
    if components.iter().any(|component| {
        component.attribute("library") == Some("xlsx")
            && (component.id() != final_target.id() || connected_component_outputs(component))
    }) {
        return Err(MfdError::UnsupportedImport(
            "pipeline import supports XLSX components only as the final primary target".into(),
        ));
    }
    let terminal_targets = components
        .iter()
        .filter(|component| {
            (component.attribute("library") == Some("xml")
                || named_csv_targets.contains(component)
                || named_json_targets.contains(component)
                || component.id() == final_target.id()
                    && (is_csv_terminal_component(component)
                        || is_fixed_width_terminal_component(component)
                        || is_flextext_terminal_component(component)
                        || is_json_terminal_component(component)
                        || is_protobuf_terminal_component(component)
                        || is_xbrl_terminal_component(component)
                        || is_xlsx_terminal_component(component)))
                && connected_inputs(component)
                && !component.children().any(|node| {
                    node.has_tag_name("properties") && node.attribute("PassThrough") == Some("1")
                })
        })
        .collect::<Vec<_>>();
    if terminal_targets.len() > 256 {
        return Err(MfdError::UnsupportedImport(
            "pipeline import supports at most 256 final targets".into(),
        ));
    }
    let connected_input_key = |component: &roxmltree::Node<'_, '_>| {
        component.descendants().find_map(|node| {
            node.has_tag_name("entry")
                .then(|| schema::parse_u32(node.attribute("inpkey")))
                .flatten()
                .filter(|key| edge_from.contains_key(key))
        })
    };
    let intermediate_keys = intermediates
        .iter()
        .map(|intermediate| {
            connected_input_key(intermediate).ok_or_else(|| {
                MfdError::UnsupportedImport("intermediate target has no connected input".into())
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let final_key = connected_input_key(final_target)
        .ok_or_else(|| MfdError::UnsupportedImport("final target has no connected input".into()))?;
    let mut final_inputs = TargetInputCoverage::default();
    for terminal in &terminal_targets {
        let coverage = target_input_coverage(terminal, &edge_from);
        final_inputs.connected.extend(coverage.connected);
        final_inputs
            .json_null_siblings
            .extend(coverage.json_null_siblings);
    }
    let mut consumers = BTreeMap::<u32, Vec<u32>>::new();
    for (&input, &output) in &edge_from {
        consumers.entry(output).or_default().push(input);
    }
    let mut function_outputs = BTreeMap::<u32, Vec<u32>>::new();
    for component in &components {
        if matches!(
            component.attribute("library"),
            Some("xml" | "text" | "json" | "binary" | "xbrl" | "xlsx")
        ) {
            continue;
        }
        let outputs = component
            .children()
            .find(|node| node.has_tag_name("targets"))
            .into_iter()
            .flat_map(|node| {
                node.descendants()
                    .filter(|node| node.has_tag_name("datapoint"))
            })
            .filter_map(|node| schema::parse_u32(node.attribute("key")))
            .collect::<Vec<_>>();
        for input in component
            .children()
            .find(|node| node.has_tag_name("sources"))
            .into_iter()
            .flat_map(|node| {
                node.descendants()
                    .filter(|node| node.has_tag_name("datapoint"))
            })
            .filter_map(|node| schema::parse_u32(node.attribute("key")))
        {
            function_outputs.insert(input, outputs.clone());
        }
    }
    for component in &components {
        if component.attribute("library") == Some("core")
            && component.attribute("kind") == Some("7")
        {
            return Err(MfdError::UnsupportedImport(
                "pipeline import does not yet support standalone output parameters".into(),
            ));
        }
        if component.attribute("library") != Some("xml") {
            continue;
        }
        if intermediates.contains(&component) || terminal_targets.contains(&component) {
            continue;
        }
        if component.children().any(|node| {
            node.has_tag_name("properties") && node.attribute("PassThrough") == Some("1")
        }) || connected_inputs(component)
            || !connected_component_outputs(component)
        {
            return Err(MfdError::UnsupportedImport(format!(
                "pipeline import cannot classify component `{}` as an original source",
                component.attribute("name").unwrap_or_default()
            )));
        }
    }
    let original_sources = components
        .iter()
        .filter(|component| {
            component.attribute("library") == Some("xml")
                && !intermediates.contains(component)
                && !terminal_targets.contains(component)
                && connected_component_outputs(component)
        })
        .count();
    if original_sources == 0 {
        return Err(MfdError::UnsupportedImport(
            "pipeline import needs an XML source component".into(),
        ));
    }
    let order = strict_serial_stage_order(
        &components,
        &intermediates,
        &terminal_targets,
        &connected_outputs,
        &consumers,
        &function_outputs,
    )?;
    Ok(DiscoveredPipelineChain {
        intermediates: order
            .into_iter()
            .map(|index| PipelineIntermediate {
                label: intermediates[index]
                    .attribute("name")
                    .unwrap_or_default()
                    .to_string(),
                key: intermediate_keys[index],
                inputs: target_input_coverage(intermediates[index], &edge_from),
            })
            .collect(),
        final_key,
        final_inputs,
    })
}

fn target_input_coverage(
    component: &roxmltree::Node<'_, '_>,
    edge_from: &BTreeMap<u32, u32>,
) -> TargetInputCoverage {
    let connected = entry_keys(component, "inpkey")
        .into_iter()
        .filter(|key| edge_from.contains_key(key))
        .collect();
    let json_null_siblings = if is_json_terminal_component(component) {
        selected_json_payload(component)
            .into_iter()
            .flat_map(|payload| payload.descendants())
            .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some("null"))
            .filter_map(|null| {
                let key = schema::parse_u32(null.attribute("inpkey"))?;
                let siblings = null
                    .parent()?
                    .children()
                    .filter(|sibling| {
                        sibling.has_tag_name("entry")
                            && sibling.id() != null.id()
                            && sibling.attribute("name") != Some("null")
                    })
                    .filter_map(|sibling| schema::parse_u32(sibling.attribute("inpkey")))
                    .collect::<BTreeSet<_>>();
                (!siblings.is_empty()).then_some((key, siblings))
            })
            .collect()
    } else {
        BTreeMap::new()
    };
    TargetInputCoverage {
        connected,
        json_null_siblings,
    }
}

fn selected_json_payload<'a, 'input>(
    component: &roxmltree::Node<'a, 'input>,
) -> Option<roxmltree::Node<'a, 'input>> {
    let root = component
        .children()
        .find(|node| node.has_tag_name("data"))?
        .children()
        .find(|node| node.has_tag_name("root"))?;
    let mut entry = root.children().find(|node| node.has_tag_name("entry"))?;
    while matches!(entry.attribute("name"), Some("FileInstance" | "document")) {
        entry = entry.children().find(|node| node.has_tag_name("entry"))?;
    }
    Some(entry)
}

fn entry_keys(component: &roxmltree::Node<'_, '_>, attribute: &str) -> BTreeSet<u32> {
    component
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
        .filter_map(|node| schema::parse_u32(node.attribute(attribute)))
        .collect()
}

fn is_csv_terminal_component(component: &roxmltree::Node<'_, '_>) -> bool {
    component.attribute("library") == Some("text")
        && component.attribute("kind") == Some("16")
        && component
            .children()
            .find(|node| node.has_tag_name("data"))
            .is_some_and(|data| {
                data.children()
                    .any(|node| node.has_tag_name("text") && node.attribute("type") == Some("csv"))
            })
}

fn is_fixed_width_terminal_component(component: &roxmltree::Node<'_, '_>) -> bool {
    component.attribute("library") == Some("text")
        && component.attribute("kind") == Some("16")
        && component
            .children()
            .find(|node| node.has_tag_name("data"))
            .is_some_and(|data| {
                data.children()
                    .any(|node| node.has_tag_name("text") && node.attribute("type") == Some("flf"))
            })
}

fn is_flextext_terminal_component(component: &roxmltree::Node<'_, '_>) -> bool {
    component.attribute("library") == Some("text")
        && component.attribute("kind") == Some("16")
        && component
            .children()
            .find(|node| node.has_tag_name("data"))
            .is_some_and(|data| {
                data.children().any(|node| {
                    node.has_tag_name("text")
                        && node.attribute("type") == Some("txt")
                        && node
                            .attribute("config")
                            .is_some_and(|config| !config.trim().is_empty())
                })
            })
}

fn is_json_terminal_component(component: &roxmltree::Node<'_, '_>) -> bool {
    component.attribute("library") == Some("json")
        && component.attribute("kind") == Some("31")
        && component
            .children()
            .find(|node| node.has_tag_name("data"))
            .is_some_and(|data| data.children().any(|node| node.has_tag_name("json")))
}

fn is_protobuf_terminal_component(component: &roxmltree::Node<'_, '_>) -> bool {
    component.attribute("library") == Some("binary")
        && component.attribute("kind") == Some("33")
        && component
            .children()
            .find(|node| node.has_tag_name("data"))
            .is_some_and(|data| {
                data.children().any(|node| node.has_tag_name("binary"))
                    && data.descendants().any(|node| {
                        node.has_tag_name("entry") && node.attribute("type") == Some("doc-protobuf")
                    })
            })
}

fn is_xbrl_terminal_component(component: &roxmltree::Node<'_, '_>) -> bool {
    component.attribute("library") == Some("xbrl")
        && component.attribute("kind") == Some("27")
        && component
            .children()
            .find(|node| node.has_tag_name("data"))
            .is_some_and(|data| {
                data.children()
                    .any(|node| node.has_tag_name("xbrl") && node.attribute("sps").is_none())
            })
}

fn protobuf_connected_inputs_match_message_boundary(
    component: &roxmltree::Node<'_, '_>,
    edge_from: &BTreeMap<u32, u32>,
) -> bool {
    let Some(root) = component
        .children()
        .find(|node| node.has_tag_name("data"))
        .and_then(|data| data.children().find(|node| node.has_tag_name("root")))
    else {
        return false;
    };
    let documents = root
        .descendants()
        .filter(|node| node.has_tag_name("entry") && node.attribute("type") == Some("doc-protobuf"))
        .collect::<Vec<_>>();
    let [document] = documents.as_slice() else {
        return false;
    };
    let Some(payload) = document.children().find(|node| node.has_tag_name("entry")) else {
        return false;
    };
    let canonical = std::iter::once(*document)
        .chain(document.ancestors().take_while(|node| *node != root))
        .filter(|node| node.has_tag_name("entry"))
        .chain(
            std::iter::once(payload)
                .chain(payload.descendants())
                .filter(|node| node.has_tag_name("entry")),
        )
        .map(|node| node.id())
        .collect::<Vec<_>>();
    !component.descendants().any(|node| {
        node.has_tag_name("entry")
            && schema::parse_u32(node.attribute("inpkey"))
                .is_some_and(|key| edge_from.contains_key(&key))
            && !canonical.contains(&node.id())
    })
}

fn is_xlsx_terminal_component(component: &roxmltree::Node<'_, '_>) -> bool {
    component.attribute("library") == Some("xlsx")
        && component.attribute("kind") == Some("26")
        && component
            .children()
            .find(|node| node.has_tag_name("data"))
            .is_some_and(|data| data.children().any(|node| node.has_tag_name("excel")))
}

/// Keep imported stages to one linear chain of XML intermediates, allowing the
/// last intermediate to feed multiple final targets. Multiple field connections
/// to the same next boundary are valid; an earlier connection to a final target
/// would read a bypassed stage value. Original host sources may supplement any
/// stage.
fn strict_serial_stage_order(
    components: &[roxmltree::Node<'_, '_>],
    intermediates: &[&roxmltree::Node<'_, '_>],
    terminal_targets: &[&roxmltree::Node<'_, '_>],
    connected_outputs: &BTreeSet<u32>,
    consumers: &BTreeMap<u32, Vec<u32>>,
    function_outputs: &BTreeMap<u32, Vec<u32>>,
) -> Result<Vec<usize>, MfdError> {
    let intermediate_indices = intermediates
        .iter()
        .map(|intermediate| {
            components
                .iter()
                .position(|component| component.id() == intermediate.id())
                .expect("selected intermediate belongs to the mapping")
        })
        .collect::<Vec<_>>();
    let terminal_indices = terminal_targets
        .iter()
        .map(|target| {
            components
                .iter()
                .position(|component| component.id() == target.id())
                .expect("selected final target belongs to the mapping")
        })
        .collect::<BTreeSet<_>>();
    let mut target_input_owners = BTreeMap::new();
    for (index, component) in components.iter().enumerate() {
        if component.attribute("library") != Some("xml")
            && !(terminal_indices.contains(&index)
                && (is_csv_terminal_component(component)
                    || is_fixed_width_terminal_component(component)
                    || is_flextext_terminal_component(component)
                    || is_json_terminal_component(component)
                    || is_protobuf_terminal_component(component)
                    || is_xbrl_terminal_component(component)
                    || is_xlsx_terminal_component(component)))
        {
            continue;
        }
        for key in entry_keys(component, "inpkey") {
            if target_input_owners.insert(key, index).is_some() {
                return Err(MfdError::UnsupportedImport(
                    "serial pipeline has duplicate target input port identities".into(),
                ));
            }
        }
    }
    let mut sinks_by_component = BTreeMap::new();
    for (index, component) in components.iter().enumerate() {
        if component.attribute("library") != Some("xml") {
            continue;
        }
        let outputs = entry_keys(component, "outkey")
            .into_iter()
            .filter(|key| connected_outputs.contains(key))
            .collect::<Vec<_>>();
        if outputs.is_empty() {
            continue;
        }
        let sinks = trace_xml_sinks(
            outputs,
            connected_outputs,
            consumers,
            function_outputs,
            &target_input_owners,
        )?;
        if sinks.is_empty() {
            return Err(MfdError::UnsupportedImport(
                "serial pipeline has a connected XML output with no downstream target".into(),
            ));
        }
        sinks_by_component.insert(index, sinks);
    }
    if terminal_indices
        .iter()
        .any(|index| sinks_by_component.contains_key(index))
    {
        return Err(MfdError::UnsupportedImport(
            "serial pipeline final target feeds another component".into(),
        ));
    }
    let invalid_chain = || {
        MfdError::UnsupportedImport(
            "pipeline needs a serial XML chain without branches, cycles, or bypasses".into(),
        )
    };
    if !components.iter().enumerate().all(|(index, component)| {
        component.attribute("library") != Some("xml")
            || intermediate_indices.contains(&index)
            || terminal_indices.contains(&index)
            || sinks_by_component.get(&index).is_some_and(|sinks| {
                sinks.iter().all(|sink| {
                    intermediate_indices.contains(sink) || terminal_indices.contains(sink)
                })
            })
    }) {
        return Err(invalid_chain());
    }

    let mut successors = BTreeMap::new();
    let mut predecessor_counts = BTreeMap::<usize, usize>::new();
    for &index in &intermediate_indices {
        let sinks = sinks_by_component.get(&index).ok_or_else(invalid_chain)?;
        let next = if sinks == &terminal_indices {
            None
        } else {
            let mut remaining = sinks.iter();
            let (Some(&next), None) = (remaining.next(), remaining.next()) else {
                return Err(invalid_chain());
            };
            if !intermediate_indices.contains(&next) {
                return Err(invalid_chain());
            }
            Some(next)
        };
        successors.insert(index, next);
        if let Some(next) = next {
            *predecessor_counts.entry(next).or_default() += 1;
        }
    }
    let heads = intermediate_indices
        .iter()
        .copied()
        .filter(|index| !predecessor_counts.contains_key(index))
        .collect::<Vec<_>>();
    let [head] = heads.as_slice() else {
        return Err(invalid_chain());
    };
    let mut current = *head;
    let mut seen = BTreeSet::new();
    let mut ordered = Vec::with_capacity(intermediate_indices.len());
    while seen.insert(current) {
        ordered.push(
            intermediate_indices
                .iter()
                .position(|&index| index == current)
                .expect("intermediate belongs to the chain"),
        );
        let next = successors[&current];
        let Some(next) = next else {
            return (ordered.len() == intermediate_indices.len())
                .then_some(ordered)
                .ok_or_else(invalid_chain);
        };
        current = next;
    }
    Err(invalid_chain())
}

fn trace_xml_sinks(
    outputs: Vec<u32>,
    connected_outputs: &BTreeSet<u32>,
    consumers: &BTreeMap<u32, Vec<u32>>,
    function_outputs: &BTreeMap<u32, Vec<u32>>,
    target_input_owners: &BTreeMap<u32, usize>,
) -> Result<BTreeSet<usize>, MfdError> {
    let mut sinks = BTreeSet::new();
    let mut active = BTreeSet::new();
    let mut done = BTreeSet::new();
    let mut stack = outputs
        .into_iter()
        .map(|output| (output, false))
        .collect::<Vec<_>>();
    while let Some((output, leaving)) = stack.pop() {
        if leaving {
            active.remove(&output);
            done.insert(output);
            continue;
        }
        if done.contains(&output) {
            continue;
        }
        if !active.insert(output) {
            return Err(MfdError::UnsupportedImport(
                "serial pipeline contains a function-feed cycle".into(),
            ));
        }
        if active.len() + done.len() > 65_536 {
            return Err(MfdError::UnsupportedImport(
                "serial pipeline exceeds 65536 connected output ports".into(),
            ));
        }
        stack.push((output, true));
        for input in consumers.get(&output).into_iter().flatten() {
            if let Some(&owner) = target_input_owners.get(input) {
                sinks.insert(owner);
            } else if let Some(next_outputs) = function_outputs.get(input) {
                let next_outputs = next_outputs
                    .iter()
                    .copied()
                    .filter(|key| connected_outputs.contains(key))
                    .collect::<Vec<_>>();
                if next_outputs.is_empty() {
                    return Err(MfdError::UnsupportedImport(
                        "serial pipeline has a function branch without a downstream target".into(),
                    ));
                }
                stack.extend(next_outputs.into_iter().map(|next| (next, false)));
            } else {
                return Err(MfdError::UnsupportedImport(
                    "serial pipeline has a feed to an unclassified component".into(),
                ));
            }
        }
    }
    Ok(sinks)
}

fn import_resolved(
    resources: &ResourceResolver,
    selection: StageSelection<'_>,
) -> Result<LoweredStage, MfdError> {
    let path = resources.mapping_path();
    let text = std::fs::read_to_string(path)?;
    let doc = roxmltree::Document::parse(&text)?;
    let mapping_el = doc.root_element();
    if mapping_el.tag_name().name() != "mapping" {
        return Err(MfdError::NotMfd("root element is not <mapping>"));
    }
    let wrapper = mapping_el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "component")
        .ok_or(MfdError::NotMfd("no wrapper component"))?;
    let structure = wrapper
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "structure")
        .ok_or(MfdError::NotMfd("wrapper has no structure"))?;
    let primary_source_hint = primary_source_hint(&mapping_el, &structure);
    let scope_source_hints = scope_source_hints(&structure);
    let selected_language = wrapper
        .children()
        .find(|node| node.has_tag_name("properties"))
        .and_then(|properties| properties.attribute("SelectedLanguage"))
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut warnings = Vec::new();
    let mut schema_components = Vec::new();
    let mut fn_components = Vec::new();
    let mut json_serializers = Vec::new();
    let mut xml_serializers = Vec::new();
    let mut json_parsers = Vec::new();
    let mut flextext_parsers = Vec::new();
    let mut output_parameters = Vec::new();
    let mut udf_registry = UdfRegistry::read(&mapping_el, path, &mut warnings);
    let mut udf_calls = Vec::new();
    let mut external_udf_candidates = Vec::new();
    let mut external_scalar_recipes = Vec::new();
    let mut external_xslt_aggregates = Vec::new();
    let mut exception_recipes = Vec::new();
    let mut pending_joins = join::PendingJoins::default();
    let mut skipped_libraries: Vec<String> = Vec::new();
    let mut fallback_xml_source_outputs = BTreeSet::new();
    let mut fallback_xml_target_inputs = BTreeSet::new();
    let source_node_functions = source_node_function::read(&mapping_el);

    if let Some(children) = structure
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "children")
    {
        for component in children
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "component")
        {
            let library = component.attribute("library").unwrap_or_default();
            let name = component.attribute("name").unwrap_or_default().to_string();
            match library {
                "xml" => {
                    match read_schema_component_in_package_with_provenance(
                        &component,
                        resources,
                        &mut warnings,
                    ) {
                        Some(read) => {
                            let sc = read.component;
                            let mut retain = |sc: SchemaComponent| {
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
                        None => warnings.push(format!("skipped xml component `{name}`")),
                    }
                }
                "json" => {
                    match read_json_component_in_package(&component, resources, &mut warnings) {
                        Some(sc) => match json_serializer::read(&component, &sc) {
                            Ok(Some(serializer)) => json_serializers.push(serializer),
                            Ok(None) => match json_parser::read(&component, &sc) {
                                Ok(Some(parser)) => json_parsers.push(parser),
                                Ok(None) => schema_components.push(sc),
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
                        },
                        None => warnings.push(format!("skipped json component `{name}`")),
                    }
                }
                "xlsx" if component.attribute("kind") == Some("26") => {
                    match read_xlsx_component(&component, &mut warnings) {
                        Some(sc) => schema_components.push(sc),
                        None => {
                            note_skipped_library(&mut skipped_libraries, "xlsx");
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
                            Some(sc) => schema_components.push(sc),
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
                    exception_recipes.push(exception::read(&component));
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
                    } else {
                        match external_scalar::read(&component, path, &selected_language) {
                            Ok(Some(recipe)) => external_scalar_recipes.push(recipe),
                            Err(reason) => {
                                note_skipped_library(&mut skipped_libraries, other);
                                warnings.push(format!(
                                    "external {selected_language} function `{name}` is unsupported: {reason}"
                                ));
                            }
                            Ok(None) => match external_xslt::read(&component, path) {
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
    let user_functions = udf_registry.user_functions();
    // UDF-owned static catalogs are secondary to ordinary mapping sources.
    // Keeping them last also preserves the document source as the default in
    // scalar-only mappings, where repetition-based primary scoring is tied.
    schema_components.extend(udf_registry.take_sources());

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

    // Edges are indexed as to-key -> from-key; each input has at most one feed.
    let edge_from = read_edges(&structure, Some(&wrapper));
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
    refine_database_roles(&mut schema_components, &edge_from);
    udf::refine_source_schemas(
        &mut schema_components,
        &udf_calls,
        &udf_registry,
        &edge_from,
    );
    refine_wsdl_target_schemas(&mut schema_components, &fn_components, &edge_from);
    let copy_all_targets = read_copy_all_targets(&structure, Some(&wrapper));
    refine_copied_fallback_source_shapes(
        &mut schema_components,
        &edge_from,
        &copy_all_targets,
        &fallback_xml_source_outputs,
        &fallback_xml_target_inputs,
        &mut warnings,
    );
    refine_copied_fallback_target_groups(
        &mut schema_components,
        &edge_from,
        &copy_all_targets,
        &fallback_xml_source_outputs,
        &fallback_xml_target_inputs,
        &mut warnings,
    );
    schema::restore_connected_structural_ports(&mut schema_components, &edge_from);
    refine_copied_json_root_schemas(&mut schema_components, &edge_from, &copy_all_targets);

    let output_failed = output_parameter::install_fallback(
        &mut schema_components,
        output_parameters,
        &edge_from,
        &mut warnings,
    );

    let target_inputs =
        external_udf::selected_target_inputs(&schema_components).ok_or_else(|| {
            output_parameter::missing_error("target", &skipped_libraries, output_failed)
        })?;
    external_udf::install_fallback(
        &mut schema_components,
        external_udf_candidates,
        &target_inputs,
        &edge_from,
        &fn_components,
        &mut warnings,
    );

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
    let unsupported =
        |side: &str| output_parameter::missing_error(side, &skipped_libraries, output_failed);
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
            .unwrap_or_else(|| primary_index(&sources, target, &edge_from, &fn_components)),
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
    let joins = pending_joins.resolve(&edge_from, &sources, &source_names, &mut warnings);

    let xml_type_conditions = alternatives::conditioned_port_types(&structure);
    let mut builder = GraphBuilder {
        graph: Graph::default(),
        next_id: 0,
        fn_nodes: BTreeMap::new(),
        sequence_items: BTreeMap::new(),
        sequence_scope_components: BTreeSet::new(),
        sequence_predicate_components: BTreeSet::new(),
        warned_sequence_uses: BTreeSet::new(),
        warned_scalar_filters: BTreeSet::new(),
        warned_join_controls: BTreeSet::new(),
        warned_variable_constructions: BTreeSet::new(),
        rejected_join_paths: BTreeSet::new(),
        source_fields: BTreeMap::new(),
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
        edge_from: &edge_from,
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

    // Dynamic document inputs must establish their driver frames before any
    // target expression is materialized. A filename expression can also feed
    // an ordinary target binding, and SourceField nodes created there need the
    // same framed suffix as the loader path expression.
    let mut dynamic_source_inputs: Vec<Option<(u32, SourcePath)>> = vec![None; sources.len()];
    for (index, extra) in sources.iter().enumerate().skip(1) {
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

    let mut root = build_target_scope(
        &mapping_el,
        target,
        &structure,
        resources,
        &edge_from,
        &copy_all_targets,
        &mut builder,
    );
    restore_scope_source_hints(&mut root, target, &scope_source_hints);
    let mut extra_targets = Vec::new();
    for (index, extra) in connected_targets.iter().copied().enumerate().skip(1) {
        let mut extra_root = build_target_scope(
            &mapping_el,
            extra,
            &structure,
            resources,
            &edge_from,
            &copy_all_targets,
            &mut builder,
        );
        restore_scope_source_hints(&mut extra_root, extra, &scope_source_hints);
        let extra_path = if extra_root.output_path().is_some() {
            None
        } else {
            extra
                .output_instance
                .clone()
                .or_else(|| extra.input_instance.clone())
                .or_else(|| default_pass_through_output_path(extra))
        };
        extra_targets.push(NamedTarget {
            name: target_names[index].clone(),
            path: extra_path,
            schema: runtime_target_schema(extra, &edge_from),
            options: extra.options.clone(),
            root: extra_root,
        });
    }

    let mut extra_sources = Vec::new();
    for (index, extra) in sources.iter().enumerate().skip(1) {
        let dynamic_path = dynamic_source_inputs[index]
            .as_ref()
            .and_then(|(feed, driver)| {
                builder
                    .binding_node(*feed, &[])
                    .map(|node| mapping::DynamicSourcePath {
                        node,
                        iteration: builder.context_path(driver),
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
                        instance_path::resolve_static_input(path, stored)
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
        .map(|stored| instance_path::resolve_static_input(path, &stored));
    let target_path = if root.output_path().is_some() {
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
        target
            .output_instance
            .clone()
            .or_else(|| target.input_instance.clone())
            .or_else(|| builder.static_target_document_path(target))
            .or_else(|| default_pass_through_output_path(target))
    };
    let failure_rules = exception::lower(exception_recipes, &mut builder);
    warnings.extend(builder.warnings);
    let mut project = Project {
        source: primary.schema.clone(),
        target: runtime_target_schema(target, &edge_from),
        source_path,
        target_path,
        source_options: primary.options.clone(),
        target_options: target.options.clone(),
        extra_sources,
        extra_targets,
        failure_rules,
        user_functions,
        graph: builder.graph,
        root,
    };
    project.prune_unreachable_nodes();
    enrich_unresolved_edi_source_schemas(&mut project);
    Ok(LoweredStage {
        imported: Imported {
            project,
            warnings,
            mapping_path: path.to_path_buf(),
        },
        source_components,
    })
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

fn build_target_scope(
    mapping: &roxmltree::Node<'_, '_>,
    target: &SchemaComponent,
    structure: &roxmltree::Node<'_, '_>,
    resources: &ResourceResolver,
    edge_from: &BTreeMap<u32, u32>,
    copy_all_targets: &BTreeSet<u32>,
    builder: &mut GraphBuilder<'_>,
) -> Scope {
    builder.rejected_join_paths.clear();
    let mut scopes = ScopeBuilder {
        root: Scope::default(),
        anchors: BTreeMap::new(),
    };
    let dynamic_document = target
        .ports
        .iter()
        .find(|(_, path)| path.as_slice() == [schema::TARGET_DOCUMENT_PATH_PORT])
        .and_then(|(input, _)| edge_from.get(input).copied())
        .and_then(|feed| {
            if builder.static_string_feed(feed).is_some() {
                return None;
            }
            let driver = builder.computed_iteration_source(feed);
            match driver {
                Some(driver) => {
                    builder.note_framed_prefixes(&driver);
                    let context = builder.context_path(&driver);
                    scopes.add_iteration(
                        &[],
                        &context,
                        scope::IterationNodes::default(),
                        mapping::IterationOutput::Repeated,
                    );
                    Some(feed)
                }
                None => None,
            }
        });
    let dynamic_target = dynamic_json::prepare_target(target, builder);
    let mut iterations = Vec::new();
    let mut bindings = Vec::new();
    let mut group_projections = Vec::new();
    let mut structured_udf_targets = Vec::new();
    let mut csv_singleton_bindings = BTreeMap::new();
    for (&inpkey, target_path) in &target.ports {
        let Some(&from) = edge_from.get(&inpkey) else {
            continue;
        };
        if target_path.as_slice() == [schema::TARGET_DOCUMENT_PATH_PORT] {
            continue;
        }
        if let Some((position, field)) = schema::split_singleton_port(target_path) {
            csv_singleton_bindings
                .entry(position)
                .or_insert_with(Vec::new)
                .push((field.to_string(), from));
            continue;
        }
        let node_kind = schema_node_at(&target.schema, target_path);
        if let Some(node) = node_kind
            && recursive::accept_target(target_path, node, from, builder, &mut scopes)
        {
            continue;
        }
        match node_kind {
            Some(node) if matches!(node.kind, SchemaKind::Group { .. }) => {
                if udf::structured::accept_target(target, target_path, node, inpkey, from, builder)
                {
                    structured_udf_targets.push((target_path.clone(), from));
                    continue;
                }
                group_projection::classify_target_connection(
                    target,
                    group_projection::TargetConnection {
                        target_path,
                        target_node: node,
                        input_key: inpkey,
                        feed: from,
                        copy_all_targets,
                    },
                    builder,
                    &mut iterations,
                    &mut group_projections,
                )
            }
            Some(_) => match TargetLeaf::from_path(target_path) {
                Some(target) => bindings.push((target, from, inpkey)),
                None => builder.warnings.push(
                    "connection into a scalar document root is not supported; binding skipped"
                        .to_string(),
                ),
            },
            None => builder.warnings.push(format!(
                "target port path `{}` not found in schema",
                target_path.join("/")
            )),
        }
    }
    order_repeating_scalar_bindings(target, &mut bindings);
    udf::structured::prepare_target_frames(&structured_udf_targets, builder);
    generated_occurrence::infer(target, builder, &mut iterations);
    iterations.sort_by_key(|iteration| iteration.target_path.len());
    let explicit_iteration_paths = iterations
        .iter()
        .map(|iteration| iteration.target_path.clone())
        .collect::<BTreeSet<_>>();
    join::prepare_iterations(&iterations, builder, &mut scopes);
    for iteration in &iterations {
        for feed_key in std::iter::once(iteration.feed)
            .chain(iteration.additional_feeds.iter().map(|(feed, _, _)| *feed))
        {
            let feed = builder.resolve_iteration_feed(feed_key);
            if let Some(idx) = feed.sequence_component {
                builder.sequence_scope_components.insert(idx);
            }
            if let Some(source_path) = builder.iteration_source_path(&feed) {
                builder.note_framed_prefixes(&source_path);
            }
        }
    }
    materialize::eager_functions(builder);
    if let Some(feed) = dynamic_document
        && let Some(node) = builder.binding_node_at_anchor(feed, &[], &[])
        && !scopes.root.set_output_path(Some(node))
    {
        builder.warnings.push(
            "target FileInstance path conflicts with another root iteration; dynamic document output was skipped"
                .to_string(),
        );
    }
    let mut skipped_iteration_paths =
        target_iteration::build(iterations, target, &mut bindings, builder, &mut scopes);
    protobuf_target::infer_singleton_messages(
        target,
        &bindings,
        &explicit_iteration_paths,
        builder,
        &mut scopes,
        &mut skipped_iteration_paths,
    );
    let structured_udf_paths = structured_udf_targets
        .iter()
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
    udf::structured::build_targets(
        structured_udf_targets,
        target,
        builder,
        &mut scopes,
        &mut skipped_iteration_paths,
    );
    group_projection::build(
        group_projections,
        target,
        &skipped_iteration_paths,
        builder,
        &mut scopes,
    );
    target_mixed_content::install(target, builder, &mut scopes);
    for (target_leaf, from, input) in bindings {
        let target_path = target_leaf.path();
        if builder.join_dependency_rejected(from) {
            continue;
        }
        if structured_udf_paths
            .iter()
            .any(|path| target_path.starts_with(path))
            && builder.is_structured_recipe(from)
        {
            continue;
        }
        if skipped_iteration_paths
            .iter()
            .any(|path| target_path.starts_with(path))
        {
            continue;
        }
        if install_repeating_scalar_iteration(target, &target_leaf, from, builder, &mut scopes) {
            continue;
        }
        let active_anchor = scopes.enclosing_anchor(&target_path);
        let node = match target.db_xml_columns.get(&input) {
            Some(column) => builder.database_xml_column_node(from, column),
            None => builder.binding_node_at_anchor(from, &target_path, &active_anchor),
        };
        let Some(node) = node else {
            continue;
        };
        let node = if scopes
            .scope(target_leaf.chain())
            .is_some_and(|scope| scope.post_group_filter.is_some())
        {
            builder.group_member_value(node)
        } else {
            node
        };
        scopes.add_binding(target_leaf, node);
    }
    dynamic_json::build_target(dynamic_target, target, builder, &mut scopes);
    compose_csv_target_rows(csv_singleton_bindings, builder, &mut scopes);
    target_node_default::install(target, structure, builder, &mut scopes);
    target_node_function::install(mapping, target, structure, resources, builder, &mut scopes);
    target_type_cast::install(target, structure, resources, builder, &mut scopes);
    group_projection::install_optional_text_occurrences(target, builder, &mut scopes);
    ensure_singleton_csv_row_set(target, &mut scopes);
    scopes.root
}

fn ensure_singleton_csv_row_set(target: &SchemaComponent, scopes: &mut ScopeBuilder) {
    let has_row_port = target.ports.values().any(Vec::is_empty);
    if target.format != ComponentFormat::Csv || has_row_port || scopes.root.iterates() {
        return;
    }
    let row = std::mem::take(&mut scopes.root);
    scopes.root.iteration = ScopeIteration::Concatenate(ScopeSequence::new(row, Vec::new()));
}

fn install_repeating_scalar_iteration(
    target_component: &SchemaComponent,
    target: &TargetLeaf,
    feed: u32,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) -> bool {
    let target_path = target.path();
    if !schema_node_at(&target_component.schema, &target_path)
        .is_some_and(|node| node.repeating && node.is_scalar())
    {
        return false;
    }
    let Some(source_path) = builder
        .source_abs_path(feed)
        .map(|path| builder.source_value_path(path.source, path.path))
    else {
        return false;
    };
    if !builder
        .schema_node(&source_path)
        .is_some_and(|node| node.repeating && node.is_scalar())
    {
        return false;
    }
    let source_abs = builder.context_path(&source_path);
    builder.note_framed_prefixes(&source_path);
    let value = builder.source_field(Some(source_abs.clone()), Vec::new());
    scopes.add_iteration(
        &target_path,
        &source_abs,
        scope::IterationNodes::default(),
        mapping::IterationOutput::Repeated,
    );
    scopes.ensure_scope(&target_path).construction = mapping::ScopeConstruction::Scalar { value };
    true
}

/// Repeated scalar target entries can be cloned several times under the same
/// schema path. Their numeric pin keys are identifiers, not an occurrence
/// order, so preserve the entry-tree branch order recorded by the schema
/// reader before the bindings are distributed into concatenated scopes.
fn order_repeating_scalar_bindings(
    target: &SchemaComponent,
    bindings: &mut [(TargetLeaf, u32, u32)],
) {
    let mut positions = BTreeMap::<Vec<String>, Vec<usize>>::new();
    for (index, (binding, _, _)) in bindings.iter().enumerate() {
        let path = binding.path();
        if schema_node_at(&target.schema, &path)
            .is_some_and(|node| node.repeating && node.is_scalar())
        {
            positions.entry(path).or_default().push(index);
        }
    }
    for positions in positions.values().filter(|positions| positions.len() > 1) {
        let mut ordered = positions
            .iter()
            .map(|index| bindings[*index].clone())
            .collect::<Vec<_>>();
        ordered.sort_by(|left, right| {
            target
                .input_ancestors
                .get(&left.2)
                .cmp(&target.input_ancestors.get(&right.2))
                .then_with(|| left.2.cmp(&right.2))
        });
        for (position, binding) in positions.iter().copied().zip(ordered) {
            bindings[position] = binding;
        }
    }
}

fn compose_csv_target_rows(
    singleton_bindings: BTreeMap<schema::CsvSingletonPosition, Vec<(String, u32)>>,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) {
    if singleton_bindings.is_empty() {
        return;
    }
    let repeated = std::mem::take(&mut scopes.root);
    let mut before = Vec::new();
    let mut after = Vec::new();
    for (position, bindings) in singleton_bindings {
        let mut segment = Scope::default();
        for (field, feed) in bindings {
            let target_path = vec![field.clone()];
            let Some(node) = builder.binding_node(feed, &target_path) else {
                continue;
            };
            segment.bindings.push(mapping::Binding {
                target_field: field,
                node,
            });
        }
        match position {
            schema::CsvSingletonPosition::Before(index) => before.push((index, segment)),
            schema::CsvSingletonPosition::After(index) => after.push((index, segment)),
        }
    }
    before.sort_by_key(|(index, _)| *index);
    after.sort_by_key(|(index, _)| *index);
    let mut segments = before
        .into_iter()
        .map(|(_, scope)| scope)
        .chain(std::iter::once(repeated))
        .chain(after.into_iter().map(|(_, scope)| scope));
    let Some(first) = segments.next() else {
        return;
    };
    scopes.root.iteration =
        ScopeIteration::Concatenate(ScopeSequence::new(first, segments.collect()));
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
