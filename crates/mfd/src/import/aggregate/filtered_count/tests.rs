use super::*;
use std::{cell::Cell, path::PathBuf};

mod input_routing;
mod receiver_contexts;
mod resource_followup;
mod review_regressions;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Probe {
    None,
    GuardedMix,
    SpeculativeOnly,
    SpeculativeRefusal,
    Remove,
    Duplicate,
    Replace,
    ExtraConsumer,
    Driver,
    Group,
    Copy,
    Dynamic,
    Node,
    Source,
    Target,
}
thread_local! { static PROBE: Cell<Probe> = const { Cell::new(Probe::None) }; }
thread_local! { static LIMITS: Cell<Option<(usize, usize)>> = const { Cell::new(None) }; }
pub(in crate::import) fn construction_limits() -> Option<(usize, usize)> {
    LIMITS.get()
}
struct LimitsGuard;
impl Drop for LimitsGuard {
    fn drop(&mut self) {
        LIMITS.set(None);
    }
}
struct ProbeGuard;
impl Drop for ProbeGuard {
    fn drop(&mut self) {
        PROBE.set(Probe::None);
    }
}
fn probe(mode: Probe) -> ProbeGuard {
    PROBE.set(mode);
    ProbeGuard
}
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/filtered-count-receivers/row-cell-fanout.mfd")
}

pub(super) fn probe_binding(
    builder: &mut GraphBuilder<'_>,
    target: &SchemaComponent,
    input: u32,
    feed: u32,
    path: &[String],
    scopes: &ScopeBuilder,
) -> Option<CountBinding> {
    let mode = PROBE.get();
    if !matches!(mode, Probe::SpeculativeOnly | Probe::SpeculativeRefusal) {
        return None;
    }
    PROBE.set(Probe::None);
    let receipts = builder.materialized_counts.receipts.len();
    let failure = builder.materialized_counts.failure.clone();
    let mut trial = builder.clone();
    let mut trial_scopes = scopes.clone();
    if mode == Probe::SpeculativeRefusal {
        trial_scopes.root.group_by = Some(0);
    }
    let result = trial.filtered_count_binding(target, input, feed, path, &trial_scopes);
    if mode == Probe::SpeculativeOnly {
        assert!(matches!(result, CountBinding::Handled(_)));
        assert_eq!(trial.materialized_counts.receipts.len(), receipts + 1);
    } else {
        assert!(matches!(result, CountBinding::Refused));
        assert!(trial.materialized_counts.failure.is_some());
    }
    assert_eq!(builder.materialized_counts.receipts.len(), receipts);
    assert_eq!(builder.materialized_counts.failure, failure);
    PROBE.set(mode);
    if mode == Probe::SpeculativeOnly {
        Some(CountBinding::Refused)
    } else {
        None
    }
}

pub(super) fn probe_audit(
    table: &CountProofTable,
    seal: &CountSeal,
    project: &mapping::Project,
    descriptors: &[&SchemaComponent],
    declared_guard: bool,
) -> Option<Result<(), String>> {
    review_regressions::audit_disjoint(table);
    let mode = PROBE.get();
    if mode == Probe::GuardedMix {
        assert!(declared_guard);
        assert_eq!(seal.receipts.len(), 1);
        assert!(seal.receipts.values().all(|r| r.mode == Mode::CurrentItem));
        assert!(
            !project
                .graph
                .nodes
                .values()
                .any(|n| matches!(n, Node::Aggregate { .. }))
        );
        let mut guarded = project.clone();
        guarded.execution_contract = Some(mapping::ExecutionContract::ClosedPrimarySourceV1);
        assert!(guarded.validate_execution_contract().is_ok());
        return None;
    }
    if matches!(
        mode,
        Probe::None | Probe::SpeculativeOnly | Probe::SpeculativeRefusal
    ) {
        return None;
    }
    assert!(
        seal.failure.is_none(),
        "normal materialization must precede the late mutation"
    );
    assert_eq!(seal.receipts.len(), 3);
    assert!(
        table
            .audit_inner(seal, project, descriptors, declared_guard)
            .is_ok(),
        "unmodified final graph must seal"
    );
    let mut changed = project.clone();
    let receipt = seal.receipts.values().next().unwrap();
    match mode {
        Probe::Remove => changed.root.bindings.retain(|b| b.node != receipt.root),
        Probe::Duplicate => {
            let b = changed
                .root
                .bindings
                .iter()
                .find(|b| b.node == receipt.root)
                .unwrap()
                .clone();
            changed.root.bindings.push(b);
        }
        Probe::Replace => {
            changed
                .root
                .bindings
                .iter_mut()
                .find(|b| b.node == receipt.root)
                .unwrap()
                .node = 0
        }
        Probe::ExtraConsumer => changed.root.bindings.push(mapping::Binding {
            target_field: "Unplanned".into(),
            node: receipt.root,
        }),
        Probe::Driver => {
            changed
                .root
                .children
                .iter_mut()
                .find(|s| s.target_field == "Row")
                .unwrap()
                .iteration = mapping::ScopeIteration::Source(vec!["Cell".into()])
        }
        Probe::Group => changed.root.children[0].group_by = Some(receipt.root),
        Probe::Copy => {
            changed.root.children[0].construction = mapping::ScopeConstruction::CopyCurrentSource
        }
        Probe::Dynamic => changed.root.merge_dynamic_fields = true,
        Probe::Node => {
            changed.graph.nodes.insert(
                receipt.root,
                Node::Const {
                    value: Value::Int(0),
                },
            );
        }
        Probe::Source => changed.source.name.push('x'),
        Probe::Target => changed.target.name.push('x'),
        _ => unreachable!(),
    }
    Some(table.audit_inner(seal, &changed, descriptors, declared_guard))
}

