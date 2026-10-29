use std::collections::{BTreeMap, BTreeSet};

use super::schema::parse_u32;
use super::{
    FilenameImport, ImportOptions, ImportedDocument, StageSelection,
    graph::GraphBuilder,
    schema::{ComponentFormat, SchemaComponent},
};
use crate::MfdError;
use std::path::Path;

/// Import the bounded, explicit one-EDI-document to one-JSON-document route.
/// Connected filenames remain graph roots; no instance or output file is opened.
pub fn import_filename_document(path: &Path) -> Result<ImportedDocument, MfdError> {
    import_filename_document_with_options(path, &ImportOptions::default())
}

pub fn import_filename_document_with_options(
    path: &Path,
    options: &ImportOptions,
) -> Result<ImportedDocument, MfdError> {
    let resources = super::resolved_resources(path, options)?;
    let result = super::import_resolved(
        &resources,
        StageSelection::Ordinary,
        options.item_ordered_exceptions,
        FilenameImport::Endpoints,
    );
    resources.finish_bindings()?;
    let lowered = result?;
    let report = super::assess_import(&lowered.imported);
    if !report.executable {
        return Err(MfdError::IncompatibleImport(Box::new(report)));
    }
    let document = mapping::DocumentProject::new(
        lowered.imported.project,
        mapping::PrimarySourcePlan::new(
            lowered
                .primary_filename
                .ok_or_else(|| unsupported("missing input filename"))?,
        ),
    )
    .with_output_filename(
        lowered
            .output_filename
            .ok_or_else(|| unsupported("missing output filename"))?,
    );
    document
        .primary_route()
        .map_err(|error| unsupported(&error.to_string()))?;
    let issues = engine::validate_primary_document(&document);
    if !issues.is_empty() {
        return Err(unsupported(
            &issues
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; "),
        ));
    }
    Ok(ImportedDocument {
        document,
        warnings: lowered.imported.warnings,
        mapping_path: lowered.imported.mapping_path,
    })
}

fn unsupported(reason: &str) -> MfdError {
    MfdError::UnsupportedImport(format!("filename endpoints: {reason}"))
}

