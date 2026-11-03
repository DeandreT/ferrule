//! A late structural seal of the graph the certified producer actually built.
//! Digests compare mutations, never establish raw producer authority.
use super::super::{Registry, limits::ConstructionBudget};
use mapping::{Graph, Node, NodeId};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(in crate::import) struct ArgumentSeal {
    nodes: BTreeMap<NodeId, [u8; 32]>,
    budget: Option<ConstructionBudget>,
}

impl Clone for ArgumentSeal {
    fn clone(&self) -> Self {
        let Some(budget) = &self.budget else {
            return Self::default();
        };
        if budget.claim(self.nodes.len(), 0, 0).is_err() {
            // The shared quota is sticky and is checked before publication.
            // A speculative builder cannot allocate a copied proof on failure.
            return Self::default();
        }
        Self {
            nodes: self.nodes.clone(),
            budget: Some(budget.clone()),
        }
    }
}

struct Writer<'a> {
    digest: Sha256,
    budget: &'a ConstructionBudget,
}

impl std::io::Write for Writer<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.budget
            .claim(0, bytes.len(), 0)
            .map_err(std::io::Error::other)?;
        self.digest.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn fingerprint(
    value: &impl serde::Serialize,
    budget: &ConstructionBudget,
) -> Result<[u8; 32], String> {
    let mut writer = Writer {
        digest: Sha256::new(),
        budget,
    };
    serde_json::to_writer(&mut writer, value).map_err(|_| {
        "declared-library materialized argument seal exceeded its structural budget".to_string()
    })?;
    Ok(writer.digest.finalize().into())
}

// JSON alone maps nonfinite floats to null. Retain exact floating identity in
// the seal without changing lazy scalar-error behavior or copying payloads.
fn float_identity(
    node: &Node,
    digest: &mut Sha256,
    budget: &ConstructionBudget,
) -> Result<(), String> {
    let mut value = |value: &ir::Value| {
        if let ir::Value::Float(value) = value {
            digest.update([1]);
            digest.update(value.to_bits().to_le_bytes());
        } else {
            digest.update([0]);
        }
    };
    match node {
        Node::Const { value: constant } => {
            budget.claim(1, 0, 0)?;
            value(constant);
        }
        Node::ValueMap { table, default, .. } => {
            budget.claim(
                table
                    .len()
                    .checked_mul(2)
                    .and_then(|n| n.checked_add(1))
                    .ok_or("argument seal work overflow")?,
                0,
                0,
            )?;
            for (key, item) in table {
                value(key);
                value(item);
            }
            if let Some(default) = default {
                value(default);
            }
        }
        _ => {}
    }
    Ok(())
}

fn fingerprint_node(node: &Node, budget: &ConstructionBudget) -> Result<[u8; 32], String> {
    let mut digest = Sha256::new();
    digest.update(fingerprint(node, budget)?);
    float_identity(node, &mut digest, budget)?;
    Ok(digest.finalize().into())
}

pub(super) fn fingerprint_functions(
    functions: &BTreeMap<mapping::FunctionId, mapping::UserFunction>,
    budget: &ConstructionBudget,
) -> Result<[u8; 32], String> {
    let mut digest = Sha256::new();
    digest.update(fingerprint(functions, budget)?);
    for function in functions.values() {
        for node in function.body.nodes.values() {
            float_identity(node, &mut digest, budget)?;
        }
    }
    Ok(digest.finalize().into())
}

impl ArgumentSeal {
    fn capture(
        &mut self,
        graph: &Graph,
        root: NodeId,
        budget: &ConstructionBudget,
    ) -> Result<(), String> {
        self.budget = Some(budget.clone());
        budget.claim(1, 0, 0)?;
        let mut pending = vec![root];
        let mut seen = BTreeSet::new();
        while let Some(id) = pending.pop() {
            budget.claim(1, 0, 0)?;
            if !seen.insert(id) {
                continue;
            }
            let node = graph
                .nodes
                .get(&id)
                .ok_or("declared-library materialized argument is missing")?;
            match node {
                Node::Const { .. }
                | Node::Unconnected
                | Node::SourceField { .. }
                | Node::Call { .. }
                | Node::UserFunctionCall { .. }
                | Node::If { .. }
                | Node::Raise { .. }
                | Node::ValueMap { .. } => {}
                _ => {
                    return Err(
                        "declared-library materialized argument has an unproved node kind".into(),
                    );
                }
            }
            let digest = fingerprint_node(node, budget)?;
            if self
                .nodes
                .get(&id)
                .is_some_and(|expected| expected != &digest)
            {
                return Err(
                    "declared-library materialized argument changed during construction".into(),
                );
            }
            if let std::collections::btree_map::Entry::Vacant(entry) = self.nodes.entry(id) {
                budget.claim(1, 0, 0)?;
                entry.insert(digest);
            }
            let mut status = Ok(());
            node.visit_inputs(&mut |input| {
                if status.is_ok() {
                    status = budget.claim(1, 0, 0);
                    if status.is_ok() {
                        pending.push(input.node);
                    }
                }
            });
            status?;
        }
        Ok(())
    }