fn refused(result: Result<crate::Imported, crate::MfdError>) {
    match result {
        Err(crate::MfdError::UnsupportedImport(reason)) => {
            assert!(reason.contains("physical filtered count"), "{reason}")
        }
        Err(other) => panic!("expected Count publication refusal, got {other:?}"),
        Ok(_) => panic!("unsafe Count published a project"),
    }
}

#[test]
fn final_seal_rejects_every_late_receiver_binding_boundary_and_node_mutation() {
    for mode in [
        Probe::Remove,
        Probe::Duplicate,
        Probe::Replace,
        Probe::ExtraConsumer,
        Probe::Driver,
        Probe::Group,
        Probe::Copy,
        Probe::Dynamic,
        Probe::Node,
        Probe::Source,
        Probe::Target,
    ] {
        let _guard = probe(mode);
        refused(crate::import(&fixture()));
        let strict = crate::import_with_profile(
            &fixture(),
            &crate::ImportOptions::default(),
            crate::ImportProfile::Executable,
        );
        assert!(matches!(strict, Err(crate::MfdError::UnsupportedImport(_))));
    }
}

#[test]
fn speculative_builder_receipts_cannot_authorize_the_selected_builder() {
    let _guard = probe(Probe::SpeculativeOnly);
    refused(crate::import(&fixture()));
}

#[test]
fn abandoned_builder_refusal_does_not_poison_the_selected_builder() {
    let _guard = probe(Probe::SpeculativeRefusal);
    let imported = crate::import(&fixture()).unwrap();
    assert!(imported.warnings.is_empty());
    assert!(imported.project.execution_contract.is_none());
}

#[test]
fn sticky_quota_and_exact_path_limits_fail_before_copying() {
    let budget = CountConstruction::for_test(3, 4);
    assert_eq!(copy_path(&["abcd".into()], &budget).unwrap(), ["abcd"]);
    assert_eq!(budget.remaining_for_test(), (1, 0));
    assert!(budget.claim(2, 0, 0).is_err());
    assert!(budget.claim(0, 0, 0).is_err());
    let short = CountConstruction::for_test(1, 4);
    assert!(copy_path(&["abcd".into()], &short).is_err());
    assert!(short.claim(0, 0, 0).is_err());
    let path = vec!["a".repeat(4096)];
    assert!(copy_path(&path, &CountConstruction::default()).is_ok());
    assert!(copy_path(&["a".repeat(4097)], &CountConstruction::default()).is_err());
    assert!(CountConstruction::default().claim(0, 0, 256).is_err());
}

#[test]
fn raw_xml_namespace_slots_and_instance_roots_are_not_repairable_proof_inputs() {
    let ordinary = r#"<component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Row"><entry name="Text" outkey="11"/></entry></entry></root><document instanceroot="{}Source"/></data></component>"#;
    for (text, accepted) in [
        (ordinary.to_string(), true),
        (
            ordinary.replace(
                "<root>",
                r#"<root><header><namespaces><namespace uid=""/></namespaces></header>"#,
            ),
            true,
        ),
        (ordinary.replace("name=\"Text\"", "name=\"1:Text\""), false),
        (
            ordinary.replace(
                "<root>",
                r#"<root><header><namespaces><namespace uid="urn:other"/></namespaces></header>"#,
            ),
            false,
        ),
        (ordinary.replace("{}Source", "{}Other"), false),
    ] {
        let doc = roxmltree::Document::parse(&text).unwrap();
        let pin = raw::Pin {
            owner: doc.root_element(),
            node: doc
                .descendants()
                .find(|n| n.attribute("outkey") == Some("11"))
                .unwrap(),
            output: true,
        };
        assert_eq!(
            raw::path(pin, &CountConstruction::default(), "Source").is_ok(),
            accepted
        );
    }
}

