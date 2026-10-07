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
    assert!(emit_with_csv_output(&named, &CsvOutputPolicy::default()).is_ok());
    named.extra_sources[0].dynamic = Some(codegen::DynamicSourceProgram {
        path: 0,
        driver: codegen::SourceIteration::new(Vec::new()),
    });
    assert!(emit_with_csv_output(&named, &CsvOutputPolicy::default()).is_ok());
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

#[test]
fn named_csv_companions_reuse_ordinary_context_and_keep_no_name_bytes_exact() {
    let plain = program();
    let before = emit_with_csv_output(&plain, &CsvOutputPolicy::default()).unwrap();
    let old_adapter = text(&before, "GeneratedMapping.Csv.cs");
    assert!(!old_adapter.contains("ExecuteCsvWithSources("));
    let mut named = plain.clone();
    named.extra_sources.push(NamedSourceProgram {
        name: "Settings".into(),
        source: SchemaNode::group("Settings", Vec::new()),
        dynamic: None,
    });
    let opted = emit_with_csv_output(&named, &CsvOutputPolicy::default()).unwrap();
    let adapter = text(&opted, "GeneratedMapping.Csv.cs");
    let legacy_prefix = old_adapter.strip_suffix("}\n").unwrap();
    assert!(adapter.starts_with(legacy_prefix));
    assert!(adapter.ends_with("}\n"));
    let added = &adapter[legacy_prefix.len()..adapter.len() - 2];
    assert_eq!(added.matches("public static ").count(), 4);
    assert_eq!(
        adapter
            .matches("public static string ExecuteCsvWithSources(")
            .count(),
        2
    );
    assert_eq!(
        adapter
            .matches("public static byte[] ExecuteCsvBytesWithSources(")
            .count(),
        2
    );
    assert_eq!(
        adapter
            .matches("var primary = ExecuteWithSources(source, extraSources);")
            .count(),
        2
    );
    assert_eq!(
        adapter
            .matches("var primary = ExecuteWithSources(source, extraSources, executionContext);")
            .count(),
        2
    );
    assert!(!adapter.contains("ExecuteOutputs"));
    assert!(!adapter.contains("catch"));
    assert!(!adapter.contains("ExecuteJson"));
    let ordinary = emit(&named).unwrap();
    for old in ordinary.files() {
        let expected = match old.path.as_str() {
            "GeneratedMapping.cs" => std::str::from_utf8(&old.contents).unwrap().replacen(
                "public static class GeneratedMapping\n{", "public static partial class GeneratedMapping\n{", 1).into_bytes(),
            "Ferrule.Generated.csproj" => std::str::from_utf8(&old.contents).unwrap().replacen(
                "    <Compile Include=\"GeneratedMapping.cs\" />\n",
                "    <Compile Include=\"GeneratedMapping.cs\" />\n    <Compile Include=\"GeneratedMapping.Csv.cs\" />\n", 1).into_bytes(),
            _ => old.contents.clone(),
        };
        assert_eq!(contents(&opted, old.path.as_str()), expected);
    }
    assert_eq!(emit(&named).unwrap(), ordinary);
    assert_eq!(
        emit_with_csv_output(&plain, &CsvOutputPolicy::default()).unwrap(),
        before
    );
}

