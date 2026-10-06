use std::error::Error as _;

use codegen::{
    ArtifactSet, Binding, CsvOutputError, CsvOutputPolicy, Expression, ExpressionNode,
    IterationPlan, NamedSourceProgram, NamedTargetProgram, Program, ProgramValidationError,
    TargetScope,
};
use ir::{ScalarType, SchemaNode, Value};

use crate::{EmitError, emit, emit_with_csv_output};

fn program() -> Program {
    let columns = [
        ("Text", ScalarType::String, Value::String("fixed".into())),
        ("Integer", ScalarType::Int, Value::Int(-7)),
        ("Number", ScalarType::Float, Value::Float(1.25)),
        ("Boolean", ScalarType::Bool, Value::Bool(true)),
    ];
    Program {
        xml_boundary: None,
        source: SchemaNode::group("Source", Vec::new()).repeating(),
        extra_sources: Vec::new(),
        target: SchemaNode::group(
            "Row",
            columns
                .iter()
                .map(|(name, ty, _)| SchemaNode::scalar(*name, *ty))
                .collect(),
        ),
        expressions: columns
            .iter()
            .enumerate()
            .map(|(id, (_, _, value))| ExpressionNode {
                id: id as u32,
                expression: Expression::Const {
                    value: value.clone(),
                },
            })
            .collect(),
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: Some(IterationPlan::source(Vec::new())),
            construction: Default::default(),
            // Serialization order belongs to the schema, not binding order.
            bindings: columns
                .iter()
                .enumerate()
                .rev()
                .map(|(id, (name, ty, _))| Binding {
                    target_field: (*name).into(),
                    expression: id as u32,
                    target_domain: (*ty).into(),
                    repeating: false,
                })
                .collect(),
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

fn contents<'a>(artifacts: &'a ArtifactSet, path: &str) -> &'a [u8] {
    &artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == path)
        .unwrap_or_else(|| panic!("missing artifact {path}"))
        .contents
}

fn text<'a>(artifacts: &'a ArtifactSet, path: &str) -> &'a str {
    std::str::from_utf8(contents(artifacts, path)).expect("generated source is UTF-8")
}

#[test]
fn csv_opt_in_is_deterministic_and_preserves_every_legacy_artifact() {
    let candidate = program();
    let legacy_before = emit(&candidate).expect("ordinary mapping emits");
    let opted = emit_with_csv_output(&candidate, &CsvOutputPolicy::default())
        .expect("flat repeated primary emits CSV adapters");
    assert_eq!(
        opted,
        emit_with_csv_output(&candidate, &CsvOutputPolicy::default()).unwrap()
    );
    assert_eq!(legacy_before, emit(&candidate).unwrap());
    assert_eq!(opted.files().len(), legacy_before.files().len() + 2);
    for original in legacy_before.files() {
        let old = std::str::from_utf8(&original.contents).unwrap();
        let expected = match original.path.as_str() {
            "GeneratedMapping.cs" => {
                assert_eq!(
                    old.matches("public static class GeneratedMapping\n{")
                        .count(),
                    1
                );
                old.replacen(
                    "public static class GeneratedMapping\n{",
                    "public static partial class GeneratedMapping\n{",
                    1,
                )
                .into_bytes()
            }
            "Ferrule.Generated.csproj" => {
                assert_eq!(
                    old.matches("    <Compile Include=\"GeneratedMapping.cs\" />\n")
                        .count(),
                    1
                );
                old.replacen(
                    "    <Compile Include=\"GeneratedMapping.cs\" />\n",
                    "    <Compile Include=\"GeneratedMapping.cs\" />\n    <Compile Include=\"GeneratedMapping.Csv.cs\" />\n",
                    1,
                ).into_bytes()
            }
            _ => original.contents.clone(),
        };
        assert_eq!(contents(&opted, original.path.as_str()), expected);
    }
    assert_eq!(
        contents(&opted, "Runtime/FerruleCsv.cs"),
        include_bytes!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsv.cs")
    );
    assert!(!legacy_before.files().iter().any(|file| {
        matches!(
            file.path.as_str(),
            "GeneratedMapping.Csv.cs" | "Runtime/FerruleCsv.cs"
        )
    }));
    assert!(!text(&legacy_before, "GeneratedMapping.cs").contains("ExecuteCsv"));
}

