use super::*;
use crate::owned_basic as emitter;
use mapping::{OwnedSourceField, OwnedSourceRef, SourceOwner};

fn options() -> Options {
    Options {
        package_name: "owned_json_api_host".into(),
        runtime_dependency: RuntimeDependency::Path(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../codegen-runtime")
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap()
                .into(),
        ),
    }
}

fn owned(owner: SourceOwner, path: &[&str]) -> Expression {
    Expression::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path.iter().map(|part| (*part).into()).collect(),
        },
        frame: None,
    })
}

fn node(id: u32, expression: Expression) -> ExpressionNode {
    ExpressionNode { id, expression }
}

fn scalar_scope(expression: u32) -> TargetScope {
    TargetScope {
        target_field: String::new(),
        repeating: false,
        iteration: None,
        construction: TargetConstruction::Scalar {
            expression,
            target_domain: ScalarTargetDomain::Single(ScalarType::String),
        },
        bindings: Vec::new(),
        children: Vec::new(),
    }
}

fn scalar_program() -> Program {
    // The minimum-policy case has a scalar primary, not a scalar target scope.
    // Control-free owned leaves are admitted in Group construction; Scalar
    // construction requires a separately permitted sort/window operation.
    Program {
        xml_boundary: None,
        source: SchemaNode::scalar("Input", ScalarType::String),
        extra_sources: Vec::new(),
        target: group_schema("Value"),
        expressions: vec![node(1, owned(SourceOwner::Primary, &[]))],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: group_scope(1),
        extra_targets: Vec::new(),
    }
}

fn group_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        "Document",
        vec![SchemaNode::scalar(name, ScalarType::String)],
    )
}

fn group_scope(expression: u32) -> TargetScope {
    TargetScope {
        construction: TargetConstruction::Group,
        bindings: vec![Binding {
            target_field: "Value".into(),
            expression,
            target_domain: ScalarTargetDomain::Single(ScalarType::String),
            repeating: false,
        }],
        ..scalar_scope(expression)
    }
}

fn named_program() -> Program {
    let mut program = scalar_program();
    program.source = group_schema("Value");
    program.target = group_schema("Value");
    program.root = group_scope(1);
    program.expressions = vec![
        node(1, owned(SourceOwner::Primary, &["Value"])),
        node(
            2,
            owned(
                SourceOwner::Named {
                    name: " Ref 雪 ".into(),
                },
                &["Exact"],
            ),
        ),
        node(
            3,
            owned(SourceOwner::Named { name: "ref".into() }, &["Lower"]),
        ),
        node(
            4,
            Expression::SourceField {
                frame: None,
                path: vec!["RequiredLegacy".into(), "Value".into()],
            },
        ),
        node(
            99,
            Expression::Const {
                value: Value::String("unused.json".into()),
            },
        ),
    ];
    program.extra_sources = vec![
        NamedSourceProgram {
            name: "Unavailable".into(),
            source: SchemaNode::scalar("Reserved", ScalarType::Bool),
            dynamic: Some(DynamicSourceProgram {
                path: 99,
                driver: SourceIteration::new(Vec::new()),
            }),
        },
        NamedSourceProgram {
            name: " Ref 雪 ".into(),
            source: group_schema("Exact"),
            dynamic: None,
        },
        NamedSourceProgram {
            name: "ref".into(),
            source: group_schema("Lower"),
            dynamic: None,
        },
        NamedSourceProgram {
            name: "RequiredLegacy".into(),
            source: group_schema("Value"),
            dynamic: None,
        },
    ];
    program.extra_targets = [(" Audit 雪 ", 2), ("audit", 3), ("legacy", 4)]
        .into_iter()
        .map(|(name, expression)| NamedTargetProgram {
            name: name.into(),
            target: group_schema("Value"),
            root: group_scope(expression),
        })
        .collect();
    program
}

fn guarded_program(failure: bool) -> Program {
    let mut program = scalar_program();
    program.source = group_schema("Value");
    program.target = group_schema("Value");
    program.root = group_scope(1);
    program.expressions = vec![
        node(1, owned(SourceOwner::Primary, &["Value"])),
        node(2, Expression::Raise { message: None }),
        node(
            3,
            Expression::Const {
                value: Value::String("global failure".into()),
            },
        ),
    ];
    program.extra_targets.push(NamedTargetProgram {
        name: "Bad".into(),
        target: group_schema("Value"),
        root: group_scope(2),
    });
    if failure {
        program.failure_rules.push(FailureRule {
            iteration: FailureIteration::Source(SourceIteration::new(Vec::new())),
            selection: FailureSelection::All,
            message: Some(3),
        });
    }
    program
}

