use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::{
    FilenameImport, ImportOptions, MfdError, Pipeline, PipelineInput, PipelineNamedInput,
    PipelineStage, ResourceResolver, StageSelection, import_resolved, pipeline_exception,
    read_edge_inputs, resolved_resources, schema,
};

/// A connected staged design imported without flattening its intermediate
/// outputs into independent targets.
pub struct ImportedPipeline {
    pub pipeline: Pipeline,
    pub warnings: Vec<String>,
    /// Canonical path used as the active mapping identity for every stage.
    pub mapping_path: PathBuf,
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
    let result = import_pipeline_resolved(&resources, options);
    resources.finish_bindings()?;
    result
}

fn import_pipeline_resolved(
    resources: &ResourceResolver,
    options: &ImportOptions,
) -> Result<ImportedPipeline, MfdError> {
    let chain = if options.resource_bindings_v1.is_empty() {
        discover_pipeline_chain(resources.mapping_path())?
    } else {
        discover_pipeline_chain_text(&resources.read_design()?)?
    };
    let first_intermediate = &chain.intermediates[0];
    let mut lowered = vec![import_resolved(
        resources,
        StageSelection::Into {
            intermediate_key: first_intermediate.key,
            label: &first_intermediate.label,
            target_inputs: &first_intermediate.inputs,
        },
        options.item_ordered_exceptions,
        FilenameImport::None,
    )?];
    for pair in chain.intermediates.windows(2) {
        let [first_intermediate, second_intermediate] = pair else {
            unreachable!("adjacent intermediate window has two entries")
        };
        lowered.push(import_resolved(
            resources,
            StageSelection::Between {
                source_key: first_intermediate.key,
                source_label: &first_intermediate.label,
                target_key: second_intermediate.key,
                target_inputs: &second_intermediate.inputs,
            },
            options.item_ordered_exceptions,
            FilenameImport::None,
        )?);
    }
    let last_intermediate = chain
        .intermediates
        .last()
        .expect("chain has an intermediate");
    lowered.push(import_resolved(
        resources,
        StageSelection::OutOf {
            intermediate_key: last_intermediate.key,
            earlier_source_key: chain.final_named_source_key,
            final_key: chain.final_key,
            label: &last_intermediate.label,
            final_inputs: &chain.final_inputs,
        },
        options.item_ordered_exceptions,
        FilenameImport::None,
    )?);

    resources.finish_bindings()?;
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
    let final_named_producer_index = chain.final_named_source_key.map(|key| {
        chain
            .intermediates
            .iter()
            .position(|intermediate| intermediate.key == key)
            .expect("branching intermediate belongs to the chain")
    });
    // The producing stage's name travels with the key it publishes.
    let final_named_output = final_named_producer_index.map(|producer_index| {
        (
            lowered[producer_index + 1].source_components[0].key,
            format!("mfd-stage-{}", producer_index + 1),
        )
    });
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
            let from = if let Some(host_name) = host_names.get(&identity.key) {
                PipelineInput::Host {
                    name: host_name.clone(),
                }
            } else if index + 1 == chain.intermediates.len() + 1
                && let Some((_, producer_stage)) = final_named_output
                    .as_ref()
                    .filter(|(key, _)| *key == identity.key)
            {
                PipelineInput::StageTarget {
                    stage: producer_stage.clone(),
                    target: None,
                }
            } else {
                return Err(MfdError::UnsupportedImport(format!(
                    "pipeline stage {} reads an unbound source `{}`",
                    index + 1,
                    identity.name
                )));
            };
            extra_sources.push(PipelineNamedInput {
                name: source.name.clone(),
                from,
            });
        }
        if index + 1 == chain.intermediates.len() + 1
            && final_named_output.is_some()
            && extra_sources
                .iter()
                .filter(|binding| matches!(binding.from, PipelineInput::StageTarget { .. }))
                .count()
                != 1
        {
            return Err(MfdError::UnsupportedImport(
                "earlier intermediate output is not a unique final named input".into(),
            ));
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

struct PipelineIntermediate {
    label: String,
    key: u32,
    inputs: TargetInputCoverage,
}

struct DiscoveredPipelineChain {
    intermediates: Vec<PipelineIntermediate>,
    final_named_source_key: Option<u32>,
    final_key: u32,
    final_inputs: TargetInputCoverage,
}

/// Raw connected target pins must survive boundary parsing. JSON's redundant
/// `null` alternative is the one intentional exception when a typed sibling
/// is also connected and represented by the imported target.
#[derive(Default)]
pub(super) struct TargetInputCoverage {
    pub(super) connected: BTreeSet<u32>,
    json_null_siblings: BTreeMap<u32, BTreeSet<u32>>,
}

impl TargetInputCoverage {
    pub(super) fn unrepresented(&self, represented: &BTreeSet<u32>) -> Option<u32> {
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
    let text = crate::design::read(path)?;
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
    let doc = crate::design::parse(text)?;
    let mapping = doc.root_element();
    if !mapping.has_tag_name("mapping") {
        return Err(MfdError::NotMfd("root element is not <mapping>"));
    }
    schema::validate_xml_optional_metadata(&mapping)?;
    schema::validate_xml_attribute_required_metadata(&mapping)?;
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
    let edges = read_edge_inputs(&structure, Some(&wrapper));
    if !edges.conflicts.is_empty() {
        return Err(MfdError::UnsupportedImport(edges.conflicts.join("; ")));
    }
    let edge_from = edges.feeds;
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
    let exception_terminals = pipeline_exception::classify(
        &structure,
        &wrapper,
        &components,
        &edge_from,
        &consumers,
        &function_outputs,
        &connected_outputs,
    )?;
    let (order, final_named_source) = strict_serial_stage_order(
        &components,
        &intermediates,
        &terminal_targets,
        &connected_outputs,
        &consumers,
        &function_outputs,
        &exception_terminals.inputs,
    )?;
    Ok(DiscoveredPipelineChain {
        final_named_source_key: final_named_source.map(|index| intermediate_keys[index]),
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

pub(super) fn entry_keys(component: &roxmltree::Node<'_, '_>, attribute: &str) -> BTreeSet<u32> {
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

/// Keep imported stages to one linear chain of XML intermediates. One bounded
/// branch may also feed the final target from a nonadjacent earlier intermediate;
/// that earlier result becomes a separate named source of the final stage.
/// Original host sources may supplement any stage.
fn strict_serial_stage_order(
    components: &[roxmltree::Node<'_, '_>],
    intermediates: &[&roxmltree::Node<'_, '_>],
    terminal_targets: &[&roxmltree::Node<'_, '_>],
    connected_outputs: &BTreeSet<u32>,
    consumers: &BTreeMap<u32, Vec<u32>>,
    function_outputs: &BTreeMap<u32, Vec<u32>>,
    exception_inputs: &BTreeSet<u32>,
) -> Result<(Vec<usize>, Option<usize>), MfdError> {
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
            exception_inputs,
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
    let mut final_named_source = None;
    for &index in &intermediate_indices {
        let sinks = sinks_by_component.get(&index).ok_or_else(invalid_chain)?;
        let next = if sinks == &terminal_indices {
            None
        } else if intermediates.len() >= 2
            && terminal_indices.len() == 1
            && terminal_indices
                .iter()
                .all(|target| components[*target].attribute("library") == Some("xml"))
            && sinks.len() == 2
            && terminal_indices.is_subset(sinks)
        {
            let next = sinks.difference(&terminal_indices).next().copied();
            let Some(next) = next.filter(|next| intermediate_indices.contains(next)) else {
                return Err(invalid_chain());
            };
            if final_named_source.replace(index).is_some() {
                return Err(invalid_chain());
            }
            Some(next)
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
            if ordered.len() != intermediate_indices.len() {
                return Err(invalid_chain());
            }
            if final_named_source.is_some_and(|source| {
                !components[source]
                    .children()
                    .find(|node| node.has_tag_name("data"))
                    .and_then(|data| data.children().find(|node| node.has_tag_name("document")))
                    .and_then(|document| document.attribute("inputinstance"))
                    .is_some_and(|preview| !preview.trim().is_empty())
            }) {
                return Err(invalid_chain());
            }
            let final_named_source = final_named_source.map(|source| {
                intermediate_indices
                    .iter()
                    .position(|&index| index == source)
                    .expect("branching intermediate belongs to the chain")
            });
            return Ok((ordered, final_named_source));
        };
        current = next;
    }
    Err(invalid_chain())
}

pub(super) fn trace_xml_sinks(
    outputs: Vec<u32>,
    connected_outputs: &BTreeSet<u32>,
    consumers: &BTreeMap<u32, Vec<u32>>,
    function_outputs: &BTreeMap<u32, Vec<u32>>,
    target_input_owners: &BTreeMap<u32, usize>,
    exception_inputs: &BTreeSet<u32>,
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
            } else if exception_inputs.contains(input) {
                // A validated terminal side effect is not an XML document sink.
                // Other branches must still reach one serial target owner.
                continue;
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