#[test]
fn csv_fields_use_schema_order_and_exact_native_scalar_domains() {
    let emitted = emit_with_csv_output(&program(), &CsvOutputPolicy::default()).unwrap();
    let source = text(&emitted, "GeneratedMapping.Csv.cs");
    let declarations = [
        "new(\"Text\", global::Ferrule.Runtime.FerruleScalarType.String)",
        "new(\"Integer\", global::Ferrule.Runtime.FerruleScalarType.Int64)",
        "new(\"Number\", global::Ferrule.Runtime.FerruleScalarType.Double)",
        "new(\"Boolean\", global::Ferrule.Runtime.FerruleScalarType.Bool)",
    ];
    let mut previous = None;
    for declaration in declarations {
        assert_eq!(source.matches(declaration).count(), 1);
        let position = source.find(declaration).unwrap();
        assert!(previous.is_none_or(|last| position > last));
        previous = Some(position);
    }
    for setting in [
        "Delimiter = null",
        "Quote = null",
        "QuoteDisabled = false",
        "HasHeaders = true",
        "Utf8Bom = false",
    ] {
        assert!(source.contains(setting), "missing default {setting}");
    }
}

#[test]
fn csv_literals_escape_field_names_and_capture_static_policy() {
    let mut candidate = program();
    let name = "É\"\n😀";
    if let ir::SchemaKind::Group { children, .. } = &mut candidate.target.kind {
        children[0].name = name.into();
    }
    candidate
        .root
        .bindings
        .iter_mut()
        .find(|binding| binding.target_field == "Text")
        .unwrap()
        .target_field = name.into();
    let policy = CsvOutputPolicy {
        delimiter: Some(';'),
        quote: Some('\''),
        quote_disabled: false,
        has_headers: false,
        utf8_bom: true,
    };
    let emitted = emit_with_csv_output(&candidate, &policy).unwrap();
    let source = text(&emitted, "GeneratedMapping.Csv.cs");
    assert!(source.is_ascii());
    assert!(source.contains(
        r#"new("\u00C9\"\n\uD83D\uDE00", global::Ferrule.Runtime.FerruleScalarType.String)"#
    ));
    for setting in [
        "Delimiter = (char)59",
        "Quote = (char)39",
        "QuoteDisabled = false",
        "HasHeaders = false",
        "Utf8Bom = true",
    ] {
        assert!(source.contains(setting));
    }
    let disabled = emit_with_csv_output(
        &candidate,
        &CsvOutputPolicy {
            delimiter: Some('\t'),
            quote_disabled: true,
            ..Default::default()
        },
    )
    .unwrap();
    let disabled = text(&disabled, "GeneratedMapping.Csv.cs");
    assert!(disabled.contains("Delimiter = (char)9"));
    assert!(disabled.contains("Quote = null"));
    assert!(disabled.contains("QuoteDisabled = true"));
}

#[test]
fn all_four_csv_adapters_execute_once_and_forward_context() {
    let emitted = emit_with_csv_output(&program(), &CsvOutputPolicy::default()).unwrap();
    let source = text(&emitted, "GeneratedMapping.Csv.cs");
    assert_eq!(
        source.matches("public static string ExecuteCsv(").count(),
        2
    );
    assert_eq!(
        source
            .matches("public static byte[] ExecuteCsvBytes(")
            .count(),
        2
    );
    assert_eq!(source.matches("var primary = Execute(source);").count(), 2);
    assert_eq!(
        source
            .matches("var primary = Execute(source, executionContext);")
            .count(),
        2
    );
    assert_eq!(source.matches("FerruleCsv.Serialize(primary,").count(), 2);
    assert_eq!(
        source.matches("FerruleCsv.SerializeBytes(primary,").count(),
        2
    );
    assert!(!source.contains("ExecuteOutputs("));
    assert!(!source.contains("catch"));
    assert!(!source.contains("ExecuteJson"));
}

