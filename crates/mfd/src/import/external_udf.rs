use std::collections::{BTreeMap, BTreeSet};

use mapping::{ExternalPayloadFormat, ExternalSourceOptions, FormatOptions};

use super::function::FnComponent;
use super::schema::{
    ComponentFormat, SchemaComponent, collect_json_ports, json_entry_value_schema, parse_u32,
    record_entry_keys,
};

pub(super) struct Candidate {
    name: String,
    reason: String,
    component: SchemaComponent,
    input_keys: BTreeSet<u32>,
    warnings: Vec<String>,
}

impl Candidate {
    pub(super) fn read(component: &roxmltree::Node<'_, '_>, reason: &str) -> Option<Self> {
        let mut warnings = Vec::new();
        let mut source = read_json_output_source(component, &mut warnings)?;
        source.options.external_source = Some(
            ExternalSourceOptions::user_function(
                source.name.clone(),
                reason,
                ExternalPayloadFormat::Json,
            )
            .ok()?,
        );
        source.options.json_document = true;
        let input_keys = component
            .descendants()
            .filter(|node| node.has_tag_name("entry"))
            .filter_map(|entry| parse_u32(entry.attribute("inpkey")))
            .collect();
        Some(Self {
            name: source.name.clone(),
            reason: reason.to_string(),
            component: source,
            input_keys,
            warnings,
        })
    }

    fn is_eligible(
        &self,
        target_inputs: &BTreeSet<u32>,
        edges: &BTreeMap<u32, u32>,
        functions: &[FnComponent],
    ) -> bool {
        !self.input_keys.iter().any(|key| edges.contains_key(key))
            && target_inputs.iter().any(|target_input| {
                edges.get(target_input).is_some_and(|feed| {
                    depends_on_candidate(
                        *feed,
                        &self.component.output_keys,
                        edges,
                        functions,
                        &mut BTreeSet::new(),
                    )
                })
            })
    }
}

pub(super) fn capture_or_warn(
    component: &roxmltree::Node<'_, '_>,
    reason: Option<&str>,
    candidates: &mut Vec<Candidate>,
    warnings: &mut Vec<String>,
) -> bool {
    let reason = reason.or_else(|| {
        (component.attribute("library") == Some("user")
            && component.attribute("kind") == Some("19"))
        .then_some("the component definition is unavailable or unsupported")
    });
    let Some(reason) = reason else {
        return false;
    };
    match Candidate::read(component, reason) {
        Some(candidate) => candidates.push(candidate),
        None => warnings.push(format!(
            "skipped user-defined function `{}`: {reason}",
            component.attribute("name").unwrap_or_default()
        )),
    }
    true
}

pub(super) fn selected_target_inputs(components: &[SchemaComponent]) -> Option<BTreeSet<u32>> {
    let targets = components
        .iter()
        .filter(|component| component.is_target())
        .collect::<Vec<_>>();
    targets
        .iter()
        .copied()
        .find(|component| component.is_default_output)
        .or_else(|| {
            targets
                .iter()
                .copied()
                .find(|component| !component.is_pass_through)
        })
        .or_else(|| targets.first().copied())
        .map(|target| target.ports.keys().copied().collect())
}

/// Treats one public JSON-shaped output tree as an ordinary external source.
/// The opaque component body remains unsupported; only its visible result
/// contract crosses this boundary.
fn read_json_output_source(
    component: &roxmltree::Node<'_, '_>,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    let output_roots = component
        .descendants()
        .filter(|node| node.has_tag_name("root"))
        .filter(|root| {
            root.descendants()
                .any(|entry| entry.has_tag_name("entry") && entry.attribute("outkey").is_some())
        })
        .collect::<Vec<_>>();
    let [root] = output_roots.as_slice() else {
        return None;
    };
    let entry = root.children().find(|node| node.has_tag_name("entry"))?;
    if !entry
        .descendants()
        .any(|node| node.has_tag_name("entry") && node.attribute("type") == Some("json-property"))
    {
        return None;
    }

    let name = component
        .attribute("name")
        .filter(|name| !name.is_empty())
        .unwrap_or("ExternalResult")
        .to_string();
    let mut ports = BTreeMap::new();
    let mut output_count = 0usize;
    let mut input_count = 0usize;
    record_entry_keys(&entry, &[], &mut ports, &mut output_count, &mut input_count);
    collect_json_ports(
        &entry,
        &mut Vec::new(),
        &mut ports,
        &mut output_count,
        &mut input_count,
        warnings,
    );
    if output_count == 0 || input_count != 0 {
        return None;
    }
    let output_keys = ports.keys().copied().collect();

    Some(SchemaComponent {
        schema: json_entry_value_schema(&name, &entry),
        name,
        format: ComponentFormat::Json,
        input_instance: None,
        output_instance: None,
        options: FormatOptions::default(),
        is_source: true,
        is_default_output: false,
        is_variable: false,
        is_pass_through: false,
        compute_when_key: None,
        ports,
        input_ancestors: BTreeMap::new(),
        input_keys: BTreeSet::new(),
        output_keys,
        db_queries: Vec::new(),
        dynamic_json: None,
    })
}