// Inspect the raw declaration before graph lowering can erase a filter, default
// or transparent function. Filename admission must not infer omitted controls.
fn filename_declaration(structure: roxmltree::Node<'_, '_>, feed: u32) -> Result<(), MfdError> {
    let points = structure
        .descendants()
        .filter(|node| {
            node.has_tag_name("datapoint") && parse_u32(node.attribute("key")) == Some(feed)
        })
        .collect::<Vec<_>>();
    let [point] = points.as_slice() else {
        return Err(unsupported(
            "filename must be a literal or required global string parameter",
        ));
    };
    let targets = point
        .parent()
        .ok_or_else(|| unsupported("missing filename declaration"))?;
    let component = targets
        .parent()
        .ok_or_else(|| unsupported("missing filename component"))?;
    if !targets.has_tag_name("targets")
        || targets.attributes().len() != 0
        || !component.has_tag_name("component")
        || component.attribute("library") != Some("core")
        || component.attributes().any(|attribute| {
            attribute.namespace().is_some()
                || !matches!(attribute.name(), "name" | "library" | "kind" | "uid")
        })
        || point.attribute("pos") != Some("0")
        || point
            .attributes()
            .any(|attribute| !matches!(attribute.name(), "pos" | "key"))
        || point.children().any(|node| node.is_element())
        || targets
            .children()
            .filter(roxmltree::Node::is_element)
            .count()
            != 1
        || component
            .children()
            .filter(|node| node.has_tag_name("targets"))
            .count()
            != 1
        || component.children().any(|node| {
            node.is_element()
                && match node.tag_name().name() {
                    "data" | "targets" | "view" => false,
                    "sources" | "properties" => {
                        node.attributes().len() != 0
                            || node.children().any(|child| child.is_element())
                    }
                    _ => true,
                }
        })
    {
        return Err(unsupported(
            "unsupported filename declaration controls or pins",
        ));
    }
    let data = component
        .children()
        .filter(|node| node.has_tag_name("data"))
        .collect::<Vec<_>>();
    let [data] = data.as_slice() else {
        return Err(unsupported("requires one filename data declaration"));
    };
    if data.attributes().len() != 0 {
        return Err(unsupported("unsupported filename data controls"));
    }
    let contents = data
        .children()
        .filter(roxmltree::Node::is_element)
        .filter(|node| {
            !(node.has_tag_name("wsdl")
                && node.attributes().len() == 0
                && !node.children().any(|child| child.is_element()))
        })
        .collect::<Vec<_>>();
    let valid = match (component.attribute("kind"), contents.as_slice()) {
        (Some("2"), [constant]) => {
            constant.has_tag_name("constant")
                && constant.attribute("datatype") == Some("string")
                && constant.attribute("value").is_some()
                && constant
                    .attributes()
                    .all(|attribute| matches!(attribute.name(), "datatype" | "value"))
                && !constant.children().any(|node| node.is_element())
        }
        (Some("6"), [input, parameter]) => {
            input.has_tag_name("input")
                && input.attribute("datatype") == Some("string")
                && input.attributes().all(|attribute| match attribute.name() {
                    "datatype" | "previewvalue" => true,
                    "required" => attribute.value() == "1",
                    "usepreviewvalue" => matches!(attribute.value(), "0" | "1"),
                    _ => false,
                })
                && !input.children().any(|node| node.is_element())
                && parameter.has_tag_name("parameter")
                && parameter.attribute("usageKind") == Some("input")
                && parameter
                    .attribute("name")
                    .is_some_and(|name| !name.is_empty())
                && parameter.attributes().all(|attribute| {
                    matches!(attribute.name(), "usageKind" | "name")
                        || attribute.name() == "optional" && attribute.value() == "0"
                })
                && !parameter.children().any(|node| node.is_element())
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(unsupported(
            "filename must be a literal or required global string parameter without defaults",
        ))
    }
}

pub(super) struct Endpoints {
    input: u32,
    output: u32,
    input_feed: u32,
    output_feed: u32,
}

impl Endpoints {
    pub(super) fn inputs(&self) -> BTreeSet<u32> {
        [self.input, self.output].into()
    }

    pub(super) fn read(
        structure: roxmltree::Node<'_, '_>,
        feeds: &BTreeMap<u32, u32>,
    ) -> Result<Self, MfdError> {
        let children = structure
            .children()
            .find(|node| node.has_tag_name("children"))
            .ok_or_else(|| unsupported("missing components"))?;
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        for component in children
            .children()
            .filter(|node| node.has_tag_name("component"))
        {
            let is_edi = component.attribute("library") == Some("text")
                && component
                    .descendants()
                    .any(|node| node.has_tag_name("text") && node.attribute("type") == Some("edi"));
            let is_json = component.attribute("library") == Some("json");
            if !is_edi && !is_json {
                continue;
            }
            if component.attribute("kind") != Some(if is_edi { "16" } else { "31" }) {
                return Err(unsupported("unsupported endpoint component kind"));
            }
            if component.attributes().any(|attribute| {
                attribute.namespace().is_some()
                    || !matches!(attribute.name(), "name" | "library" | "kind" | "uid")
            }) || component.children().any(|node| {
                node.is_element()
                    && match node.tag_name().name() {
                        "data" | "view" => false,
                        "properties" => {
                            node.attributes().any(|attribute| {
                                attribute.name() != "XSLTDefaultOutput"
                                    || !matches!(attribute.value(), "0" | "1")
                            }) || node.children().any(|child| child.is_element())
                        }
                        _ => true,
                    }
            }) {
                return Err(unsupported("unsupported endpoint component controls"));
            }
            let data = component
                .children()
                .filter(|node| node.has_tag_name("data"))
                .collect::<Vec<_>>();
            let [data] = data.as_slice() else {
                return Err(unsupported("requires one data declaration per endpoint"));
            };
            if data.attributes().len() != 0 {
                return Err(unsupported("unsupported endpoint data controls"));
            }
            let formats = data
                .children()
                .filter(|node| node.has_tag_name(if is_edi { "text" } else { "json" }))
                .collect::<Vec<_>>();
            let [format] = formats.as_slice() else {
                return Err(unsupported("requires one endpoint format declaration"));
            };
            if format.attributes().any(|attribute| {
                attribute.namespace().is_some()
                    || !(if is_edi {
                        matches!(
                            attribute.name(),
                            "type" | "kind" | "config" | "inputinstance" | "outputinstance"
                        )
                    } else {
                        matches!(
                            attribute.name(),
                            "schema" | "inputinstance" | "outputinstance"
                        )
                    })
            }) || format.children().any(|node| node.is_element())
                || data
                    .children()
                    .any(|node| node.is_element() && node != *format && !node.has_tag_name("root"))
            {
                return Err(unsupported("unsupported endpoint transport settings"));
            }
            let roots = data
                .children()
                .filter(|node| node.has_tag_name("root"))
                .collect::<Vec<_>>();
            let [root] = roots.as_slice() else {
                return Err(unsupported("requires one endpoint root"));
            };
            if root.attributes().len() != 0 {
                return Err(unsupported("unsupported endpoint root controls"));
            }
            if root
                .children()
                .filter(|node| node.has_tag_name("header"))
                .count()
                > 1
            {
                return Err(unsupported("ambiguous namespace headers"));
            }
            let entries = root
                .children()
                .filter(roxmltree::Node::is_element)
                .filter(|node| !super::csv_document::protocol_namespace_header(*node))
                .collect::<Vec<_>>();
            let [wrapper] = entries.as_slice() else {
                return Err(unsupported(
                    "requires one FileInstance wrapper without controls",
                ));
            };
            if !wrapper.has_tag_name("entry")
                || wrapper.attribute("name") != Some("FileInstance")
                || wrapper.attributes().any(|attribute| {
                    attribute.namespace().is_some()
                        || !matches!(
                            attribute.name(),
                            "name" | "ns" | "inpkey" | "outkey" | "expanded"
                        )
                })
            {
                return Err(unsupported("unsupported FileInstance metadata"));
            }
            let files = wrapper
                .children()
                .filter(|node| node.has_tag_name("file"))
                .collect::<Vec<_>>();
            if files.len() > 1
                || files.iter().any(|file| {
                    !matches!(
                        file.attribute("role"),
                        Some("inputinstance" | "outputinstance")
                    ) || file.attribute("name").is_none()
                        || file
                            .attributes()
                            .any(|attribute| !matches!(attribute.name(), "role" | "name"))
                        || file.children().any(|node| node.is_element())
                })
            {
                return Err(unsupported("unsupported preview file metadata"));
            }
            let documents = wrapper
                .children()
                .filter(roxmltree::Node::is_element)
                .filter(|node| !node.has_tag_name("file"))
                .collect::<Vec<_>>();
            if !matches!(documents.as_slice(), [document] if document.has_tag_name("entry") && document.attribute("name") == Some("document"))
            {
                return Err(unsupported(
                    "requires one document without wrapper controls",
                ));
            }
            if documents[0].attributes().any(|attribute| {
                attribute.namespace().is_some()
                    || !matches!(attribute.name(), "name" | "ns" | "expanded")
            }) {
                return Err(unsupported("unsupported document wrapper metadata"));
            }
            if !super::schema::is_xml_document_wrapper(&documents[0], *root) {
                return Err(unsupported(
                    "FileInstance namespace is not a protocol namespace",
                ));
            }
            let input = parse_u32(wrapper.attribute("inpkey"))
                .ok_or_else(|| unsupported("missing filename input port"))?;
            let feed = *feeds
                .get(&input)
                .ok_or_else(|| unsupported("filename input must be connected"))?;
            if feeds.contains_key(&feed) {
                return Err(unsupported(
                    "filename expression output cannot have an incoming feed",
                ));
            }
            filename_declaration(structure, feed)?;
            if wrapper.attribute("outkey").is_some_and(|key| {
                key.parse::<u32>()
                    .ok()
                    .is_none_or(|key| feeds.values().any(|feed| *feed == key))
            }) {
                return Err(unsupported(
                    "connected or malformed filename output ports are not admitted",
                ));
            }
            for key in [input, feed] {
                let declarations = structure
                    .descendants()
                    .filter(|node| node.has_tag_name("entry") || node.has_tag_name("datapoint"))
                    .flat_map(|node| {
                        [
                            node.attribute("key"),
                            node.attribute("inpkey"),
                            node.attribute("outkey"),
                        ]
                    })
                    .filter(|value| parse_u32(*value) == Some(key))
                    .count();
                if declarations != 1 {
                    return Err(unsupported("filename port declarations must be unique"));
                }
            }
            if is_edi {
                inputs.push((input, feed));
            } else {
                outputs.push((input, feed));
            }
        }
        let ([(input, input_feed)], [(output, output_feed)]) =
            (inputs.as_slice(), outputs.as_slice())
        else {
            return Err(unsupported("requires one EDI source and one JSON target"));
        };
        Ok(Self {
            input: *input,
            output: *output,
            input_feed: *input_feed,
            output_feed: *output_feed,
        })
    }

    pub(super) fn retain(
        &self,
        source: &SchemaComponent,
        target: &SchemaComponent,
        builder: &mut GraphBuilder<'_>,
    ) -> Result<(mapping::NodeId, mapping::NodeId), MfdError> {
        if source.format != ComponentFormat::Edi || target.format != ComponentFormat::Json {
            return Err(unsupported(
                "selected endpoints must be an EDI source and JSON target",
            ));
        }
        let mut retain = |feed| {
            builder
                .binding_node(feed, &[])
                .ok_or_else(|| unsupported("cannot retain filename feed"))
        };
        Ok((retain(self.input_feed)?, retain(self.output_feed)?))
    }
}

/// These readers retain data ports, but do not lower the enclosing
/// filename inputs or outputs into acquisition and document-output scopes.
pub(super) fn warn_with_admitted_outputs(
    component: roxmltree::Node<'_, '_>,
    feeds: &BTreeMap<u32, u32>,
    format: &str,
    admitted_outputs: &BTreeSet<u32>,
    warnings: &mut Vec<String>,
) {
    let name = component.attribute("name").unwrap_or_default();
    let outputs = feeds.values().copied().collect::<BTreeSet<_>>();
    let mut connected = BTreeSet::new();
    for data in component
        .children()
        .filter(|node| node.has_tag_name("data"))
    {
        for root in data.children().filter(|node| node.has_tag_name("root")) {
            for wrapper in root.children().filter(|node| {
                node.has_tag_name("entry") && node.attribute("name") == Some("FileInstance")
            }) {
                for (attribute, direction) in [("inpkey", "input"), ("outkey", "output")] {
                    if let Some(key) = parse_u32(wrapper.attribute(attribute))
                        && (if direction == "input" {
                            feeds.contains_key(&key)
                        } else {
                            outputs.contains(&key)
                        })
                        && (direction != "input" || !admitted_outputs.contains(&key))
                    {
                        connected.insert((direction, key));
                    }
                }
            }
        }
    }
    for (direction, key) in connected {
        warnings.push(format!(
            "{format} component `{name}` has connected FileInstance {direction} port `{key}`; filename-driven documents are unsupported"
        ));
    }
}