fn source(artifacts: &ArtifactSet) -> &str {
    std::str::from_utf8(
        &artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "src/lib.rs")
            .unwrap()
            .contents,
    )
    .unwrap()
}

#[test]
fn owned_json_companion_is_appended_only_to_control_free_emission() {
    let program = scalar_program();
    let options = options();
    let typed = emitter::render_public_basic_owned_source(&program).unwrap();
    let artifacts = emit_with_basic_owned_sources(&program, &options).unwrap();
    let appended = source(&artifacts)
        .strip_prefix(&typed)
        .expect("typed source unchanged");
    assert_eq!(
        appended,
        super::super::owned_json_api::render(&program).unwrap()
    );
    assert_eq!(appended.matches("pub fn ").count(), 4);
    assert_eq!(
        artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "Cargo.toml")
            .unwrap()
            .contents,
        render_manifest(&options).as_bytes()
    );

    let mut ordinary = program.clone();
    ordinary.expressions[0].expression = Expression::Const {
        value: Value::String("ordinary".into()),
    };
    assert_eq!(
        source(&emit(&ordinary, &options).unwrap()),
        render_source(&ordinary).unwrap()
    );
    assert!(!source(&emit(&ordinary, &options).unwrap()).contains("admit_basic_json_inputs"));
    for (artifacts, expected) in [
        (
            emit_with_basic_owned_filters(&program, &options),
            emitter::render_public_basic_owned_filter_source(&program),
        ),
        (
            emit_with_basic_owned_sort(&program, &options),
            emitter::render_public_basic_owned_sort_source(&program),
        ),
        (
            emit_with_basic_owned_windows(&program, &options),
            emitter::render_public_basic_owned_window_source(&program),
        ),
        (
            emit_with_basic_owned_row_failures(&program, &options),
            emitter::render_public_basic_owned_row_failure_source(&program),
        ),
    ] {
        let artifacts = artifacts.unwrap();
        assert_eq!(source(&artifacts), expected.unwrap());
        assert!(!source(&artifacts).contains("NamedJsonInput"));
        assert!(!source(&artifacts).contains("admit_basic_json_inputs"));
    }
    assert!(
        !emitter::render_checked_basic_owned_source(&program)
            .unwrap()
            .contains("NamedJsonInput")
    );
}

#[test]
fn owned_json_base_profile_keeps_owned_scalar_target_construction_closed() {
    use codegen::{
        OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Capability,
        OwnedSourceDiagnostic as Diagnostic, OwnedSourceDiagnosticLocation as Location,
        OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
        OwnedSourceNamespace as Namespace,
    };
    use mapping::{SourceScopeLocation, SourceScopeTarget};

    // Retain the original invalid fixture as a negative profile guard rather
    // than relaxing the production gate to make a JSON adapter test pass.
    let mut program = scalar_program();
    program.target = SchemaNode::scalar("Output", ScalarType::String);
    program.root = scalar_scope(1);
    let scope = SourceScopeLocation::root(SourceScopeTarget::Primary);
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(Diagnostic::Capability {
            location: Location::Expression {
                namespace: Namespace::Main,
                node: 1,
                scope: Some(scope.clone()),
            },
            request: Some(Request::Field {
                owner: Owner::Primary,
                path: Vec::new(),
                frame: None,
            }),
            reason: Capability::SpecialConstruction,
            origin: Origin::Scope { scope },
        }),
    };
    for result in [
        emit_with_basic_owned_sources(&program, &options()).map(|_| ()),
        emitter::render_public_basic_owned_source(&program).map(|_| ()),
    ] {
        match result {
            Err(EmitError::InvalidProgram(actual)) => assert_eq!(actual, expected),
            other => panic!("owned scalar target construction must remain closed: {other:?}"),
        }
    }
}