fn depends_on_candidate(
    feed: u32,
    candidate_outputs: &BTreeSet<u32>,
    edges: &BTreeMap<u32, u32>,
    functions: &[FnComponent],
    seen: &mut BTreeSet<u32>,
) -> bool {
    if candidate_outputs.contains(&feed) {
        return true;
    }
    if !seen.insert(feed) {
        return false;
    }
    let Some(function) = functions
        .iter()
        .find(|function| function.outputs.contains(&feed))
    else {
        return false;
    };
    function
        .inputs
        .iter()
        .flatten()
        .filter_map(|input| edges.get(input))
        .any(|upstream| depends_on_candidate(*upstream, candidate_outputs, edges, functions, seen))
}

pub(super) fn install_fallback(
    components: &mut Vec<SchemaComponent>,
    candidates: Vec<Candidate>,
    target_inputs: &BTreeSet<u32>,
    edges: &BTreeMap<u32, u32>,
    functions: &[FnComponent],
    warnings: &mut Vec<String>,
) {
    let needs_source = !components
        .iter()
        .any(|component| !component.is_variable && component.is_source);
    let eligible = candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.is_eligible(target_inputs, edges, functions))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let selected = if needs_source && eligible.len() == 1 {
        eligible.first().copied()
    } else {
        None
    };

    for (index, candidate) in candidates.into_iter().enumerate() {
        if selected == Some(index) {
            warnings.extend(candidate.warnings);
            components.push(candidate.component);
        } else {
            warnings.push(format!(
                "skipped user-defined function `{}`: {}",
                candidate.name, candidate.reason
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use ir::{ScalarType, SchemaKind};

    use super::{Candidate, capture_or_warn, install_fallback, read_json_output_source};
    use crate::import::function::read as read_function;

    const JSON_OUTPUT: &str = r#"
        <component name="ExternalRows" library="user" kind="19">
          <data><root><entry name="object" componentid="70"><entry name="object">
            <entry name="rows" type="json-property"><entry name="array">
              <entry name="item" type="json-item"><entry name="object" outkey="10">
                <entry name="label" type="json-property"><entry name="string" outkey="11"/></entry>
                <entry name="count" type="json-property"><entry name="integer" outkey="12"/></entry>
                <entry name="weight" type="json-property"><entry name="number" outkey="13"/></entry>
                <entry name="enabled" type="json-property"><entry name="boolean" outkey="14"/></entry>
              </entry></entry>
            </entry></entry>
          </entry></entry></root></data>
        </component>
    "#;

    fn candidate() -> Option<Candidate> {
        let document = roxmltree::Document::parse(JSON_OUTPUT).ok()?;
        Candidate::read(&document.root_element(), "definition is recursive")
    }

    #[test]
    fn json_output_source_preserves_typed_paths_and_repetition()
    -> Result<(), Box<dyn std::error::Error>> {
        let document = roxmltree::Document::parse(JSON_OUTPUT)?;
        let mut warnings = Vec::new();
        let source = read_json_output_source(&document.root_element(), &mut warnings)
            .ok_or_else(|| std::io::Error::other("JSON output source was not recognized"))?;

        assert!(warnings.is_empty());
        assert!(source.is_source);
        assert_eq!(source.ports.get(&10), Some(&vec!["rows".to_string()]));
        assert_eq!(
            source.ports.get(&14),
            Some(&vec!["rows".to_string(), "enabled".to_string()])
        );
        assert_eq!(source.output_keys, BTreeSet::from([10_u32, 11, 12, 13, 14]));

        let rows = source
            .schema
            .child("rows")
            .ok_or_else(|| std::io::Error::other("missing rows schema"))?;
        assert!(rows.repeating);
        assert!(matches!(rows.kind, SchemaKind::Group { .. }));
        assert!(matches!(
            rows.child("label").map(|node| &node.kind),
            Some(SchemaKind::Scalar {
                ty: ScalarType::String
            })
        ));
        assert!(matches!(
            rows.child("count").map(|node| &node.kind),
            Some(SchemaKind::Scalar {
                ty: ScalarType::Int
            })
        ));
        assert!(matches!(
            rows.child("weight").map(|node| &node.kind),
            Some(SchemaKind::Scalar {
                ty: ScalarType::Float
            })
        ));
        assert!(matches!(
            rows.child("enabled").map(|node| &node.kind),
            Some(SchemaKind::Scalar {
                ty: ScalarType::Bool
            })
        ));
        Ok(())
    }

    #[test]
    fn json_output_source_rejects_missing_or_mixed_output_ports()
    -> Result<(), Box<dyn std::error::Error>> {
        let missing = JSON_OUTPUT
            .replace(" outkey=\"10\"", "")
            .replace(" outkey=\"11\"", "")
            .replace(" outkey=\"12\"", "")
            .replace(" outkey=\"13\"", "")
            .replace(" outkey=\"14\"", "");
        let document = roxmltree::Document::parse(&missing)?;
        assert!(read_json_output_source(&document.root_element(), &mut Vec::new()).is_none());

        let mixed = JSON_OUTPUT.replace("outkey=\"14\"", "inpkey=\"14\"");
        let document = roxmltree::Document::parse(&mixed)?;
        assert!(read_json_output_source(&document.root_element(), &mut Vec::new()).is_none());
        Ok(())
    }

    #[test]
    fn fallback_rejects_an_output_not_connected_to_the_target()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut components = Vec::new();
        let mut warnings = Vec::new();
        install_fallback(
            &mut components,
            vec![candidate().ok_or_else(|| std::io::Error::other("missing candidate"))?],
            &BTreeSet::from([30]),
            &BTreeMap::new(),
            &[],
            &mut warnings,
        );

        assert!(components.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].starts_with("skipped user-defined function"));
        Ok(())
    }

    #[test]
    fn missing_definition_can_supply_an_opaque_candidate() -> Result<(), Box<dyn std::error::Error>>
    {
        let document = roxmltree::Document::parse(JSON_OUTPUT)?;
        let mut candidates = Vec::new();
        let mut warnings = Vec::new();
        assert!(capture_or_warn(
            &document.root_element(),
            None,
            &mut candidates,
            &mut warnings,
        ));
        assert_eq!(candidates.len(), 1);
        assert!(warnings.is_empty());
        Ok(())
    }

    #[test]
    fn fallback_rejects_a_call_with_a_connected_input() -> Result<(), Box<dyn std::error::Error>> {
        let xml = JSON_OUTPUT.replace(
            "<data>",
            "<data><root><entry name=\"argument\" inpkey=\"9\"/></root>",
        );
        let document = roxmltree::Document::parse(&xml)?;
        let candidate = Candidate::read(&document.root_element(), "unsupported")
            .ok_or_else(|| std::io::Error::other("missing candidate"))?;
        let mut components = Vec::new();
        let mut warnings = Vec::new();
        install_fallback(
            &mut components,
            vec![candidate],
            &BTreeSet::from([30]),
            &BTreeMap::from([(9, 40), (30, 10)]),
            &[],
            &mut warnings,
        );

        assert!(components.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].starts_with("skipped user-defined function"));
        Ok(())
    }

    #[test]
    fn fallback_follows_function_inputs_to_a_candidate_output()
    -> Result<(), Box<dyn std::error::Error>> {
        let function_document = roxmltree::Document::parse(
            r#"<component name="normalize-space" library="core" kind="5">
                <sources><datapoint pos="0" key="20"/></sources>
                <targets><datapoint pos="0" key="21"/></targets>
              </component>"#,
        )?;
        let function = read_function(&function_document.root_element());
        let mut components = Vec::new();
        let mut warnings = Vec::new();
        install_fallback(
            &mut components,
            vec![candidate().ok_or_else(|| std::io::Error::other("missing candidate"))?],
            &BTreeSet::from([30]),
            &BTreeMap::from([(20, 10), (30, 21)]),
            &[function],
            &mut warnings,
        );

        assert_eq!(components.len(), 1);
        assert!(components[0].is_source);
        assert_eq!(components[0].name, "ExternalRows");
        assert!(warnings.is_empty());
        assert!(components[0].options.external_source.is_some());
        Ok(())
    }
}
