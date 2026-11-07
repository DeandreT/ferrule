use super::*;
use crate::import::udf::Registry;
use crate::import::udf::certificate::OriginCapture;

const ORDINARY: &str = r#"<mapping><component><structure><children>
<component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Row" outkey="10"><entry name="Text" outkey="11"/></entry></entry></root></data></component>
<component name="filter" library="core" kind="3"><sources><datapoint key="40"/><datapoint key="41"/></sources><targets><datapoint key="42"/></targets></component>
<component name="count" library="core" kind="5"><sources><datapoint/><datapoint pos="1" key="50"/></sources><targets><datapoint key="51"/></targets></component>
<component name="target" library="xml" kind="14"><data><root><entry name="Result"><entry name="Total" inpkey="100"/></entry></root></data></component>
</children><connections><edge from="11" to="40"/><edge from="42" to="80"/><edge from="10" to="50"/><edge from="51" to="100"/></connections></structure></component></mapping>"#;

fn normalized_test(functions: &RegisteredVec<FnComponent>, apply: impl FnOnce(Normalized<'_>)) {
    let registry = Registry::default();
    let schemas = RegisteredVec::new(&registry);
    let calls = RegisteredVec::new(&registry);
    let external_scalar = RegisteredVec::new(&registry);
    let external_xslt = RegisteredVec::new(&registry);
    let json_serializers = RegisteredVec::new(&registry);
    let xml_serializers = RegisteredVec::new(&registry);
    let json_parsers = RegisteredVec::new(&registry);
    let flextext_parsers = RegisteredVec::new(&registry);
    let joins = crate::import::join::Registry::default();
    let rules = Default::default();
    apply(Normalized {
        schemas: &schemas,
        sources: &[],
        source_names: &[],
        intermediates: &[],
        targets: &[],
        functions,
        calls: &calls,
        external_scalar: &external_scalar,
        external_xslt: &external_xslt,
        json_serializers: &json_serializers,
        xml_serializers: &xml_serializers,
        json_parsers: &json_parsers,
        flextext_parsers: &flextext_parsers,
        joins: &joins,
        rules: &rules,
    });
}

#[test]
fn input_derived_work_slots_and_bytes_are_exact_sticky_and_fail_before_allocation() {
    let dimensions = Dimensions {
        nodes: 3,
        walk_nodes: 0,
        pins: 2,
        edges: 1,
        attributes: 4,
        decoded: 20,
        registered: 0,
    };
    for lane in ["work", "slots", "bytes"] {
        let mut ledger = Ledger::new(dimensions);
        let (work, slots, bytes) = ledger.limits();
        match lane {
            "work" => ledger.work(usize::try_from(work).unwrap()).unwrap(),
            "slots" => ledger.reserve(slots, 0).unwrap(),
            "bytes" => ledger.reserve(0, bytes).unwrap(),
            _ => unreachable!(),
        }
        let previous = (ledger.work, ledger.slots, ledger.bytes);
        let mut buffer = Buffer::<u64>::new();
        assert!(buffer.push(1, &mut ledger).is_err());
        assert_eq!(buffer.0.len(), 0);
        assert_eq!(buffer.0.capacity(), 0);
        assert_eq!((ledger.slots, ledger.bytes), (previous.1, previous.2));
        assert!(ledger.check().is_err());
        assert!(
            ledger.extend_registered(1, 100).is_err(),
            "new events must not renew a failed ledger"
        );
        assert!(buffer.push(2, &mut ledger).is_err());
        assert_eq!(buffer.0.capacity(), 0);
    }
    let mut ledger = Ledger::new(dimensions);
    let old = ledger.limits();
    ledger.extend_registered(7, 12).unwrap();
    assert_eq!(ledger.dimensions.registered, 7);
    assert_eq!(ledger.dimensions.decoded, 32);
    assert!(ledger.limits().0 > old.0 && ledger.limits().1 > old.1 && ledger.limits().2 > old.2);
}

#[test]
fn completed_ordinary_certificate_survives_real_unrelated_positive_exhaustion() {
    let document = crate::design::parse(ORDINARY).unwrap();
    let structure = document
        .descendants()
        .find(|n| n.has_tag_name("structure"))
        .unwrap();
    let header = Headers::read(structure).unwrap();
    let (_, routing) = crate::import::graph::read_edge_inputs_with_probe(
        &structure,
        structure.parent().as_ref(),
        |claims| Routing::prepare(header, structure, claims),
    );
    let routing = routing.unwrap().unwrap();
    let budget = CountConstruction::for_test(0, 0).with_routing(routing.clone());
    let mut functions = RegisteredVec::with_origin_capture(
        &Registry::default(),
        OriginCapture::Count(budget.clone()),
    );
    let count = document
        .descendants()
        .find(|n| n.has_tag_name("component") && n.attribute("name") == Some("count"))
        .unwrap();
    functions.set_active(count);
    functions.push(function::read(&count));
    normalized_test(&functions, |normalized| {
        let complete = finish(Some(&routing), &normalized).unwrap();
        assert!(complete.seeds.is_empty());
        assert!(budget.claim(1, 0, 0).is_err());
        let mut table = CountProofTable::EMPTY;
        complete.install(&mut table).unwrap();
        assert!(
            table.candidates.is_empty() && table.failure.is_none() && !table.discovery_incomplete
        );
    });
}

#[test]
fn late_promoted_or_synthetic_dependencies_and_counts_are_union_facts_not_legacy() {
    for late_count in [false, true] {
        for promoted in [false, true] {
            let document = crate::design::parse(ORDINARY).unwrap();
            let structure = document
                .descendants()
                .find(|n| n.has_tag_name("structure"))
                .unwrap();
            let header = Headers::read(structure).unwrap();
            let (_, routing) = crate::import::graph::read_edge_inputs_with_probe(
                &structure,
                structure.parent().as_ref(),
                |claims| Routing::prepare(header, structure, claims),
            );
            let routing = routing.unwrap().unwrap();
            let budget = CountConstruction::for_test(0, 0).with_routing(routing.clone());
            let mut functions = RegisteredVec::with_origin_capture(
                &Registry::default(),
                OriginCapture::Count(budget),
            );
            let count = document
                .descendants()
                .find(|n| n.has_tag_name("component") && n.attribute("name") == Some("count"))
                .unwrap();
            functions.set_active(count);
            functions.push(function::read(&count));
            let text = if late_count {
                r#"<component name="count" library="core" kind="5"><sources><datapoint key="80"/></sources><targets><datapoint key="51"/></targets></component>"#
            } else {
                r#"<component name="opaque" library="core" kind="99"><sources><datapoint key="80"/></sources><targets><datapoint key="10"/></targets></component>"#
            };
            let late = roxmltree::Document::parse(text).unwrap();
            let origin = if promoted {
                Origin::PromotedLocalCatalog(RawOwner::of(late.root_element()))
            } else {
                Origin::SyntheticOrUnproved
            };
            functions.push_origin(function::read(&late.root_element()), origin);
            normalized_test(&functions, |normalized| {
                let complete = finish(Some(&routing), &normalized).unwrap();
                let mut table = CountProofTable::EMPTY;
                complete.install(&mut table).unwrap();
                assert!(table.candidate_outputs.contains_key(&51));
                assert!(table.owns_index(usize::from(late_count)));
                assert!(
                    table
                        .candidates
                        .iter()
                        .all(|candidate| candidate.plan.is_err())
                );
            });
        }
    }
}

#[test]
fn incomplete_negative_authority_cannot_construct_a_complete_token_or_publish_legacy() {
    let document = crate::design::parse(ORDINARY).unwrap();
    let structure = document
        .descendants()
        .find(|n| n.has_tag_name("structure"))
        .unwrap();
    let header = Headers::read(structure).unwrap();
    let (_, routing) = crate::import::graph::read_edge_inputs_with_probe(
        &structure,
        structure.parent().as_ref(),
        |claims| Routing::prepare(header, structure, claims),
    );
    let routing = routing.unwrap().unwrap();
    let byte_limit = routing.borrow().ledger.limits().2;
    assert!(
        routing
            .borrow_mut()
            .ledger
            .reserve(0, byte_limit + 1)
            .is_err()
    );
    let functions = RegisteredVec::new(&Registry::default());
    normalized_test(&functions, |normalized| {
        assert!(finish(Some(&routing), &normalized).is_err())
    });
}