#[test]
fn a_nillable_group_endpoint_cannot_supply_an_ordinary_receiver_instance() {
    let package = Package::new(DESIGN);
    let schema_path = package.0.join("row-cell-source.xsd");
    let schema = std::fs::read_to_string(&schema_path).unwrap().replace(
        r#"<xs:element name="Cell" minOccurs="0" maxOccurs="unbounded">"#,
        r#"<xs:element name="Cell" minOccurs="0" maxOccurs="unbounded" nillable="true">"#,
    );
    std::fs::write(schema_path, schema).unwrap();
    refused(package.import());
    let result = crate::import_with_profile(
        &package.0.join("map.mfd"),
        &crate::ImportOptions::default(),
        crate::ImportProfile::Executable,
    );
    assert!(matches!(result, Err(crate::MfdError::UnsupportedImport(_))));
}

#[test]
fn whole_import_exhaustion_refuses_the_new_route_but_keeps_ordinary_counts() {
    let ordinary = Package::new(&DESIGN.replace("from=\"42\" to=\"50\"", "from=\"10\" to=\"50\""));
    let shadowed = Package::new(
        &DESIGN
            .replace(
                r#"name="filter" library="core""#,
                r#"name="filter" library="unknown""#,
            )
            .replace(
                r#"<entry name="Cell"><entry name="Label"/>"#,
                r#"<entry name="Cell" outkey="42"><entry name="Label"/>"#,
            ),
    );
    let shadowed_output = Package::new(&DESIGN.replace(
        r#"<entry name="Cell"><entry name="Label"/>"#,
        r#"<entry name="Cell" outkey="51"><entry name="Label"/>"#,
    ));
    for limits in [(0, 0), (2000, 32 * 1024 * 1024), (65_536, 1)] {
        let _guard = LimitsGuard;
        LIMITS.set(Some(limits));
        refused(crate::import(&fixture()));
        refused(shadowed.import());
        refused(shadowed_output.import());
        let result = crate::import_with_profile(
            &fixture(),
            &crate::ImportOptions::default(),
            crate::ImportProfile::Executable,
        );
        assert!(matches!(result, Err(crate::MfdError::UnsupportedImport(_))));
        let imported = ordinary.import().unwrap();
        assert!(imported.project.execution_contract.is_none());
        assert!(COUNTERS.with(|c| c.borrow()[Counter::QuotaFailure as usize] > 0));
    }
}