#[test]
fn csv_emission_keeps_typed_dialect_and_named_boundary_refusals() {
    let candidate = program();
    for (policy, offending) in [
        (
            CsvOutputPolicy {
                delimiter: Some('\n'),
                ..Default::default()
            },
            '\n',
        ),
        (
            CsvOutputPolicy {
                delimiter: Some('é'),
                ..Default::default()
            },
            'é',
        ),
    ] {
        assert!(matches!(emit_with_csv_output(&candidate, &policy),
            Err(EmitError::CsvOutput(CsvOutputError::BadDelimiter(c))) if c == offending));
    }
    assert!(matches!(
        emit_with_csv_output(
            &candidate,
            &CsvOutputPolicy {
                quote: Some('é'),
                ..Default::default()
            }
        ),
        Err(EmitError::CsvOutput(CsvOutputError::BadQuote('é')))
    ));
    assert!(matches!(
        emit_with_csv_output(
            &candidate,
            &CsvOutputPolicy {
                delimiter: Some('|'),
                quote: Some('|'),
                ..Default::default()
            }
        ),
        Err(EmitError::CsvOutput(CsvOutputError::DelimiterQuoteConflict))
    ));
    assert!(matches!(
        emit_with_csv_output(
            &candidate,
            &CsvOutputPolicy {
                quote_disabled: true,
                quote: Some('"'),
                ..Default::default()
            }
        ),
        Err(EmitError::CsvOutput(
            CsvOutputError::ConflictingQuoteSettings
        ))
    ));

    let mut named = candidate.clone();
    named.extra_sources.push(NamedSourceProgram {
        name: "Secondary".into(),
        source: SchemaNode::group("Secondary", Vec::new()),
        dynamic: None,
    });
    assert!(matches!(
        emit_with_csv_output(&named, &CsvOutputPolicy::default()),
        Err(EmitError::CsvOutput(CsvOutputError::NamedInputs))
    ));
    named.extra_sources.clear();
    named.extra_targets.push(NamedTargetProgram {
        name: "Secondary".into(),
        target: named.target.clone(),
        root: named.root.clone(),
    });
    assert!(matches!(
        emit_with_csv_output(&named, &CsvOutputPolicy::default()),
        Err(EmitError::CsvOutput(CsvOutputError::NamedOutputs))
    ));

    let mut single = candidate;
    single.root.iteration = None;
    assert!(matches!(
        emit_with_csv_output(&single, &CsvOutputPolicy::default()),
        Err(EmitError::CsvOutput(CsvOutputError::PrimaryRows))
    ));
}

#[test]
fn invalid_mapping_retains_original_cause_before_csv_policy() {
    let mut candidate = program();
    candidate.root.bindings[0].expression = 999;
    let error = emit_with_csv_output(
        &candidate,
        &CsvOutputPolicy {
            delimiter: Some('\n'),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(
        matches!(&error, EmitError::CsvOutput(CsvOutputError::InvalidProgram(
        ProgramValidationError::MissingBindingExpression {
            target_path, target_field, expression: 999
        }
    )) if target_path.is_empty() && target_field == "Boolean")
    );
    let admission = error.source().unwrap();
    assert!(admission.downcast_ref::<CsvOutputError>().is_some());
    assert!(
        admission
            .source()
            .unwrap()
            .downcast_ref::<ProgramValidationError>()
            .is_some()
    );
    assert_eq!(error.clone(), error);
}