#[test]
fn dynamic_csv_companions_delegate_once_and_preserve_complete_existing_api_bodies() {
    let plain = program();
    let old = emit_with_csv_output(&plain, &CsvOutputPolicy::default()).unwrap();
    let old_adapter = text(&old, "GeneratedMapping.Csv.cs");
    let mut dynamic = plain.clone();
    dynamic.extra_sources.push(NamedSourceProgram {
        name: "Dynamic".into(),
        source: SchemaNode::group("Dynamic", Vec::new()),
        dynamic: Some(codegen::DynamicSourceProgram {
            path: 0,
            driver: codegen::SourceIteration::new(Vec::new()),
        }),
    });
    let ordinary = emit(&dynamic).unwrap();
    let opted = emit_with_csv_output(&dynamic, &CsvOutputPolicy::default()).unwrap();
    let adapter = text(&opted, "GeneratedMapping.Csv.cs");
    let prefix = old_adapter.strip_suffix("}\n").unwrap();
    assert!(adapter.starts_with(prefix));
    let added = &adapter[prefix.len()..adapter.len() - 2];
    assert_eq!(added.matches("public static ").count(), 6);
    assert!(!adapter.contains("public static string ExecuteCsvWithSources("));
    for suffix in [
        "WithDynamicSourceLoader",
        "WithSourcesAndDynamicSourceLoader",
        "WithSourcesContextAndDynamicSourceLoader",
    ] {
        assert_eq!(
            added
                .matches(&format!("public static string ExecuteCsv{suffix}("))
                .count(),
            1
        );
        assert_eq!(
            added
                .matches(&format!("public static byte[] ExecuteCsvBytes{suffix}("))
                .count(),
            1
        );
    }
    for call in [
        "ExecuteWithDynamicSourceLoader(source, loader)",
        "ExecuteWithSourcesAndDynamicSourceLoader(source, extraSources, loader)",
        "ExecuteWithSourcesContextAndDynamicSourceLoader(source, extraSources, executionContext, loader)",
    ] {
        assert_eq!(added.matches(call).count(), 2);
    }
    assert_eq!(added.matches("FerruleCsv.Serialize(primary,").count(), 3);
    assert_eq!(
        added.matches("FerruleCsv.SerializeBytes(primary,").count(),
        3
    );
    assert!(!added.contains("ExecuteOutputs"));
    assert!(!added.contains("ExecuteJson"));
    assert!(!added.contains("catch"));
    assert!(!added.contains("Parse"));
    assert_eq!(opted.files().len(), ordinary.files().len() + 2);
    for file in ordinary.files() {
        let expected = match file.path.as_str() {
            "GeneratedMapping.cs" => std::str::from_utf8(&file.contents).unwrap().replacen(
                "public static class GeneratedMapping\n{", "public static partial class GeneratedMapping\n{", 1).into_bytes(),
            "Ferrule.Generated.csproj" => std::str::from_utf8(&file.contents).unwrap().replacen(
                "    <Compile Include=\"GeneratedMapping.cs\" />\n",
                "    <Compile Include=\"GeneratedMapping.cs\" />\n    <Compile Include=\"GeneratedMapping.Csv.cs\" />\n", 1).into_bytes(),
            _ => file.contents.clone(),
        };
        assert_eq!(contents(&opted, file.path.as_str()), expected);
    }
    assert_eq!(emit(&dynamic).unwrap(), ordinary);
    dynamic.extra_sources.push(NamedSourceProgram {
        name: "Static".into(),
        source: SchemaNode::group("Static", Vec::new()),
        dynamic: None,
    });
    let mut static_only = dynamic.clone();
    static_only.extra_sources.remove(0);
    let static_artifacts = emit_with_csv_output(&static_only, &CsvOutputPolicy::default()).unwrap();
    let static_adapter = text(&static_artifacts, "GeneratedMapping.Csv.cs");
    let mixed = emit_with_csv_output(&dynamic, &CsvOutputPolicy::default()).unwrap();
    let mixed_adapter = text(&mixed, "GeneratedMapping.Csv.cs");
    assert_eq!(
        mixed_adapter,
        format!(
            "{}{}{}",
            static_adapter.strip_suffix("}\n").unwrap(),
            added,
            "}\n"
        )
    );
    assert_eq!(
        emit_with_csv_output(&plain, &CsvOutputPolicy::default()).unwrap(),
        old
    );
    let mixed_ordinary = emit(&dynamic).unwrap();
    let _mixed_opted = emit_with_csv_output(&dynamic, &CsvOutputPolicy::default()).unwrap();
    assert_eq!(emit(&dynamic).unwrap(), mixed_ordinary);
}
