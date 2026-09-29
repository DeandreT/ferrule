//! Canonical export for a bounded serial chain of XML mapping stages with
//! optional independent final XML targets.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::{Path, PathBuf};

use mapping::{Pipeline, PipelineInput};
use roxmltree::{Document, Node};

use super::artifact::write_artifacts;
use super::compatibility::{ExportProfile, ExportReport};
use super::schema::{SideFormat, side_format};
use super::{PreparedExport, compatibility, prepare_export};
use crate::MfdError;

const MAX_STAGES: usize = 65;

/// Inspect the supported XML pipeline export without writing artifacts.
pub fn preflight_pipeline_export(
    pipeline: &Pipeline,
    path: &Path,
) -> Result<ExportReport, MfdError> {
    prepare_pipeline_export(pipeline, path).map(|prepared| prepared.report)
}

/// Write a bounded XML pipeline as one connected `.mfd` design.
///
/// Unsupported stage graphs reject before any design or schema sibling is
/// published. The supported shape has one host primary source, then each
/// stage reads the preceding stage's primary XML target. Later stages may
/// retain unused references to original host inputs. Connected later named
/// inputs, independent intermediate targets, and non-XML boundaries reject
/// explicitly. The final stage may write connected independent XML targets.
pub fn export_pipeline(pipeline: &Pipeline, path: &Path) -> Result<Vec<String>, MfdError> {
    export_pipeline_with_profile(pipeline, path, ExportProfile::default())
        .map(|report| report.warnings)
}

/// Export a supported XML pipeline under the selected compatibility policy.
pub fn export_pipeline_with_profile(
    pipeline: &Pipeline,
    path: &Path,
    profile: ExportProfile,
) -> Result<ExportReport, MfdError> {
    let prepared = prepare_pipeline_export(pipeline, path)?;
    if profile == ExportProfile::NativeMfd && !prepared.report.is_native_compatible() {
        return Err(MfdError::IncompatibleExport(Box::new(prepared.report)));
    }
    write_artifacts(
        path.parent().unwrap_or_else(|| Path::new(".")),
        prepared.artifacts,
    )?;
    Ok(prepared.report)
}

fn prepare_pipeline_export(pipeline: &Pipeline, path: &Path) -> Result<PreparedExport, MfdError> {
    validate_serial_shape(pipeline)?;
    let mut key_offset = 0u32;
    let mut uid_offset = 0u32;
    let mut combined: Option<String> = None;
    let mut artifacts = Vec::new();
    let mut warnings = Vec::new();
    for (index, stage) in pipeline.stages.iter().enumerate() {
        let stage_path = stage_artifact_path(path, index + 1)?;
        let mut project = stage.project.clone();
        if index > 0 {
            // The physical input is supplied by the previous pass-through
            // component, never by an independent file boundary.
            project.source_path = None;
        }
        if index + 1 < pipeline.stages.len() {
            // Intermediate targets become pass-through components, not
            // independent output files in the native design.
            project.target_path = None;
        }
        let prepared = prepare_export(&project, &stage_path)?;
        warnings.extend(prepared.report.warnings);
        let (stage_xml, siblings) = separate_stage_artifacts(prepared.artifacts, &stage_path)?;
        artifacts.extend(siblings);
        let (stage_xml, max_key, max_uid) = remap_identifiers(&stage_xml, key_offset, uid_offset)?;
        key_offset = key_offset.checked_add(max_key).ok_or_else(|| {
            MfdError::Unsupported("pipeline port keys exceed the .mfd integer range".into())
        })?;
        uid_offset = uid_offset.checked_add(max_uid).ok_or_else(|| {
            MfdError::Unsupported("pipeline component IDs exceed the .mfd integer range".into())
        })?;
        combined = Some(match combined {
            None => stage_xml,
            Some(previous) => {
                append_stage(&previous, &stage_xml, stage.project.extra_sources.len())?
            }
        });
    }
    let xml = combined.expect("validated pipeline has at least two stages");
    if pipeline
        .stages
        .last()
        .is_some_and(|stage| !stage.project.extra_targets.is_empty())
    {
        crate::import::validate_pipeline_export_graph(&xml)?;
    }
    let report = compatibility::profile(&xml, warnings)?;
    artifacts.push((path.to_path_buf(), xml));
    Ok(PreparedExport { artifacts, report })
}