#[test]
fn owned_json_companion_uses_canonical_descriptors_and_one_checked_context() {
    let program = named_program();
    let adapters = super::super::owned_json_api::render(&program).unwrap();
    let expected = program
        .extra_sources
        .iter()
        .map(|source| {
            rust_string(
                &serialize_embedded_schema(&source.source, codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES)
                    .unwrap(),
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    assert!(adapters.contains(&format!(
        "const BASIC_OWNED_EXTRA_SOURCE_JSON_SCHEMAS: &[&str] = &[{expected}];"
    )));
    assert!(adapters.contains("BASIC_OWNED_EXTRA_SOURCE_JSON_SCHEMAS[declaration_index]"));
    for name in [
        "execute_json_outputs_with_basic_owned_sources",
        "execute_json_bytes_outputs_with_basic_owned_sources",
        "execute_json_selected_target_with_basic_owned_sources",
        "execute_json_bytes_selected_target_with_basic_owned_sources",
    ] {
        let function = &adapters[adapters.find(&format!("pub fn {name}(")).unwrap()..];
        let function = function.split("\n}\n").next().unwrap();
        assert!(function.contains("limits: OwnedSourceLimits,"));
        let admission = function.find("admit_basic_json_inputs(").unwrap();
        let primary_parse = function.find("let source = parse_json").unwrap();
        let named_parse = function.find("admission.parse_named_with(").unwrap();
        let roots = function.find("parsed.basic_roots(&source)?").unwrap();
        let evaluation = function
            .find("execute_private_checked_basic_context(")
            .unwrap();
        let serialization = function.rfind("serialize_basic_owned_").unwrap();
        assert!(admission < primary_parse && primary_parse < named_parse && named_parse < roots);
        assert!(roots < evaluation && evaluation < serialization);
        if name.contains("selected_target") {
            assert!(function.find("RuntimeError::UnknownTarget").unwrap() < admission);
            assert!(function.contains("PRIVATE_CHECKED_PRIMARY_INPUTS"));
            assert!(function.contains("PRIVATE_CHECKED_NAMED_INPUTS_2"));
        } else {
            assert!(function.contains("PRIVATE_CHECKED_ALL_INPUTS"));
        }
    }
    for call in [
        "inputs.iter()",
        "admit_basic_json_inputs(",
        "admission.parse_named_with(",
        "parsed.basic_roots(&source)?",
        "execute_private_checked_basic_context(",
        "roots.root_context()?",
    ] {
        assert_eq!(adapters.matches(call).count(), 4, "{call}");
    }
    for excluded in [
        "BasicNamedSourceInput {",
        "NamedInput {",
        ".collect::<Vec<_>>()",
        "OwnedSourceLimits {",
        "OwnedSourceLimits::default",
        "DynamicSourceLoader",
        "execute_checked_private_owned_basic(",
        "execute_selected_checked_private_owned_basic(",
    ] {
        assert!(!adapters.contains(excluded), "{excluded}");
    }
    assert!(adapters.contains("serialize_json_compact_bytes"));
    assert!(adapters.contains("parse_json_bytes_with_utf8_cause"));
}

#[test]
fn owned_json_generated_external_api_and_required_policy_compile_checks() {
    let directory = TempDir::new("owned_json_api");
    let options = options();
    let mut invalid_output = scalar_program();
    let mut fixed_value = SchemaNode::scalar("Value", ScalarType::String);
    fixed_value.fixed = Some("allowed".into());
    invalid_output.target = SchemaNode::group("Document", vec![fixed_value]);
    let programs = [
        ("scalar", scalar_program()),
        ("named", named_program()),
        ("guarded", guarded_program(false)),
        ("failure", guarded_program(true)),
        ("invalid_output", invalid_output),
    ];
    let mut library = String::new();
    for (name, program) in programs {
        let artifacts = emit_with_basic_owned_sources(&program, &options).unwrap();
        library.push_str(&format!("pub mod {name} {{\n{}\n}}\n", source(&artifacts)));
    }
    fs::create_dir_all(directory.path().join("src")).unwrap();
    fs::create_dir_all(directory.path().join("tests")).unwrap();
    fs::write(
        directory.path().join("Cargo.toml"),
        render_manifest(&options),
    )
    .unwrap();
    fs::write(directory.path().join("src/lib.rs"), library).unwrap();
    fs::write(directory.path().join("tests/public_api.rs"), EXTERNAL_API).unwrap();
    let output = Command::new("cargo")
        .args(["test", "--offline", "--test", "public_api"])
        .current_dir(directory.path())
        .generated_host_output(directory.path())
        .unwrap();
    assert!(
        output.status.success(),
        "generated public JSON API failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    fs::write(directory.path().join("tests/omission.rs"), OMITTED_POLICY).unwrap();
    let output = Command::new("cargo")
        .args(["check", "--offline", "--test", "omission"])
        .current_dir(directory.path())
        .generated_host_output(directory.path())
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "omitted policies unexpectedly compiled"
    );
    assert_eq!(stderr.matches("error[E0061]").count(), 4, "{stderr}");
    assert!(
        !stderr.contains("error[E0425]"),
        "all four adapters must exist: {stderr}"
    );
}

const EXTERNAL_API: &str = r##"
use owned_json_api_host::{scalar, named, guarded, failure, invalid_output};
use codegen_runtime::{ExecutionContext, JsonBoundaryError, OwnedSourceError, OwnedSourceLimitKind,
    OwnedSourceLimits, RuntimeError, TargetSelection};
use std::{error::Error, path::Path};

fn limits() -> OwnedSourceLimits {
    OwnedSourceLimits { max_roots:8, max_active_frames:16, max_path_segments:64,
        max_metadata_bytes:65536, max_walk_contexts:64 }
}
fn minimum() -> OwnedSourceLimits {
    OwnedSourceLimits { max_roots:1, max_active_frames:0, max_path_segments:0,
        max_metadata_bytes:0, max_walk_contexts:0 }
}
fn execution_error<T:std::fmt::Debug>(result:Result<T,JsonBoundaryError>, expected:RuntimeError) {
    let error=result.unwrap_err();
    assert_eq!(error.source().unwrap().downcast_ref::<RuntimeError>(),Some(&expected));
    assert_eq!(error,JsonBoundaryError::Execution(expected));
}

#[test]
fn scalar_minimal_policy_admits_all_four_public_signatures() {
    let execution=ExecutionContext::new(Path::new("minimal.mapping"));
    let source="\"P.root\"";
    let expected=r#"{"Value":"P.root"}"#;
    let output=scalar::execute_json_outputs_with_basic_owned_sources(source,&[],Some(&execution),minimum()).unwrap();
    assert_eq!(output,scalar::JsonExecutionOutputs {primary:expected.into(),extras:Vec::new()});
    let output=scalar::execute_json_bytes_outputs_with_basic_owned_sources(source.as_bytes(),&[],None,minimum()).unwrap();
    assert_eq!(output,scalar::JsonBytesExecutionOutputs {primary:expected.as_bytes().to_vec(),extras:Vec::new()});
    assert_eq!(scalar::execute_json_selected_target_with_basic_owned_sources(source,&[],None,minimum(),TargetSelection::Primary).unwrap(),scalar::SelectedJsonTargetOutput::Primary(expected.into()));
    assert_eq!(scalar::execute_json_bytes_selected_target_with_basic_owned_sources(source.as_bytes(),&[],None,minimum(),TargetSelection::Primary).unwrap(),scalar::SelectedJsonBytesTargetOutput::Primary(expected.as_bytes().to_vec()));
}

#[test]
fn canonical_ordinals_preserve_names_descriptors_order_and_compact_bytes() {
    let source=r#"{"Value":"P.root"}"#;
    let supplied=[
        named::NamedJsonInput {name:"RequiredLegacy",document:r#"{"Value":"legacy"}"#},
        named::NamedJsonInput {name:"ref",document:r#"{"Lower":"lower"}"#},
        named::NamedJsonInput {name:" Ref 雪 ",document:r#"{"Exact":"exact"}"#},
    ];
    let before=supplied;
    let output=named::execute_json_outputs_with_basic_owned_sources(source,&supplied,None,limits()).unwrap();
    assert_eq!(output,named::JsonExecutionOutputs {primary:source.into(),extras:vec![
        named::NamedJsonOutput {name:" Audit 雪 ",document:r#"{"Value":"exact"}"#.into()},
        named::NamedJsonOutput {name:"audit",document:r#"{"Value":"lower"}"#.into()},
        named::NamedJsonOutput {name:"legacy",document:r#"{"Value":"legacy"}"#.into()},
    ]});
    assert_eq!(supplied,before);
    let bytes=supplied.iter().map(|input|named::NamedJsonBytesInput {name:input.name,document:input.document.as_bytes()}).collect::<Vec<_>>();
    let output_bytes=named::execute_json_bytes_outputs_with_basic_owned_sources(source.as_bytes(),&bytes,None,limits()).unwrap();
    assert_eq!(output_bytes.primary,output.primary.as_bytes());
    for (bytes,text) in output_bytes.extras.iter().zip(&output.extras) {
        assert_eq!(bytes.name,text.name);assert_eq!(bytes.document,text.document.as_bytes());
    }
    let output=named::execute_json_selected_target_with_basic_owned_sources(source,&supplied[1..],None,limits(),TargetSelection::Named(" Audit 雪 ")).unwrap();
    assert_eq!(output,named::SelectedJsonTargetOutput::Named(named::NamedJsonOutput {name:" Audit 雪 ",document:r#"{"Value":"exact"}"#.into()}));
    let output=named::execute_json_bytes_selected_target_with_basic_owned_sources(source.as_bytes(),&bytes[1..],None,limits(),TargetSelection::Named("audit")).unwrap();
    assert_eq!(output,named::SelectedJsonBytesTargetOutput::Named(named::NamedJsonBytesOutput {name:"audit",document:br#"{"Value":"lower"}"#.to_vec()}));
}

#[test]
fn selected_target_and_metadata_admission_precede_parsing() {
    let bad=[named::NamedJsonInput {name:"not declared",document:"not JSON"}];
    let mut zero=minimum();zero.max_roots=0;
    execution_error(named::execute_json_selected_target_with_basic_owned_sources("not JSON",&bad,None,zero,TargetSelection::Named("AUDIT")),RuntimeError::UnknownTarget {name:"AUDIT".into()});
    execution_error(named::execute_json_selected_target_with_basic_owned_sources("not JSON",&bad,None,zero,TargetSelection::Primary),RuntimeError::OwnedSource(OwnedSourceError::MetadataLimit {kind:OwnedSourceLimitKind::Roots,used:0,requested:5,max:0}));
    execution_error(named::execute_json_outputs_with_basic_owned_sources("not JSON",&[],None,limits()),RuntimeError::MissingNamedSource {name:"RequiredLegacy"});
    execution_error(named::execute_json_selected_target_with_basic_owned_sources("not JSON",&bad,None,limits(),TargetSelection::Primary),RuntimeError::UnexpectedNamedSource {name:"not declared".into()});
    let mut bounded=limits();bounded.max_metadata_bytes=["Unavailable"," Ref 雪 ","ref","RequiredLegacy"].iter().map(|name|name.len()).sum();
    execution_error(named::execute_json_selected_target_with_basic_owned_sources("not JSON",&bad,None,bounded,TargetSelection::Primary),RuntimeError::OwnedSource(OwnedSourceError::MetadataLimit {kind:OwnedSourceLimitKind::MetadataBytes,used:bounded.max_metadata_bytes,requested:"not declared".len(),max:bounded.max_metadata_bytes}));
    let duplicate=[named::NamedJsonInput {name:"ref",document:"not JSON"};2];
    execution_error(named::execute_json_selected_target_with_basic_owned_sources("not JSON",&duplicate,None,limits(),TargetSelection::Primary),RuntimeError::DuplicateNamedSource {name:"ref"});
    let reserved=[named::NamedJsonInput {name:"Unavailable",document:"not JSON"}];
    execution_error(named::execute_json_selected_target_with_basic_owned_sources("not JSON",&reserved,None,limits(),TargetSelection::Primary),RuntimeError::UnexpectedNamedSource {name:"Unavailable".into()});
}

#[test]
fn primary_then_all_supplied_documents_parse_in_supplied_order() {
    let source=r#"{"Value":"P.root"}"#;
    let exact=named::NamedJsonInput {name:" Ref 雪 ",document:r#"{"Exact":true}"#};
    let lower=named::NamedJsonInput {name:"ref",document:r#"{"Lower":true}"#};
    let error=named::execute_json_selected_target_with_basic_owned_sources(source,&[lower,exact],None,limits(),TargetSelection::Primary).unwrap_err();
    assert!(matches!(error,JsonBoundaryError::InvalidInput {ref message} if message.contains("Lower")),"{error:?}");
    let error=named::execute_json_selected_target_with_basic_owned_sources(source,&[exact,lower],None,limits(),TargetSelection::Primary).unwrap_err();
    assert!(matches!(error,JsonBoundaryError::InvalidInput {ref message} if message.contains("Exact")),"{error:?}");
    let error=named::execute_json_selected_target_with_basic_owned_sources("not JSON",&[exact],None,limits(),TargetSelection::Primary).unwrap_err();
    assert_eq!(error,codegen_runtime::parse_json(r#"{"name":"Document","kind":{"kind":"group","children":[{"name":"Value","kind":{"kind":"scalar","ty":"string"}}]}}"#,"not JSON").unwrap_err());
    let invalid=b"{\"Lower\":\"ok\xff\"}";
    let expected=std::str::from_utf8(invalid).unwrap_err();
    let input=[named::NamedJsonBytesInput {name:"ref",document:invalid}];
    let error=named::execute_json_bytes_selected_target_with_basic_owned_sources(source.as_bytes(),&input,None,limits(),TargetSelection::Primary).unwrap_err();
    assert_eq!(error.source().unwrap().downcast_ref::<std::str::Utf8Error>(),Some(&expected));
    let error=scalar::execute_json_bytes_outputs_with_basic_owned_sources(b"\"ok\xff\"",&[],None,minimum()).unwrap_err();
    assert_eq!(error.source().unwrap().downcast_ref::<std::str::Utf8Error>().unwrap().valid_up_to(),3);
}

#[test]
fn selected_execution_skips_other_targets_but_not_global_failures() {
    let source=r#"{"Value":"P.root"}"#;
    assert_eq!(guarded::execute_json_selected_target_with_basic_owned_sources(source,&[],None,limits(),TargetSelection::Primary).unwrap(),guarded::SelectedJsonTargetOutput::Primary(source.into()));
    execution_error(guarded::execute_json_outputs_with_basic_owned_sources(source,&[],None,limits()),RuntimeError::MappingException {node:2,message:None});
    execution_error(guarded::execute_json_bytes_selected_target_with_basic_owned_sources(source.as_bytes(),&[],None,limits(),TargetSelection::Named("Bad")),RuntimeError::MappingException {node:2,message:None});
    execution_error(failure::execute_json_selected_target_with_basic_owned_sources(source,&[],None,limits(),TargetSelection::Primary),RuntimeError::MappingFailure {rule:1,message:Some("global failure".into())});
    execution_error(failure::execute_json_bytes_outputs_with_basic_owned_sources(source.as_bytes(),&[],None,limits()),RuntimeError::MappingFailure {rule:1,message:Some("global failure".into())});
}

#[test]
fn complete_schema_serialization_finishes_before_any_success_escapes() {
    for result in [
        invalid_output::execute_json_outputs_with_basic_owned_sources("\"wrong\"",&[],None,minimum()).map(|_|()),
        invalid_output::execute_json_bytes_outputs_with_basic_owned_sources(b"\"wrong\"",&[],None,minimum()).map(|_|()),
        invalid_output::execute_json_selected_target_with_basic_owned_sources("\"wrong\"",&[],None,minimum(),TargetSelection::Primary).map(|_|()),
        invalid_output::execute_json_bytes_selected_target_with_basic_owned_sources(b"\"wrong\"",&[],None,minimum(),TargetSelection::Primary).map(|_|()),
    ] {assert!(matches!(result,Err(JsonBoundaryError::InvalidOutput {..})),"{result:?}");}
}
"##;

const OMITTED_POLICY: &str = r#"
use owned_json_api_host::scalar;
use codegen_runtime::TargetSelection;
#[test] fn omitted_policy_does_not_compile() {
    let _=scalar::execute_json_outputs_with_basic_owned_sources("\"P.root\"",&[],None);
    let _=scalar::execute_json_bytes_outputs_with_basic_owned_sources(b"\"P.root\"",&[],None);
    let _=scalar::execute_json_selected_target_with_basic_owned_sources("\"P.root\"",&[],None,TargetSelection::Primary);
    let _=scalar::execute_json_bytes_selected_target_with_basic_owned_sources(b"\"P.root\"",&[],None,TargetSelection::Primary);
}
"#;