    pub(super) fn audit(&self, graph: &Graph, budget: &ConstructionBudget) -> Result<(), String> {
        for (&id, expected) in &self.nodes {
            budget.claim(1, 0, 0)?;
            let node = graph
                .nodes
                .get(&id)
                .ok_or("declared-library final materialized argument is missing")?;
            if &fingerprint_node(node, budget)? != expected {
                return Err("declared-library final materialized argument kind/path/frame/function/ordered edges differ from its seal".into());
            }
        }
        Ok(())
    }
}

impl Registry {
    pub(in crate::import) fn seal_materialized_call(
        &self,
        seal: &mut ArgumentSeal,
        graph: &Graph,
        node: NodeId,
    ) -> Result<(), String> {
        let budget = self
            .construction
            .as_ref()
            .ok_or("declared-library materialization has no construction budget")?;
        seal.capture(graph, node, budget)
            .map_err(|error| budget.refuse(error))
    }

    pub(in crate::import) fn audit_materialized_arguments(
        &self,
        seal: &ArgumentSeal,
        graph: &Graph,
    ) -> Result<(), String> {
        match &self.construction {
            Some(budget) => seal.audit(graph, budget),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
std::thread_local! { static SPECULATIVE_PROBE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

#[cfg(test)]
pub(in crate::import) fn probe_speculative_builder(
    builder: &crate::import::graph::GraphBuilder<'_>,
) {
    if !SPECULATIVE_PROBE.with(|probe| probe.replace(false)) {
        return;
    }
    let budget = builder.udf_registry.construction.as_ref().unwrap();
    let before = budget.remaining_for_test();
    assert!(builder.graph.nodes.is_empty());
    assert!(builder.materialized_arguments.nodes.is_empty());
    let mut trial = builder.clone();
    assert!(std::ptr::eq(builder.udf_registry, trial.udf_registry));
    // Deliberately make speculative IDs disagree with the eventual parent.
    trial.alloc(Node::Const {
        value: ir::Value::String("discarded speculation".into()),
    });
    let (&output, &(call, component)) = trial.udf_by_output.iter().next().unwrap();
    assert!(trial.udf_calls[call].certificate.is_some());
    let node = trial.udf_output_node(output, call, component).unwrap();
    assert!(trial.materialized_arguments.nodes.contains_key(&node));
    assert!(trial.materialized_arguments.nodes.len() >= 2);
    let after = budget.remaining_for_test();
    assert!(after.0 < before.0 && after.1 < before.1);
    drop(trial);
    assert!(builder.materialized_arguments.nodes.is_empty());
    assert!(builder.graph.nodes.is_empty());
    assert_eq!(builder.next_id, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abandoned_speculative_builder_seals_do_not_pollute_a_complete_import_or_run() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        struct Fixture(std::path::PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let fixture = Fixture(std::env::temp_dir().join(format!(
            "ferrule_speculative_seal_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )));
        std::fs::create_dir(&fixture.0).unwrap();
        for (file, root, leaf) in [("in.xsd", "Input", "Text"), ("out.xsd", "Output", "Result")] {
            std::fs::write(fixture.0.join(file), format!(r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="{root}"><xs:complexType><xs:sequence><xs:element name="{leaf}" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#)).unwrap();
        }
        std::fs::write(fixture.0.join("helpers.mfd"), r#"<mapping><component name="Identity" library="generic" inline="0"><structure><children><component name="value" library="core" kind="6" uid="100"><targets><datapoint pos="0" key="1000"/></targets><data><input datatype="string"/></data></component><component name="result" library="core" kind="7" uid="101"><sources><datapoint pos="0" key="1001"/></sources><data><output datatype="string"/></data></component></children><graph directed="1"><vertices><vertex vertexkey="1000"><edges><edge vertexkey="1001"/></edges></vertex></vertices></graph></structure></component></mapping>"#).unwrap();
        let path = fixture.0.join("mapping.mfd");
        std::fs::write(&path, r#"<mapping version="26"><settings><libraries><import path="helpers.mfd" saverelative="1"/></libraries></settings><component name="main"><structure><children><component name="Input" library="xml" uid="1" kind="14"><data><root><entry name="Input"><entry name="Text" outkey="10"/></entry></root><document schema="in.xsd" instanceroot="{}Input"/></data></component><component name="Identity" library="generic" uid="2" kind="19"><data><root><entry name="value" inpkey="20" componentid="100"/></root><root rootindex="1"><entry name="result" outkey="21" componentid="101"/></root></data></component><component name="Output" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Output"><entry name="Result" inpkey="30"/></entry></root><document schema="out.xsd" instanceroot="{}Output"/></data></component></children><graph directed="1"><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="21"><edges><edge vertexkey="30"/></edges></vertex></vertices></graph></structure></component></mapping>"#).unwrap();
        SPECULATIVE_PROBE.with(|probe| probe.set(true));
        let actual = crate::import(&path).unwrap();
        assert!(!SPECULATIVE_PROBE.with(std::cell::Cell::get));
        assert!(actual.warnings.is_empty());
        assert_eq!(
            actual.project.execution_contract,
            Some(mapping::ExecutionContract::ClosedPrimarySourceV1)
        );
        assert_eq!(actual.project.graph.nodes.len(), 2);
        assert!(matches!(
            actual.project.graph.nodes.get(&0),
            Some(Node::SourceField { .. })
        ));
        let control = crate::import(&path).unwrap();
        assert_eq!(
            serde_json::to_value(&actual.project).unwrap(),
            serde_json::to_value(&control.project).unwrap()
        );
        assert!(engine::validate(&actual.project).is_empty());
        let input = format_xml::from_str(
            "<Input><Text>legitimate</Text></Input>",
            &actual.project.source,
        )
        .unwrap();
        let output = engine::run(&actual.project, &input).unwrap();
        assert_eq!(
            output.field("Result").and_then(ir::Instance::as_scalar),
            Some(&ir::Value::String("legitimate".into()))
        );
    }

    #[test]
    fn speculative_seal_cloning_reserves_before_copying_and_failure_is_sticky() {
        let budget = ConstructionBudget::for_test(0, 0);
        let seal = ArgumentSeal {
            nodes: BTreeMap::from([(0, [0; 32])]),
            budget: Some(budget.clone()),
        };
        let copy = seal.clone();
        assert!(copy.nodes.is_empty());
        assert!(budget.finish().is_err());
    }

    #[test]
    fn scalar_identity_seal_distinguishes_nonfinite_bits_and_null_without_evaluation() {
        let budget = ConstructionBudget::default();
        let nan = Node::Const {
            value: ir::Value::Float(f64::NAN),
        };
        let null = Node::Const {
            value: ir::Value::Null,
        };
        assert_ne!(
            fingerprint_node(&nan, &budget).unwrap(),
            fingerprint_node(&null, &budget).unwrap()
        );
        let other_nan = Node::Const {
            value: ir::Value::Float(f64::from_bits(f64::NAN.to_bits() ^ 1)),
        };
        assert_ne!(
            fingerprint_node(&nan, &budget).unwrap(),
            fingerprint_node(&other_nan, &budget).unwrap()
        );
    }

    #[test]
    fn argument_mutations_refuse_without_changing_the_output_call_binding() {
        let graph = Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Text".into()],
                        frame: Some(vec!["Row".into()]),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: ir::Value::String("suffix".into()),
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![0, 1],
                    },
                ),
                (
                    3,
                    Node::UserFunctionCall {
                        function: mapping::FunctionId::new(1),
                        args: vec![2],
                    },
                ),
            ]),
        };
        let budget = ConstructionBudget::default();
        let mut seal = ArgumentSeal::default();
        seal.capture(&graph, 3, &budget).unwrap();
        seal.audit(&graph, &budget).unwrap();
        for mutation in 0..7 {
            let mut changed = graph.clone();
            match mutation {
                0 => {
                    if let Node::SourceField { path, .. } = changed.nodes.get_mut(&0).unwrap() {
                        path[0] = "Other".into();
                    }
                }
                1 => {
                    if let Node::SourceField { frame, .. } = changed.nodes.get_mut(&0).unwrap() {
                        *frame = None;
                    }
                }
                2 => {
                    changed.nodes.insert(
                        0,
                        Node::Const {
                            value: ir::Value::Null,
                        },
                    );
                }
                3 => {
                    if let Node::Call { args, .. } = changed.nodes.get_mut(&2).unwrap() {
                        args.reverse();
                    }
                }
                4 => {
                    if let Node::Call { function, .. } = changed.nodes.get_mut(&2).unwrap() {
                        *function = "coerce_string".into();
                    }
                }
                5 => {
                    if let Node::UserFunctionCall { args, .. } = changed.nodes.get_mut(&3).unwrap()
                    {
                        args[0] = 0;
                    }
                }
                _ => {
                    if let Node::UserFunctionCall { function, .. } =
                        changed.nodes.get_mut(&3).unwrap()
                    {
                        *function = mapping::FunctionId::new(2);
                    }
                }
            }
            assert!(seal.audit(&changed, &budget).is_err());
        }
    }
}