fn validate_serial_shape(pipeline: &Pipeline) -> Result<(), MfdError> {
    if !(2..=MAX_STAGES).contains(&pipeline.stages.len()) {
        return Err(MfdError::Unsupported(format!(
            "serial XML pipeline export needs 2 to {MAX_STAGES} stages"
        )));
    }
    let issues = engine::validate_pipeline(pipeline);
    if !issues.is_empty() {
        return Err(MfdError::Unsupported(format!(
            "pipeline validation failed: {}",
            issues
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }
    let mut original_hosts = BTreeSet::new();
    for (index, stage) in pipeline.stages.iter().enumerate() {
        let connected = if index == 0 {
            if let PipelineInput::Host { name } = &stage.source {
                original_hosts.insert(name.as_str());
                true
            } else {
                false
            }
        } else {
            matches!(
                &stage.source,
                PipelineInput::StageTarget { stage: from, target: None }
                    if from == &pipeline.stages[index - 1].id
            )
        };
        if !connected {
            return Err(MfdError::Unsupported(format!(
                "pipeline stage `{}` is not part of a serial primary-target chain",
                stage.id
            )));
        }
        if index + 1 < pipeline.stages.len() && !stage.project.extra_targets.is_empty() {
            return Err(MfdError::Unsupported(format!(
                "pipeline stage `{}` has independent targets before the final stage",
                stage.id
            )));
        }
        if stage.project.extra_targets.iter().any(|target| {
            side_format(&target.path, &target.options) != SideFormat::Xml
                || target.options.wsdl.is_some()
        }) {
            return Err(MfdError::Unsupported(format!(
                "pipeline stage `{}` has a non-file-XML named target",
                stage.id
            )));
        }
        for binding in &stage.extra_sources {
            let PipelineInput::Host { name } = &binding.from else {
                return Err(MfdError::Unsupported(format!(
                    "pipeline stage `{}` has a non-host named input",
                    stage.id
                )));
            };
            if index > 0 && !original_hosts.contains(name.as_str()) {
                return Err(MfdError::Unsupported(format!(
                    "pipeline stage `{}` introduces host input `{name}` after the first stage",
                    stage.id
                )));
            }
            original_hosts.insert(name.as_str());
        }
        if stage.project.extra_sources.iter().any(|source| {
            source.dynamic_path.is_some()
                || side_format(&Some(source.path.clone()), &source.options) != SideFormat::Xml
                || source.options.http_get.is_some()
                || source.options.external_source.is_some()
                || source.options.wsdl.is_some()
        }) {
            return Err(MfdError::Unsupported(format!(
                "pipeline stage `{}` has a non-static-XML named input",
                stage.id
            )));
        }
        if side_format(&stage.project.source_path, &stage.project.source_options) != SideFormat::Xml
            || side_format(&stage.project.target_path, &stage.project.target_options)
                != SideFormat::Xml
            || stage.project.source_options.external_source.is_some()
            || stage.project.source_options.http_get.is_some()
            || stage.project.source_options.local_xml_file_set
            || stage.project.source_options.wsdl.is_some()
            || stage.project.target_options.wsdl.is_some()
        {
            return Err(MfdError::Unsupported(format!(
                "pipeline stage `{}` has a non-file-XML boundary",
                stage.id
            )));
        }
    }
    Ok(())
}

fn stage_artifact_path(path: &Path, index: usize) -> Result<PathBuf, MfdError> {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| {
            MfdError::Unsupported("pipeline export path needs a UTF-8 file stem".into())
        })?;
    Ok(path.with_file_name(format!("{stem}-stage-{index}.mfd")))
}

fn separate_stage_artifacts(
    artifacts: Vec<(PathBuf, String)>,
    stage_path: &Path,
) -> Result<(String, Vec<(PathBuf, String)>), MfdError> {
    let mut stage_xml = None;
    let mut siblings = Vec::new();
    for (path, contents) in artifacts {
        if path == stage_path {
            stage_xml = Some(contents);
        } else {
            siblings.push((path, contents));
        }
    }
    Ok((
        stage_xml
            .ok_or_else(|| MfdError::Unsupported("stage design artifact is missing".into()))?,
        siblings,
    ))
}

struct Edit {
    range: Range<usize>,
    replacement: String,
}

fn apply_edits(mut text: String, mut edits: Vec<Edit>) -> Result<String, MfdError> {
    edits.sort_by(|left, right| {
        right
            .range
            .start
            .cmp(&left.range.start)
            .then(right.range.end.cmp(&left.range.end))
    });
    let mut preceding_start = text.len();
    for edit in edits {
        if edit.range.end > preceding_start {
            return Err(MfdError::Unsupported(
                "pipeline export produced overlapping XML edits".into(),
            ));
        }
        preceding_start = edit.range.start;
        text.replace_range(edit.range, &edit.replacement);
    }
    Ok(text)
}

fn remap_identifiers(
    xml: &str,
    key_offset: u32,
    uid_offset: u32,
) -> Result<(String, u32, u32), MfdError> {
    let document = Document::parse(xml)?;
    let mut edits = Vec::new();
    let mut max_key = 0;
    let mut max_uid = 0;
    for node in document.descendants().filter(Node::is_element) {
        for attribute in node.attributes() {
            let is_key = attribute.name() == "key" || attribute.name().ends_with("key");
            let is_uid = attribute.name() == "uid" || attribute.name() == "ferrule-primary-source";
            if !is_key && !is_uid {
                continue;
            }
            let Ok(value) = attribute.value().parse::<u32>() else {
                continue;
            };
            let offset = if is_key {
                max_key = max_key.max(value);
                key_offset
            } else {
                max_uid = max_uid.max(value);
                uid_offset
            };
            let remapped = value.checked_add(offset).ok_or_else(|| {
                MfdError::Unsupported("pipeline identifiers exceed the .mfd integer range".into())
            })?;
            if offset > 0 {
                edits.push(Edit {
                    range: attribute.range(),
                    replacement: format!("{}=\"{remapped}\"", attribute.name()),
                });
            }
        }
    }
    Ok((apply_edits(xml.to_string(), edits)?, max_key, max_uid))
}

fn append_stage(previous: &str, next: &str, extra_sources: usize) -> Result<String, MfdError> {
    let previous_doc = Document::parse(previous)?;
    let next_doc = Document::parse(next)?;
    let previous_structure = structure(&previous_doc)?;
    let next_structure = structure(&next_doc)?;
    empty_resources(&next_doc)?;
    let previous_children = child(previous_structure, "children")?;
    let next_children = child(next_structure, "children")?;
    let previous_target = default_target(previous_children)?;
    let next_source = primary_source(&next_doc, next_children)?;
    let source_components = next_children
        .children()
        .filter(|node| node.has_tag_name("component"))
        .take(extra_sources + 1)
        .collect::<Vec<_>>();
    if source_components.first().copied() != Some(next_source) {
        return Err(MfdError::Unsupported(
            "rendered stage source component order changed".into(),
        ));
    }
    let connected = child(child(next_structure, "graph")?, "vertices")?
        .children()
        .filter(|node| node.has_tag_name("vertex"))
        .filter_map(|node| node.attribute("vertexkey"))
        .collect::<std::collections::BTreeSet<_>>();
    for source in source_components.iter().skip(1) {
        if source
            .descendants()
            .filter(|node| node.has_tag_name("entry"))
            .filter_map(|node| node.attribute("outkey"))
            .any(|key| connected.contains(key))
        {
            return Err(MfdError::Unsupported(format!(
                "stage named source `{}` is connected; serial XML export cannot merge that host component yet",
                source.attribute("name").unwrap_or_default()
            )));
        }
    }
    let mut edits = Vec::new();
    connect_pass_through(previous, previous_target, next_source, &mut edits)?;

    let components = next_children
        .children()
        .filter(Node::is_element)
        .filter(|component| !source_components.contains(component))
        .map(|component| &next[component.range()])
        .collect::<String>();
    insert_before_close(previous, previous_children, components, &mut edits)?;

    let previous_graph = child(previous_structure, "graph")?;
    let next_graph = child(next_structure, "graph")?;
    let previous_vertices = child(previous_graph, "vertices")?;
    let next_vertices = child(next_graph, "vertices")?;
    let vertices = next_vertices
        .children()
        .filter(Node::is_element)
        .map(|vertex| &next[vertex.range()])
        .collect::<String>();
    insert_before_close(previous, previous_vertices, vertices, &mut edits)?;
    let previous_edges = child(previous_graph, "edges")?;
    let next_edges = child(next_graph, "edges")?;
    let edges = next_edges
        .children()
        .filter(Node::is_element)
        .map(|edge| &next[edge.range()])
        .collect::<String>();
    insert_before_close(previous, previous_edges, edges, &mut edits)?;
    if next_graph
        .children()
        .filter(Node::is_element)
        .any(|node| !node.has_tag_name("edges") && !node.has_tag_name("vertices"))
    {
        return Err(MfdError::Unsupported(
            "pipeline stage has unsupported graph metadata".into(),
        ));
    }

    let next_hints = next_structure
        .children()
        .find(|node| node.has_tag_name("ferrule-scope-sources"));
    if next_structure
        .children()
        .filter(Node::is_element)
        .any(|node| {
            !node.has_tag_name("ferrule-scope-sources")
                && !node.has_tag_name("children")
                && !node.has_tag_name("graph")
        })
    {
        return Err(MfdError::Unsupported(
            "pipeline stage has unsupported structure metadata".into(),
        ));
    }
    if let Some(next_hints) = next_hints {
        let previous_hints = previous_structure
            .children()
            .find(|node| node.has_tag_name("ferrule-scope-sources"));
        if let Some(previous_hints) = previous_hints {
            let hints = next_hints
                .children()
                .filter(Node::is_element)
                .map(|hint| &next[hint.range()])
                .collect::<String>();
            insert_before_close(previous, previous_hints, hints, &mut edits)?;
        } else {
            edits.push(Edit {
                range: previous_children.range().start..previous_children.range().start,
                replacement: next[next_hints.range()].to_string(),
            });
        }
    }
    apply_edits(previous.to_string(), edits)
}

fn structure<'a>(document: &'a Document<'a>) -> Result<Node<'a, 'a>, MfdError> {
    let mapping = document.root_element();
    if mapping
        .children()
        .filter(Node::is_element)
        .any(|node| !node.has_tag_name("resources") && !node.has_tag_name("component"))
    {
        return Err(MfdError::Unsupported(
            "pipeline stage declares an unsupported mapping-level function".into(),
        ));
    }
    let wrapper = child(mapping, "component")?;
    child(wrapper, "structure")
}

fn empty_resources(document: &Document<'_>) -> Result<(), MfdError> {
    if document
        .root_element()
        .children()
        .find(|node| node.has_tag_name("resources"))
        .is_some_and(|resources| resources.children().any(|node| node.is_element()))
    {
        return Err(MfdError::Unsupported(
            "serial XML pipeline stages cannot declare mapping resources".into(),
        ));
    }
    Ok(())
}

fn child<'a>(parent: Node<'a, 'a>, name: &str) -> Result<Node<'a, 'a>, MfdError> {
    parent
        .children()
        .find(|node| node.has_tag_name(name))
        .ok_or_else(|| MfdError::Unsupported(format!("rendered stage has no `{name}` element")))
}

