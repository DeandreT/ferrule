//! Physical component dependencies, independent of importer recipe families.

use std::collections::{BTreeMap, BTreeSet};

use super::super::schema::parse_u32;

pub(super) struct RawDependencies {
    components_by_input: BTreeMap<u32, Vec<usize>>,
    outputs: Vec<BTreeSet<u32>>,
}

impl RawDependencies {
    pub(super) fn read(structure: roxmltree::Node<'_, '_>) -> Self {
        let mut components_by_input = BTreeMap::<u32, Vec<usize>>::new();
        let mut component_outputs = Vec::new();
        for component in structure
            .children()
            .filter(|node| node.has_tag_name("children"))
            .flat_map(|children| children.children())
            .filter(|node| node.has_tag_name("component"))
        {
            let mut inputs = BTreeSet::new();
            let mut outputs = BTreeSet::new();
            let mut pending = vec![component];
            while let Some(node) = pending.pop() {
                if !node.is_element() || (node != component && node.has_tag_name("component")) {
                    continue;
                }
                // Nested components own their own ports. Skip their complete
                // subtrees instead of walking every node's ancestor chain.
                pending.extend(node.children());
                inputs.extend(parse_u32(node.attribute("inpkey")));
                outputs.extend(parse_u32(node.attribute("outkey")));
                if node.has_tag_name("datapoint") {
                    match node.parent().map(|parent| parent.tag_name().name()) {
                        Some("sources") => inputs.extend(parse_u32(node.attribute("key"))),
                        Some("targets") => outputs.extend(parse_u32(node.attribute("key"))),
                        _ => {}
                    }
                }
            }
            // A missing argument can activate a UDF default, and an omitted
            // serializer field can still produce an object. Treat every output
            // as dependent, even if a specialized reader drops this input.
            if !outputs.is_empty() {
                let index = component_outputs.len();
                component_outputs.push(outputs);
                for input in inputs {
                    components_by_input.entry(input).or_default().push(index);
                }
            }
        }
        Self {
            components_by_input,
            outputs: component_outputs,
        }
    }

    pub(super) fn extend_outputs(
        &self,
        input: u32,
        expanded: &mut BTreeSet<usize>,
        pending: &mut Vec<u32>,
    ) {
        for &component in self.components_by_input.get(&input).into_iter().flatten() {
            // A broad record variable can have thousands of inputs/outputs.
            // Store its outputs once and expand it once per refusal closure.
            if expanded.insert(component) {
                pending.extend(&self.outputs[component]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outputs(dependencies: &RawDependencies, input: u32) -> Vec<u32> {
        let mut outputs = Vec::new();
        dependencies.extend_outputs(input, &mut BTreeSet::new(), &mut outputs);
        outputs.sort_unstable();
        outputs.dedup();
        outputs
    }

    #[test]
    fn physical_ports_cover_specialized_families_without_a_kind_allowlist() {
        let xml = r#"<structure><children>
          <component name="udf" library="helpers" kind="19"><data><root><entry inpkey="1"/></root><root><entry outkey="2"/></root></data></component>
          <component name="serializer" library="json" kind="31"><data><root><entry outkey="4"><entry inpkey="3"/><entry inpkey="5"/></entry></root></data></component>
          <component name="parser" library="json" kind="31"><data><root><entry inpkey="6"><entry outkey="7"/><entry outkey="8"/></entry></root></data></component>
          <component name="external" library="custom" kind="19"><sources><datapoint key="9"/></sources><targets><datapoint key="10"/></targets></component>
          <component name="xslt" library="xslt"><sources><datapoint key="11"/></sources><targets><datapoint key="12"/></targets></component>
          <component name="service" library="webservice" kind="20"><data><root><entry inpkey="13"/><entry outkey="14"/></root></data></component>
          <component name="dynamic" library="xml" kind="14"><data><root><entry inpkey="15"><entry outkey="16"/></entry></root></data></component>
          <component name="unknown-future-kind" kind="999"><sources><datapoint key="17"/></sources><targets><datapoint key="18"/></targets></component>
          <component name="unrelated" library="core" kind="2"><targets><datapoint key="19"/></targets></component>
        </children></structure>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let dependencies = RawDependencies::read(doc.root_element());
        for (input, expected) in [
            (1, vec![2]),
            (3, vec![4]),
            (5, vec![4]),
            (6, vec![7, 8]),
            (9, vec![10]),
            (11, vec![12]),
            (13, vec![14]),
            (15, vec![16]),
            (17, vec![18]),
        ] {
            assert_eq!(outputs(&dependencies, input), expected);
        }
        assert!(outputs(&dependencies, 19).is_empty());
        let mut expanded = BTreeSet::new();
        let mut pending = Vec::new();
        dependencies.extend_outputs(3, &mut expanded, &mut pending);
        dependencies.extend_outputs(5, &mut expanded, &mut pending);
        assert_eq!(pending, [4]);
    }

    #[test]
    fn nested_component_scopes_and_neighbor_definitions_do_not_leak_ports() {
        let xml = r#"<mapping><component name="main"><structure><children>
          <component name="outer"><sources><datapoint key="1"/></sources><targets><datapoint key="2"/></targets><structure><children>
            <component name="inner"><sources><datapoint key="3"/></sources><targets><datapoint key="4"/></targets></component>
          </children></structure></component>
        </children></structure></component>
        <component name="definition"><structure><children><component><sources><datapoint key="1"/></sources><targets><datapoint key="99"/></targets></component></children></structure></component>
        </mapping>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let structure = doc
            .descendants()
            .find(|node| node.has_tag_name("structure"))
            .unwrap();
        let dependencies = RawDependencies::read(structure);
        assert_eq!(outputs(&dependencies, 1), [2]);
        assert!(outputs(&dependencies, 3).is_empty());
    }

    #[test]
    fn deeply_wrapped_ports_do_not_require_per_node_ancestor_searches() {
        let levels = 60_000;
        let mut xml = String::from("<structure><children><component>");
        xml.push_str(&"<n>".repeat(levels));
        xml.push_str("<entry inpkey=\"1\" outkey=\"2\"/>");
        xml.push_str(&"</n>".repeat(levels));
        xml.push_str("</component></children></structure>");
        let document = roxmltree::Document::parse(&xml).unwrap();
        let dependencies = RawDependencies::read(document.root_element());
        assert_eq!(outputs(&dependencies, 1), [2]);
    }
}