struct Package(PathBuf);
impl Package {
    fn new(design: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "ferrule_count_proof_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        for entry in std::fs::read_dir(fixture().parent().unwrap()).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), directory.join(entry.file_name())).unwrap();
        }
        std::fs::write(directory.join("map.mfd"), design).unwrap();
        Self(directory)
    }
    fn import(&self) -> Result<crate::Imported, crate::MfdError> {
        crate::import(&self.0.join("map.mfd"))
    }
}
impl Drop for Package {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
const DESIGN: &str =
    include_str!("../../../../tests/fixtures/filtered-count-receivers/row-cell-fanout.mfd");

fn counts() -> [usize; 24] {
    COUNTERS.with(|c| *c.borrow())
}
fn reset() {
    COUNTERS.with(|c| *c.borrow_mut() = [0; 24]);
}

#[test]
fn one_added_raw_and_final_census_is_demanded_not_per_function_or_count() {
    for unrelated in [0, 24] {
        let mut existing_edge_reads = None;
        let mut design = DESIGN.to_string();
        let mut functions = String::new();
        for n in 0..unrelated {
            functions.push_str(&format!(r#"<component name="equal" library="core" kind="5"><sources><datapoint key="{}"/><datapoint pos="1" key="{}"/></sources><targets><datapoint key="{}"/></targets></component>"#, 1000 + n*3, 1001 + n*3, 1002 + n*3));
        }
        design = design.replace("</children>", &format!("{functions}</children>"));
        for filtered in [false, true] {
            let current = if filtered {
                design.clone()
            } else {
                design.replace("from=\"42\" to=\"50\"", "from=\"10\" to=\"50\"")
            };
            let package = Package::new(&current);
            reset();
            package.import().unwrap();
            let c = counts();
            assert_eq!(c[Counter::RawBuild as usize], usize::from(filtered));
            assert_eq!(c[Counter::FinalBuild as usize], usize::from(filtered));
            if filtered {
                assert_eq!(
                    Some(c[Counter::EdgeIndex as usize]),
                    existing_edge_reads,
                    "Count must not rebuild the legacy edge index"
                );
            } else {
                existing_edge_reads = Some(c[Counter::EdgeIndex as usize]);
            }
            if filtered {
                assert_eq!(c[Counter::Lower as usize], 3);
                assert_eq!(c[Counter::Template as usize], 1);
            }
            // The added inventory visits are bounded by this small authored
            // package, independent of the number of local grammar checks.
            assert!(c[Counter::RawElement as usize] < 500);
        }
    }
}

#[test]
fn multiple_demanded_counts_still_build_one_pair_of_authorities() {
    let design = DESIGN.replace("</children>", r#"<component name="count" library="core" kind="5"><sources><datapoint/><datapoint pos="1" key="60"/></sources><targets><datapoint key="61"/></targets></component></children>"#)
        .replace(r#"<entry name="Total" inpkey="100"/>"#, r#"<entry name="Extra" inpkey="101"/><entry name="Total" inpkey="100"/>"#)
        .replace("</connections>", r#"<edge from="42" to="60"/><edge from="61" to="101"/></connections>"#);
    let package = Package::new(&design);
    let schema_path = package.0.join("row-cell-target.xsd");
    let schema = std::fs::read_to_string(&schema_path).unwrap().replace(r#"<xs:element name="Total" type="xs:integer"/>"#, r#"<xs:element name="Extra" type="xs:integer"/><xs:element name="Total" type="xs:integer"/>"#);
    std::fs::write(schema_path, schema).unwrap();
    reset();
    package.import().unwrap();
    let c = counts();
    assert_eq!(c[Counter::RawBuild as usize], 1);
    assert_eq!(c[Counter::FinalBuild as usize], 1);
    assert_eq!(c[Counter::Lower as usize], 4);
    assert_eq!(c[Counter::Template as usize], 2);
}

#[test]
fn a_source_named_count_is_only_an_optional_capture_trigger() {
    let base = DESIGN.replace(
        r#"name="count" library="core""#,
        r#"name="sum" library="core""#,
    );
    let normal = Package::new(&base).import().unwrap();
    let renamed = Package::new(&base.replace(
        r#"name="source" library="xml""#,
        r#"name="count" library="xml""#,
    ));
    for limits in [None, Some((0, 0))] {
        let _guard = LimitsGuard;
        LIMITS.set(limits);
        reset();
        let imported = renamed.import().unwrap();
        assert_eq!(
            mapping::project_file::encode_pretty(&normal.project).unwrap(),
            mapping::project_file::encode_pretty(&imported.project).unwrap()
        );
        let c = counts();
        assert_eq!(c[Counter::RawBuild as usize], 0);
        assert_eq!(c[Counter::FinalBuild as usize], 0);
    }
}

#[test]
fn current_item_only_count_still_refuses_guarded_udf_composition_after_sum_pruning() {
    const HELPER: &str = r#"<mapping><component name="Identity" library="generic" inline="0"><structure><children>
<component name="value" library="core" kind="6" uid="100"><targets><datapoint pos="0" key="1000"/></targets><data><input datatype="string"/></data></component>
<component name="result" library="core" kind="7" uid="101"><sources><datapoint pos="0" key="1001"/></sources><data><output datatype="string"/></data></component>
</children><graph directed="1"><vertices><vertex vertexkey="1000"><edges><edge vertexkey="1001"/></edges></vertex></vertices></graph></structure></component></mapping>"#;
    const COUNT: &str = r#"<component name="constant" library="core" kind="2"><targets><datapoint key="20"/></targets><data><constant value="keep" datatype="string"/></data></component>
<component name="equal" library="core" kind="5"><sources><datapoint pos="0" key="30"/><datapoint pos="1" key="31"/></sources><targets><datapoint key="32"/></targets></component>
<component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="40"/><datapoint pos="1" key="41"/></sources><targets><datapoint key="42"/></targets></component>
<component name="count" library="core" kind="5"><sources><datapoint/><datapoint pos="1" key="50"/></sources><targets><datapoint key="51"/></targets></component>"#;
    for counted in [false, true] {
        let count_entry = if counted {
            r#"<entry name="Count" inpkey="111"/>"#
        } else {
            ""
        };
        let count_schema = if counted {
            r#"<xs:element name="Count" type="xs:integer"/>"#
        } else {
            ""
        };
        let count_edges = if counted {
            r#"<edge from="11" to="30"/><edge from="20" to="31"/><edge from="11" to="40"/><edge from="32" to="41"/><edge from="42" to="50"/><edge from="51" to="111"/>"#
        } else {
            ""
        };
        let count_components = if counted { COUNT } else { "" };
        let design = format!(
            r#"<mapping version="26"><settings><libraries><import path="helpers.mfd" saverelative="1"/></libraries></settings><component name="map"><structure><children>
<component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Row" outkey="10"><entry name="Text" outkey="11"/></entry></entry></root><document schema="mixed-source.xsd" instanceroot="{{}}Source"/></data></component>
{count_components}
<component name="Identity" library="generic" kind="19"><data><root><entry name="value" inpkey="60" componentid="100"/></root><root rootindex="1"><entry name="result" outkey="61" componentid="101"/></root></data></component>
<component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Results"><entry name="Row" inpkey="110">{count_entry}<entry name="Text" inpkey="112"/></entry></entry></root><document schema="mixed-target.xsd" instanceroot="{{}}Results"/></data></component>
</children><connections><edge from="10" to="110"/><edge from="11" to="60"/><edge from="61" to="112"/>{count_edges}</connections></structure></component></mapping>"#
        );
        let package = Package::new(&design);
        std::fs::write(package.0.join("helpers.mfd"), HELPER).unwrap();
        std::fs::write(package.0.join("mixed-source.xsd"), r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Row" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Text" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#).unwrap();
        std::fs::write(package.0.join("mixed-target.xsd"), format!(r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Results"><xs:complexType><xs:sequence><xs:element name="Row" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence>{count_schema}<xs:element name="Text" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#)).unwrap();
        if counted {
            let _guard = probe(Probe::GuardedMix);
            for result in [
                package.import().map(|_| ()),
                crate::import_with_profile(
                    &package.0.join("map.mfd"),
                    &crate::ImportOptions::default(),
                    crate::ImportProfile::Executable,
                )
                .map(|_| ()),
            ] {
                match result {
                    Err(crate::MfdError::UnsupportedImport(reason)) => {
                        assert!(reason.contains("cannot compose"), "{reason}")
                    }
                    other => panic!("expected Count profile exclusion: {other:?}"),
                }
            }
        } else {
            let imported = package.import().unwrap();
            assert_eq!(
                imported.project.execution_contract,
                Some(mapping::ExecutionContract::ClosedPrimarySourceV1)
            );
            let input = format_xml::from_str(
                "<Source><Row><Text>keep</Text></Row></Source>",
                &imported.project.source,
            )
            .unwrap();
            let output = engine::run(&imported.project, &input).unwrap();
            assert_eq!(
                output.field("Row").unwrap().as_repeated().unwrap()[0]
                    .field("Text")
                    .unwrap()
                    .as_scalar(),
                Some(&Value::String("keep".into()))
            );
        }
    }
}

#[test]
fn raw_opposite_claims_alternate_edges_indirect_uses_and_malformed_counts_refuse() {
    for (index, design) in [
        DESIGN.replace("</children>", r#"<component name="hidden"><sources><datapoint key="11"/></sources></component></children>"#),
        DESIGN.replace("</connections>", r#"<edge from="20" to="50"/></connections>"#),
        DESIGN.replace("</connections>", r#"<edge from="51" to="100"/></connections>"#),
        DESIGN.replace("</children>", r#"<component name="hidden"><sources><datapoint key="250"/></sources></component></children>"#).replace("</connections>", r#"<edge from="51" to="250"/></connections>"#),
        DESIGN.replace(r#"<component name="count" library="core" kind="5">"#, r#"<component name="count" library="unknown" kind="5">"#),
        DESIGN.replace(r#"<datapoint/><datapoint pos="1" key="50"/>"#, r#"<datapoint/><datapoint pos="2" key="50"/>"#),
        DESIGN.replace("</children>", r#"<component name="opaque" library="unknown" kind="99"><sources><datapoint key="80"/></sources><targets><datapoint key="81"/></targets></component></children>"#)
            .replace(r#"<edge from="42" to="50"/>"#, r#"<edge from="42" to="80"/><edge from="81" to="50"/>"#),
        DESIGN.replace("</children>", r#"<component name="opaque" library="unknown" kind="99"><data><root><entry name="Value" inpkey="80" outkey="81"/></root></data></component></children>"#)
            .replace(r#"<edge from="42" to="50"/>"#, r#"<edge from="42" to="80"/><edge from="81" to="50"/>"#),
    ].into_iter().enumerate() { eprintln!("raw authority control {index}"); refused(Package::new(&design).import()); }
}
