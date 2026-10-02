#[path = "../../mapping/tests/support/primary_root_project.rs"]
mod fixture;

use ir::{Instance, InstanceGroup, PrimaryRootError, Value, XmlTypeOrigin};

fn root(value: Option<Value>, origin: XmlTypeOrigin<'_>) -> Instance {
    Instance::Group(
        InstanceGroup::from(
            value
                .into_iter()
                .map(|value| ("Code".into(), Instance::Scalar(value)))
                .collect::<Vec<_>>(),
        )
        .with_xml_type_origin(origin)
        .unwrap(),
    )
}

#[test]
fn required_primary_root_reads_fail_only_in_the_reached_branch() {
    let project = fixture::required_primary_root_project(true);
    assert!(engine::validate(&project).is_empty());
    for missing in [None, Some(Value::Null)] {
        let source = root(missing.clone(), XmlTypeOrigin::Explicit("Derived"));
        assert!(
            matches!(engine::run(&project, &source), Err(engine::EngineError::PrimaryRoot {
            node: 1, source: PrimaryRootError::MissingRequiredField { path }
        }) if path == ["Code"])
        );
        for origin in [XmlTypeOrigin::Absent, XmlTypeOrigin::Explicit("Base")] {
            let source = root(missing.clone(), origin);
            let output = engine::run(&project, &source).unwrap();
            assert_eq!(
                output.field("Code").and_then(Instance::as_scalar),
                Some(&Value::Null)
            );
        }
    }
    for value in [
        Value::String("".into()),
        Value::String("present".into()),
        Value::xml_nil(),
    ] {
        let source = root(Some(value.clone()), XmlTypeOrigin::Explicit("Derived"));
        let output = engine::run(&project, &source).unwrap();
        assert_eq!(
            output.field("Code").and_then(Instance::as_scalar),
            Some(&value)
        );
    }
    let unknown = root(None, XmlTypeOrigin::Unknown);
    assert!(matches!(
        engine::run(&project, &unknown),
        Err(engine::EngineError::PrimaryRoot {
            node: 0,
            source: PrimaryRootError::UnknownXmlTypeOrigin
        })
    ));
}

#[test]
fn legacy_nullable_policy_stays_nullable_for_a_required_schema_attribute() {
    let project = fixture::required_primary_root_project(false);
    let source = root(None, XmlTypeOrigin::Explicit("Derived"));
    let output = engine::run(&project, &source).unwrap();
    assert_eq!(
        output.field("Code").and_then(Instance::as_scalar),
        Some(&Value::Null)
    );
}
