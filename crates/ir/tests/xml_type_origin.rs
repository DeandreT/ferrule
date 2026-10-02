use ir::{
    DocumentMember, Instance, InstanceGroup, Value, XML_TYPE_FIELD, XML_TYPE_ORIGIN_FIELD,
    XmlTypeOrigin, XmlTypeOriginError,
};

fn data() -> Vec<(String, Instance)> {
    vec![("Code".into(), Instance::Scalar(Value::String("a".into())))]
}
fn marked(origin: XmlTypeOrigin<'_>) -> Instance {
    Instance::Group(
        InstanceGroup::from(data())
            .with_xml_type_origin(origin)
            .unwrap(),
    )
}

#[test]
fn state_is_owned_by_the_exact_occurrence_and_never_by_a_data_name() {
    for name in [XML_TYPE_FIELD, XML_TYPE_ORIGIN_FIELD] {
        let value = Instance::Group(
            vec![(
                name.into(),
                Instance::Scalar(Value::String("Derived".into())),
            )]
            .into(),
        );
        assert_eq!(value.xml_type_origin(), Ok(XmlTypeOrigin::Unknown));
        assert!(value.field(name).is_some());
    }
    let absent = marked(XmlTypeOrigin::Absent);
    let explicit = marked(XmlTypeOrigin::Explicit("{urn:types}Derived"));
    assert_eq!(absent.xml_type_origin(), Ok(XmlTypeOrigin::Absent));
    assert_eq!(
        explicit.xml_type_origin(),
        Ok(XmlTypeOrigin::Explicit("{urn:types}Derived"))
    );
    assert_ne!(absent, explicit);
    let nested = Instance::Group(vec![("Inner".into(), marked(XmlTypeOrigin::Unknown))].into());
    assert_eq!(nested.xml_type_origin(), Ok(XmlTypeOrigin::Unknown));
    assert_eq!(
        nested.field("Inner").unwrap().xml_type_origin(),
        Ok(XmlTypeOrigin::Unknown)
    );
    for value in [
        Instance::Scalar(Value::Null),
        Instance::Repeated(vec![explicit.clone()]),
        Instance::MappedSequence(vec![explicit.clone()]),
        Instance::DocumentSet(vec![DocumentMember::new("one.xml", explicit).unwrap()]),
    ] {
        assert_eq!(
            value.xml_type_origin(),
            Err(XmlTypeOriginError::ExpectedGroup)
        );
    }
}

#[test]
fn exact_clone_retains_metadata_legacy_wire_keeps_data_and_resets_metadata() {
    let explicit = marked(XmlTypeOrigin::Explicit("Derived"));
    assert_eq!(explicit.clone(), explicit);
    let wire = serde_json::to_value(&explicit).unwrap();
    assert_eq!(wire, serde_json::json!({"Group":[["Code",{"Scalar":"a"}]]}));
    let decoded: Instance = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(decoded.xml_type_origin(), Ok(XmlTypeOrigin::Unknown));
    assert_ne!(explicit, decoded);
    assert_eq!(serde_json::to_value(&decoded).unwrap(), wire);
    assert!(!format!("{explicit:?}").contains("Derived"));
    let bogus: Instance = serde_json::from_value(
        serde_json::json!({"Group":[[XML_TYPE_ORIGIN_FIELD,{"Scalar":"ordinary data"}]]}),
    )
    .unwrap();
    assert_eq!(bogus.xml_type_origin(), Ok(XmlTypeOrigin::Unknown));
    assert_eq!(
        bogus
            .field(XML_TYPE_ORIGIN_FIELD)
            .and_then(Instance::as_scalar),
        Some(&Value::String("ordinary data".into()))
    );
}

#[test]
fn every_mutable_access_and_general_rebuild_invalidates_the_owner() {
    for mode in 0..6 {
        let Instance::Group(mut group) = marked(XmlTypeOrigin::Explicit("Derived")) else {
            unreachable!()
        };
        match mode {
            0 => {
                let _ = group.iter_mut();
            }
            1 => {
                group[0].1 = Instance::Scalar(Value::String("a".into()));
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
        assert_eq!(group.xml_type_origin(), XmlTypeOrigin::Unknown);
    }
    let Instance::Group(source) = marked(XmlTypeOrigin::Absent) else {
        unreachable!()
    };
    assert_eq!(source.clone().xml_type_origin(), XmlTypeOrigin::Absent);
    let rebuilt = InstanceGroup::from(source.into_fields());
    assert_eq!(rebuilt.xml_type_origin(), XmlTypeOrigin::Unknown);
    let mut parent = InstanceGroup::from(vec![(
        "Child".into(),
        marked(XmlTypeOrigin::Explicit("Derived")),
    )])
    .with_xml_type_origin(XmlTypeOrigin::Absent)
    .unwrap();
    let _ = &mut parent[0].1;
    assert_eq!(parent.xml_type_origin(), XmlTypeOrigin::Unknown);
    assert_eq!(
        parent[0].1.xml_type_origin(),
        Ok(XmlTypeOrigin::Explicit("Derived"))
    );
}

#[test]
fn trusted_ingress_rejects_empty_identity_without_changing_an_existing_fact() {
    let Instance::Group(mut source) = marked(XmlTypeOrigin::Absent) else {
        unreachable!()
    };
    assert_eq!(
        source.set_xml_type_origin(XmlTypeOrigin::Explicit("")),
        Err(XmlTypeOriginError::InvalidPayload)
    );
    assert_eq!(source.xml_type_origin(), XmlTypeOrigin::Absent);
}

#[test]
fn optional_boxed_known_state_does_not_grow_the_instance_union() {
    #[allow(dead_code)]
    enum LegacyInstance {
        Scalar(Value),
        Group(Vec<(String, Instance)>),
        Repeated(Vec<Instance>),
        DocumentSet(Vec<DocumentMember>),
        MappedSequence(Vec<Instance>),
    }
    let baseline = std::mem::size_of::<LegacyInstance>();
    let actual = std::mem::size_of::<Instance>();
    eprintln!(
        "layout legacy-instance={baseline} typed-instance={actual} group={} legacy-group={}",
        std::mem::size_of::<InstanceGroup>(),
        std::mem::size_of::<Vec<(String, Instance)>>()
    );
    assert_eq!(actual, baseline);
}
