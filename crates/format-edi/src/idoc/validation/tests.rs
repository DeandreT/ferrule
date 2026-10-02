//! Self-authored descriptor validation contracts.

use std::num::NonZeroU32;

use mapping::{
    IdocNativeCode, IdocNativeField, IdocNativeFieldType, IdocNativeGroup, IdocNativeSegment,
};

use super::*;

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

fn segment(
    name: &str,
    status: IdocNativeStatus,
    minimum: u64,
    maximum: u64,
    codes: &[&str],
) -> IdocNativeNode {
    IdocNativeNode::Segment(
        IdocNativeSegment::new(
            name,
            name,
            false,
            nz(2),
            status,
            minimum,
            maximum,
            vec![
                IdocNativeField::new(
                    "Code",
                    "Self-authored code",
                    IdocNativeFieldType::Character,
                    nz(2),
                    nz(1),
                    nz(10),
                    nz(11),
                    codes
                        .iter()
                        .map(|code| IdocNativeCode::new(*code, "").unwrap())
                        .collect(),
                )
                .unwrap(),
            ],
        )
        .unwrap(),
    )
}

fn descriptor() -> IdocNativeConfig {
    IdocNativeConfig::new(
        "SELFTEST",
        vec![IdocNativeNode::Group(
            IdocNativeGroup::new(
                nz(1),
                nz(1),
                IdocNativeStatus::Mandatory,
                1,
                2,
                vec![
                    segment("HEAD0001", IdocNativeStatus::Mandatory, 1, 1, &[]),
                    segment(
                        "ROW00001",
                        IdocNativeStatus::Optional,
                        2,
                        3,
                        &["AA", "BB", "1", ""],
                    ),
                ],
            )
            .unwrap(),
        )],
    )
    .unwrap()
}

fn record(value: Value) -> Instance {
    Instance::Group((vec![("Code".into(), Instance::Scalar(value))]).into())
}

fn group(codes: &[&str]) -> Instance {
    Instance::Group(
        (vec![
            ("HEAD0001".into(), record(Value::String("H".into()))),
            (
                "ROW00001".into(),
                Instance::Repeated(
                    codes
                        .iter()
                        .map(|code| record(Value::String((*code).into())))
                        .collect(),
                ),
            ),
        ])
        .into(),
    )
}

fn document(groups: Vec<Instance>) -> Instance {
    Instance::Group((vec![("SG1".into(), Instance::Repeated(groups))]).into())
}

fn report(descriptor: &IdocNativeConfig, instance: &Instance) -> IdocValidationReport {
    let (schema, layout) = descriptor.project().unwrap();
    validate_native(&schema, instance, &layout, descriptor).unwrap()
}

#[test]
fn occurrence_counts_are_independent_for_each_parent() {
    let descriptor = descriptor();
    assert!(
        report(
            &descriptor,
            &document(vec![group(&["AA", "BB"]), group(&[])])
        )
        .is_empty()
    );

    // Four total rows would satisfy a flattened minimum, but the first parent
    // has only one. The second parent's three rows cannot satisfy its sibling.
    let invalid = document(vec![group(&["AA"]), group(&["AA", "BB", "AA"])]);
    let report = report(&descriptor, &invalid);
    assert_eq!(report.issues().len(), 1);
    assert_eq!(report.issues()[0].location(), "SG1[1]/ROW00001");
    assert_eq!(
        report.issues()[0].violation(),
        IdocConstraintViolation::Occurrences {
            status: IdocNativeStatus::Optional,
            minimum: 2,
            maximum: 3,
            effective_minimum: 2,
            actual: 1,
        }
    );

    let report = super::validate_native(
        &descriptor.project().unwrap().0,
        &document(vec![group(&["AA", "AA", "AA", "AA"]), group(&[])]),
        &descriptor.project().unwrap().1,
        &descriptor,
    )
    .unwrap();
    assert_eq!(report.issues().len(), 1);
    assert!(matches!(
        report.issues()[0].violation(),
        IdocConstraintViolation::Occurrences {
            actual: 4,
            maximum: 3,
            ..
        }
    ));
}