fn default_target<'a>(children: Node<'a, 'a>) -> Result<Node<'a, 'a>, MfdError> {
    let mut targets = children.children().filter(|node| {
        node.has_tag_name("component")
            && node.children().any(|properties| {
                properties.has_tag_name("properties")
                    && properties.attribute("XSLTDefaultOutput") == Some("1")
            })
    });
    let target = targets
        .next()
        .ok_or_else(|| MfdError::Unsupported("rendered stage has no default XML target".into()))?;
    if targets.next().is_some() {
        return Err(MfdError::Unsupported(
            "rendered stage has multiple default targets".into(),
        ));
    }
    Ok(target)
}

fn primary_source<'a>(
    document: &Document<'a>,
    children: Node<'a, 'a>,
) -> Result<Node<'a, 'a>, MfdError> {
    let uid = document
        .root_element()
        .attribute("ferrule-primary-source")
        .ok_or_else(|| MfdError::Unsupported("rendered stage has no primary source ID".into()))?;
    children
        .children()
        .find(|node| node.has_tag_name("component") && node.attribute("uid") == Some(uid))
        .ok_or_else(|| MfdError::Unsupported("rendered primary XML source is missing".into()))
}

fn connect_pass_through(
    previous: &str,
    target: Node<'_, '_>,
    source: Node<'_, '_>,
    edits: &mut Vec<Edit>,
) -> Result<(), MfdError> {
    let properties = child(target, "properties")?;
    let output = properties
        .attributes()
        .find(|attribute| attribute.name() == "XSLTDefaultOutput")
        .ok_or_else(|| {
            MfdError::Unsupported("intermediate target is not a default XML target".into())
        })?;
    edits.push(Edit {
        range: output.range(),
        replacement: "PassThrough=\"1\"".into(),
    });
    let source_keys = entry_output_keys(source)?;
    let target_entries = entry_ranges(target)?;
    for (path, outkey) in source_keys {
        let entries = target_entries.get(&path).ok_or_else(|| {
            MfdError::Unsupported(format!(
                "pass-through target is missing source port `{}`",
                path.join("/")
            ))
        })?;
        let [entry] = entries.as_slice() else {
            return Err(MfdError::Unsupported(format!(
                "pass-through port `{}` is ambiguous",
                path.join("/")
            )));
        };
        if entry.attribute("outkey").is_some() {
            return Err(MfdError::Unsupported(
                "intermediate target already has an output port".into(),
            ));
        }
        let start = entry.range().start;
        let open_end = previous[start..entry.range().end]
            .find('>')
            .ok_or_else(|| MfdError::Unsupported("rendered XML entry is not closed".into()))?
            + start;
        let insertion = if previous.as_bytes().get(open_end.wrapping_sub(1)) == Some(&b'/') {
            open_end - 1
        } else {
            open_end
        };
        edits.push(Edit {
            range: insertion..insertion,
            replacement: format!(" outkey=\"{outkey}\""),
        });
    }
    Ok(())
}

