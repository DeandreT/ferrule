use ir::{
    Instance, InstanceGroup, MAX_SOURCE_ORIGIN_PATH_BYTES, SourceDocumentOrigin, SourceOriginError,
    Value, XmlTypeOrigin,
};

fn fields() -> Vec<(String, Instance)> {
    vec![("Value".into(), Instance::Scalar(Value::String("A".into())))]
}

#[test]
fn physical_identity_is_independent_of_paths_values_and_clones() {
    let first = SourceDocumentOrigin::new(" ./same.csv ", Some("C:/resolved/same.csv")).unwrap();
    let second = SourceDocumentOrigin::new(" ./same.csv ", Some("C:/resolved/same.csv")).unwrap();
    assert_eq!(first.path(), " ./same.csv ");
    assert_eq!(first.resolved_path(), Some("C:/resolved/same.csv"));
    assert_eq!(first.source_path(), "C:/resolved/same.csv");
    assert!(first.same_document(&first.clone()));
    assert!(!first.same_document(&second));
    let row = first.row(1).unwrap();
    assert_eq!(row.position(), 1);
    assert!(row.same_occurrence(&row.clone()));
    assert!(row.same_occurrence(&first.row(1).unwrap()));
    assert!(!row.same_occurrence(&first.row(2).unwrap()));
    assert!(!row.same_occurrence(&second.row(1).unwrap()));
    drop(first);
    assert_eq!(row.document().source_path(), "C:/resolved/same.csv");
    assert_eq!(
        SourceDocumentOrigin::new("opaque:anywhere", None)
            .unwrap()
            .source_path(),
        "opaque:anywhere"
    );
}

#[test]
fn provenance_stays_out_of_equality_debug_and_the_legacy_wire() {
    let document = SourceDocumentOrigin::new("private/source.csv", None).unwrap();
    let plain = InstanceGroup::from(fields());
    let marked = plain.clone().with_source_origin(document.row(1).unwrap());
    assert_eq!(marked, plain);
    assert_eq!(
        marked,
        plain.clone().with_source_origin(document.row(2).unwrap())
    );
    assert_eq!(format!("{marked:?}"), format!("{plain:?}"));
    let wire = serde_json::to_string(&marked).unwrap();
    assert_eq!(wire, r#"[["Value",{"Scalar":"A"}]]"#);
    assert_eq!(wire, serde_json::to_string(&plain).unwrap());
    let decoded: InstanceGroup = serde_json::from_str(&wire).unwrap();
    assert_eq!(decoded, marked);
    assert!(decoded.source_origin().is_none());
    assert!(plain.source_origin().is_none());
    assert!(InstanceGroup::default().source_origin().is_none());
    assert!(
        marked
            .clone()
            .source_origin()
            .unwrap()
            .same_occurrence(&document.row(1).unwrap())
    );
}

#[test]
fn mutation_and_reconstruction_invalidate_but_exact_cloning_retains() {
    let document = SourceDocumentOrigin::new("input.csv", None).unwrap();
    let marked = InstanceGroup::from(fields()).with_source_origin(document.row(1).unwrap());
    for mode in 0..6 {
        let mut group = marked.clone();
        match mode {
            0 => {
                let _ = group.iter_mut();
            }
            1 => {
                group[0].1 = Instance::Scalar(Value::String("A".into()));
            }
            2 => {
                let _ = group.get_mut(0);
            }
            3 => {
                group.retain(|_| true);
            }
            4 => {
                let _ = (&mut group).into_iter();
            }
            5 => {
                let _ = std::ops::DerefMut::deref_mut(&mut group);
            }
            _ => unreachable!(),
        }
        assert!(group.source_origin().is_none());
    }
    assert!(
        InstanceGroup::from(marked.clone().into_fields())
            .source_origin()
            .is_none()
    );
    assert!(
        InstanceGroup::from(marked.clone().into_iter().collect::<Vec<_>>())
            .source_origin()
            .is_none()
    );
    assert_eq!(marked.source_origin(), Some(&document.row(1).unwrap()));
}

#[test]
fn source_and_xml_facts_coexist_without_changing_the_xml_contract() {
    let document = SourceDocumentOrigin::new("input.csv", None).unwrap();
    let mut group = InstanceGroup::from(fields())
        .with_source_origin(document.row(1).unwrap())
        .with_xml_type_origin(XmlTypeOrigin::Absent)
        .unwrap();
    assert_eq!(group.xml_type_origin(), XmlTypeOrigin::Absent);
    assert_eq!(group.source_origin(), Some(&document.row(1).unwrap()));
    assert_ne!(group, InstanceGroup::from(fields()));
    group.set_xml_type_origin(XmlTypeOrigin::Unknown).unwrap();
    assert_eq!(group.xml_type_origin(), XmlTypeOrigin::Unknown);
    assert_eq!(group.source_origin(), Some(&document.row(1).unwrap()));
    assert_eq!(group, InstanceGroup::from(fields()));
    group.set_xml_type_origin(XmlTypeOrigin::Absent).unwrap();
    let _ = group.iter_mut();
    assert_eq!(group.xml_type_origin(), XmlTypeOrigin::Unknown);
    assert!(group.source_origin().is_none());
}

#[test]
fn paths_are_bounded_utf8_labels_and_positions_are_one_based() {
    for invalid in [
        String::new(),
        "a\0b".into(),
        "x".repeat(MAX_SOURCE_ORIGIN_PATH_BYTES + 1),
        "é".repeat(MAX_SOURCE_ORIGIN_PATH_BYTES / 2 + 1),
    ] {
        assert_eq!(
            SourceDocumentOrigin::new(&invalid, None).unwrap_err(),
            SourceOriginError::Path { resolved: false }
        );
        assert_eq!(
            SourceDocumentOrigin::new("valid", Some(&invalid)).unwrap_err(),
            SourceOriginError::Path { resolved: true }
        );
    }
    for path in [
        "x".repeat(MAX_SOURCE_ORIGIN_PATH_BYTES),
        "é".repeat(MAX_SOURCE_ORIGIN_PATH_BYTES / 2),
    ] {
        let document = SourceDocumentOrigin::new(&path, Some(&path)).unwrap();
        assert_eq!(document.source_path(), path);
        assert_eq!(document.row(0), Err(SourceOriginError::ZeroPosition));
        assert_eq!(document.row(usize::MAX).unwrap().position(), usize::MAX);
    }
}