#[test]
fn mandatory_status_requires_presence_even_with_zero_configured_minimum() {
    let descriptor = IdocNativeConfig::new(
        "SELFTEST",
        vec![segment("HEAD0001", IdocNativeStatus::Mandatory, 0, 1, &[])],
    )
    .unwrap();
    let report = report(&descriptor, &Instance::Group((vec![]).into()));
    assert_eq!(report.issues()[0].location(), "HEAD0001");
    assert_eq!(
        report.issues()[0].violation(),
        IdocConstraintViolation::Occurrences {
            status: IdocNativeStatus::Mandatory,
            minimum: 0,
            maximum: 1,
            effective_minimum: 1,
            actual: 0,
        }
    );
}

#[test]
fn reports_parent_and_child_limits_and_ordered_code_issues() {
    let descriptor = descriptor();
    let report = report(
        &descriptor,
        &document(vec![group(&["AA", "XX"]), group(&[]), group(&["ZZ"])]),
    );
    assert_eq!(
        report
            .issues()
            .iter()
            .map(|issue| issue.location())
            .collect::<Vec<_>>(),
        vec![
            "SG1",
            "SG1[1]/ROW00001[2]/Code",
            "SG1[3]/ROW00001",
            "SG1[3]/ROW00001[1]/Code",
        ]
    );
    assert_eq!(
        report.issues()[1].violation(),
        IdocConstraintViolation::NotAllowed { allowed_count: 4 }
    );
    assert!(!report.truncated());
}

#[test]
fn code_validation_skips_absent_null_and_checks_explicit_empty_and_writer_coercion() {
    let descriptor = IdocNativeConfig::new(
        "SELFTEST",
        vec![segment(
            "ROW00001",
            IdocNativeStatus::Optional,
            0,
            10,
            &["", "1"],
        )],
    )
    .unwrap();
    let instance = Instance::Group(
        (vec![(
            "ROW00001".into(),
            Instance::Repeated(vec![
                Instance::Group((vec![]).into()),
                record(Value::Null),
                record(Value::String("".into())),
                record(Value::Int(1)),
            ]),
        )])
        .into(),
    );
    assert!(report(&descriptor, &instance).is_empty());

    let no_empty_code = IdocNativeConfig::new(
        "SELFTEST",
        vec![segment(
            "ROW00001",
            IdocNativeStatus::Optional,
            0,
            10,
            &["AA"],
        )],
    )
    .unwrap();
    let report = report(&no_empty_code, &instance);
    assert_eq!(
        report
            .issues()
            .iter()
            .map(|issue| issue.location())
            .collect::<Vec<_>>(),
        vec!["ROW00001[3]/Code", "ROW00001[4]/Code",]
    );
}

#[test]
fn guarded_parsing_preserves_lenient_unknown_handling_and_always_checks_declared_codes() {
    let descriptor = descriptor();
    let (schema, layout) = descriptor.project().unwrap();
    let bytes = b"UNKNOWN skipped\r\nHEAD0001 H \r\nROW00001 AA\r\nROW00001 BB\r\n";
    assert!(matches!(
        crate::idoc::from_bytes_with_native(bytes, &schema, &layout, &descriptor, false),
        Err(EdiFormatError::UnrecognizedIdocSegment { .. })
    ));
    assert_eq!(
        crate::idoc::from_bytes_with_native(bytes, &schema, &layout, &descriptor, true).unwrap(),
        document(vec![group(&["AA", "BB"])])
    );

    let bad_code = b"HEAD0001 H \r\nROW00001 AA\r\nROW00001 XX\r\n";
    for lenient in [false, true] {
        assert!(crate::idoc::from_bytes(bad_code, &schema, &layout, lenient).is_ok());
        let Err(EdiFormatError::IdocValidation(report)) =
            crate::idoc::from_bytes_with_native(bad_code, &schema, &layout, &descriptor, lenient)
        else {
            panic!("expected explicit code validation");
        };
        assert_eq!(report.issues()[0].location(), "SG1[1]/ROW00001[2]/Code");
    }
    let one_row = b"HEAD0001 H \r\nROW00001 AA\r\n";
    assert!(matches!(
        crate::idoc::from_bytes_with_native(one_row, &schema, &layout, &descriptor, true),
        Err(EdiFormatError::IdocValidation(_))
    ));
}