fn entry_output_keys(component: Node<'_, '_>) -> Result<BTreeMap<Vec<String>, u32>, MfdError> {
    let mut keys = BTreeMap::new();
    walk_entries(component, &mut |path, entry| {
        if let Some(outkey) = entry.attribute("outkey") {
            let key = outkey
                .parse::<u32>()
                .map_err(|_| MfdError::Unsupported("source output port key is invalid".into()))?;
            if keys.insert(path.to_vec(), key).is_some() {
                return Err(MfdError::Unsupported(format!(
                    "source port `{}` is ambiguous",
                    path.join("/")
                )));
            }
        }
        Ok(())
    })?;
    Ok(keys)
}

fn entry_ranges<'a>(
    component: Node<'a, 'a>,
) -> Result<BTreeMap<Vec<String>, Vec<Node<'a, 'a>>>, MfdError> {
    let mut entries = BTreeMap::<Vec<String>, Vec<Node<'a, 'a>>>::new();
    walk_entries(component, &mut |path, entry| {
        entries.entry(path.to_vec()).or_default().push(entry);
        Ok(())
    })?;
    Ok(entries)
}

fn walk_entries<'a>(
    component: Node<'a, 'a>,
    visit: &mut impl FnMut(&[String], Node<'a, 'a>) -> Result<(), MfdError>,
) -> Result<(), MfdError> {
    fn walk<'a>(
        entry: Node<'a, 'a>,
        path: &mut Vec<String>,
        visit: &mut impl FnMut(&[String], Node<'a, 'a>) -> Result<(), MfdError>,
    ) -> Result<(), MfdError> {
        visit(path, entry)?;
        for child in entry.children().filter(|node| node.has_tag_name("entry")) {
            path.push(child.attribute("name").unwrap_or_default().to_string());
            walk(child, path, visit)?;
            path.pop();
        }
        Ok(())
    }
    let data = child(component, "data")?;
    let root = child(data, "root")?;
    let entry = child(root, "entry")?;
    walk(entry, &mut Vec::new(), visit)
}

fn insert_before_close(
    xml: &str,
    node: Node<'_, '_>,
    value: String,
    edits: &mut Vec<Edit>,
) -> Result<(), MfdError> {
    if value.is_empty() {
        return Ok(());
    }
    let closing = format!("</{}>", node.tag_name().name());
    let range = node.range();
    let offset = xml[range.clone()].rfind(&closing).ok_or_else(|| {
        MfdError::Unsupported(format!(
            "rendered `{}` has no end tag",
            node.tag_name().name()
        ))
    })?;
    let index = range.start + offset;
    edits.push(Edit {
        range: index..index,
        replacement: value,
    });
    Ok(())
}