#[test]
fn guarded_write_rejects_before_touching_destination_and_legacy_write_is_unchanged() {
    let descriptor = descriptor();
    let (schema, layout) = descriptor.project().unwrap();
    let path = std::env::temp_dir().join(format!(
        "ferrule_idoc_guarded_write_{}.idoc",
        std::process::id()
    ));
    std::fs::write(&path, b"existing bytes").unwrap();
    let invalid = document(vec![group(&["XX"])]);
    assert!(crate::idoc::to_bytes(&schema, &invalid, &layout).is_ok());
    assert!(matches!(
        crate::idoc::write_with_native(&path, &schema, &invalid, &layout, &descriptor),
        Err(EdiFormatError::IdocValidation(_))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), b"existing bytes");

    let valid = document(vec![group(&["AA", "BB"])]);
    crate::idoc::write_with_native(&path, &schema, &valid, &layout, &descriptor).unwrap();
    assert_eq!(
        crate::idoc::read_with_native(&path, &schema, &layout, &descriptor, false).unwrap(),
        valid
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn descriptor_pairing_rejects_changed_schema_and_layout_before_reading() {
    let descriptor = descriptor();
    let (mut schema, layout) = descriptor.project().unwrap();
    schema.name = "Changed".into();
    assert!(matches!(
        validate_native(&schema, &document(vec![]), &layout, &descriptor),
        Err(EdiFormatError::IdocDescriptorMismatch)
    ));
    assert!(matches!(
        crate::idoc::read_with_native(
            std::path::Path::new("does-not-exist"),
            &schema,
            &layout,
            &descriptor,
            true
        ),
        Err(EdiFormatError::IdocDescriptorMismatch)
    ));

    let (schema, _) = descriptor.project().unwrap();
    let layout = IdocLayout::new(vec![
        mapping::IdocSegmentLayout::new(
            "OTHER000",
            vec![mapping::IdocFieldLayout::new("Code", nz(10), nz(11)).unwrap()],
        )
        .unwrap(),
    ])
    .unwrap();
    assert!(matches!(
        validate_native(&schema, &document(vec![]), &layout, &descriptor),
        Err(EdiFormatError::IdocDescriptorMismatch)
    ));
}

#[test]
fn shape_failures_and_duplicate_fields_remain_typed() {
    let descriptor = descriptor();
    let (schema, layout) = descriptor.project().unwrap();
    let malformed = Instance::Group((vec![("SG1".into(), group(&[]))]).into());
    assert!(matches!(
        validate_native(&schema, &malformed, &layout, &descriptor),
        Err(EdiFormatError::InstanceShape {
            expected: "repeating values",
            ..
        })
    ));
    let duplicate = Instance::Group(
        (vec![
            ("SG1".into(), Instance::Repeated(vec![])),
            ("SG1".into(), Instance::Repeated(vec![])),
        ])
        .into(),
    );
    assert!(matches!(
        validate_native(&schema, &duplicate, &layout, &descriptor),
        Err(EdiFormatError::DuplicateField { .. })
    ));
}

#[test]
fn issue_collection_is_bounded_and_truncation_is_visible() {
    let descriptor = IdocNativeConfig::new(
        "SELFTEST",
        vec![segment(
            "ROW00001",
            IdocNativeStatus::Optional,
            0,
            u64::MAX,
            &["AA"],
        )],
    )
    .unwrap();
    let instance = Instance::Group(
        (vec![(
            "ROW00001".into(),
            Instance::Repeated(vec![record(Value::String("XX".into())); 10_005]),
        )])
        .into(),
    );
    let report = report(&descriptor, &instance);
    assert_eq!(report.issues().len(), 10_000);
    assert!(report.truncated());
    assert_eq!(
        report.issues().last().unwrap().location(),
        "ROW00001[10000]/Code"
    );
}

#[test]
fn host_owned_instance_work_and_text_are_bounded() {
    let descriptor = IdocNativeConfig::new(
        "SELFTEST",
        vec![segment(
            "ROW00001",
            IdocNativeStatus::Optional,
            0,
            u64::MAX,
            &[],
        )],
    )
    .unwrap();
    let (schema, layout) = descriptor.project().unwrap();
    let many = Instance::Group(
        (vec![(
            "ROW00001".into(),
            Instance::Repeated(vec![Instance::Group((vec![]).into()); 333_334]),
        )])
        .into(),
    );
    assert!(matches!(
        validate_native(&schema, &many, &layout, &descriptor),
        Err(EdiFormatError::IdocLimit("validation work"))
    ));
    let huge = Instance::Group(
        (vec![(
            "ROW00001".into(),
            Instance::Repeated(vec![record(Value::String(
                "X".repeat(64 * 1024 * 1024 + 1),
            ))]),
        )])
        .into(),
    );
    assert!(matches!(
        validate_native(&schema, &huge, &layout, &descriptor),
        Err(EdiFormatError::IdocLimit("validation bytes"))
    ));
}

#[test]
fn guarded_parser_bounds_short_record_field_expansion_before_materialization() {
    let fields = (0..1024)
        .map(|index| {
            IdocNativeField::new(
                format!("Field{index}"),
                "",
                IdocNativeFieldType::Character,
                nz(1),
                nz(index + 1),
                nz(index + 10),
                nz(index + 10),
                vec![],
            )
            .unwrap()
        })
        .collect();
    let descriptor = IdocNativeConfig::new(
        "SELFTEST",
        vec![IdocNativeNode::Segment(
            IdocNativeSegment::new(
                "ROW00001",
                "ROW00001",
                false,
                nz(1),
                IdocNativeStatus::Optional,
                0,
                u64::MAX,
                fields,
            )
            .unwrap(),
        )],
    )
    .unwrap();
    let (schema, layout) = descriptor.project().unwrap();
    let short_records = "ROW00001\n".repeat(1000);
    assert!(matches!(
        crate::idoc::from_bytes_with_native(
            short_records.as_bytes(),
            &schema,
            &layout,
            &descriptor,
            true,
        ),
        Err(EdiFormatError::IdocLimit("validation work"))
    ));
}

#[test]
fn guarded_writer_bounds_wide_record_searches_before_serialization() {
    let fields: Vec<_> = (0..10_000)
        .map(|index| {
            IdocNativeField::new(
                format!("Field{index}"),
                "",
                IdocNativeFieldType::Character,
                nz(1),
                nz(index + 1),
                nz(index + 10),
                nz(index + 10),
                vec![],
            )
            .unwrap()
        })
        .collect();
    let values: Vec<_> = fields
        .iter()
        .map(|field| (field.name().into(), Instance::Scalar(Value::Null)))
        .collect();
    let descriptor = IdocNativeConfig::new(
        "SELFTEST",
        vec![IdocNativeNode::Segment(
            IdocNativeSegment::new(
                "ROW00001",
                "ROW00001",
                false,
                nz(1),
                IdocNativeStatus::Mandatory,
                1,
                1,
                fields,
            )
            .unwrap(),
        )],
    )
    .unwrap();
    let (schema, layout) = descriptor.project().unwrap();
    let instance =
        Instance::Group((vec![("ROW00001".into(), Instance::Group((values).into()))]).into());
    assert!(
        validate_native(&schema, &instance, &layout, &descriptor)
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        crate::idoc::to_bytes_with_native(&schema, &instance, &layout, &descriptor),
        Err(EdiFormatError::IdocLimit("validation output work"))
    ));
}
