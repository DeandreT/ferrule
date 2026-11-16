#[path = "owned_basic_rows.rs"]
mod primary_failures;
#[path = "owned_basic_windows.rs"]
mod windows;

mod private_corpus;
use private_corpus::{RUNNER_COMMON, generated_programs};
mod checked_failures;
mod checked_inputs;
mod source_failures;
use checked_failures::composed_ordered_failure_program;
use checked_inputs::CHECKED_RUNNER_COMMON;
use source_failures::FAILURE_COMMON;
mod public_family;
use public_family::{PUBLIC519_HOST_COMMON, PUBLIC519_LITERAL_IMPORT, public519_literal_program};
mod dynamic_failures;
use dynamic_failures::{
    failure586_dynamic, failure586_expression, failure586_program, failure586_retained,
};
mod value_maps;
use value_maps::{public582_lazy_program, public582_map_body};
mod failure_inventory;
mod root_pins;
use root_pins::{root_pin632_program, root_pin632_rows_program};
mod filters;
use filters::{
    FILTER644_HOST_COMMON, assert_control_free_typed_artifacts_match, filter644_host_programs,
    filter644_invalid, filter644_program, filter644_result, filter644_schema,
};
mod sort;
use sort::{
    sort668_host_programs, sort668_iteration, sort668_program, sort668_result,
    sort668_scalar_program, sort668_scope_error,
};
mod legacy_alias;

use super::*;
use mapping::{OwnedPosition, OwnedSourceField, OwnedSourceRef, SourceOwner};
use std::error::Error;
use std::io::Write;

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn owned(owner: SourceOwner, parts: &[&str], frame: Option<&[&str]>) -> Expression {
    Expression::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: frame.map(path),
    })
}
fn position(owner: SourceOwner, collection: Option<&[&str]>) -> Expression {
    Expression::OwnedPosition(OwnedPosition {
        owner,
        collection: collection.map(path),
    })
}
fn named(name: &str) -> SourceOwner {
    SourceOwner::Named { name: name.into() }
}
fn node(id: u32, expression: Expression) -> ExpressionNode {
    ExpressionNode { id, expression }
}
fn source_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::group("Ref", vec![SchemaNode::scalar("Value", ScalarType::String)]),
            SchemaNode::group(
                "Details",
                vec![SchemaNode::scalar("Counter", ScalarType::Int)],
            ),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Value", ScalarType::String),
                    SchemaNode::scalar("Score", ScalarType::Int),
                    SchemaNode::group(
                        "Details",
                        vec![SchemaNode::scalar("Counter", ScalarType::Int)],
                    ),
                    SchemaNode::group(
                        "Rows",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    )
}
fn scope(fields: &[(&str, u32, ScalarType)]) -> TargetScope {
    TargetScope {
        target_field: String::new(),
        repeating: false,
        iteration: None,
        construction: TargetConstruction::Group,
        bindings: fields
            .iter()
            .map(|(name, expression, ty)| Binding {
                target_field: (*name).into(),
                expression: *expression,
                target_domain: ScalarTargetDomain::Single(*ty),
                repeating: false,
            })
            .collect(),
        children: Vec::new(),
    }
}
fn basic(nodes: Vec<ExpressionNode>, fields: &[(&str, u32, ScalarType)]) -> Program {
    Program {
        execution_contract: None,
        xml_boundary: None,
        source: source_schema(),
        extra_sources: ["Unused", "Ref", " Ref 雪 ", "ref", "primary"]
            .into_iter()
            .map(|name| NamedSourceProgram {
                name: name.into(),
                source: source_schema(),
                dynamic: None,
            })
            .collect(),
        target: SchemaNode::group(
            "Output",
            fields
                .iter()
                .map(|(name, _, ty)| SchemaNode::scalar(*name, *ty))
                .collect(),
        ),
        expressions: nodes,
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: scope(fields),
        extra_targets: Vec::new(),
    }
}
fn private_result(label: &str, program: &Program) -> Result<String, EmitError> {
    eprintln!("INPUT {label} program={program:?}");
    let actual = super::super::owned_basic::render_basic_owned_source(program);
    eprintln!("RAW {label} source_result={actual:?}");
    if let Err(error) = &actual {
        eprintln!(
            "RAW {label} source_cause={:?}",
            error.source().map(ToString::to_string)
        );
    }
    actual
}
fn private_source(label: &str, program: &Program) -> String {
    private_result(label, program).expect("complete private source result captured")
}

#[test]
fn private_basic_literals_preserve_exact_owner_and_present_empty_identity() {
    let p = basic(
        vec![
            node(1, owned(named(" Ref 雪 "), &["Value"], None)),
            node(2, position(named("primary"), Some(&[]))),
            node(
                3,
                Expression::Const {
                    value: Value::String("quote\" backslash\\ newline\n tab\t".into()),
                },
            ),
        ],
        &[("Value", 1, ScalarType::String)],
    );
    let source = private_source("literal", &p);
    let access = super::super::owned_basic::render_owned_access(&p.expressions[1].expression);
    eprintln!("RAW literal absent-from-target position_body={access:?}");
    assert!(source.contains("SourceOwnerRef::Named(\" Ref \\u{96ea} \")"));
    assert!(source.contains(
        "context.owned_field(SourceOwnerRef::Named(\" Ref \\u{96ea} \"), &[\"Value\"], None)"
    ));
    assert_eq!(access, Some("{ let position = context.owned_position(SourceOwnerRef::Named(\"primary\"), Some(&[][..]))?; Ok(Value::Int(position as i64)) }".into()));
    let escaped = rust_string("quote\" backslash\\ newline\n tab\t");
    eprintln!("RAW literal control_escape={escaped:?}");
    assert_eq!(escaped, "\"quote\\\" backslash\\\\ newline\\n tab\\t\"");
}

#[test]
fn private_basic_source_holds_real_cohorts_and_keeps_explicit_policy() {
    let mut p = basic(
        vec![node(1, owned(named("Ref"), &["Rows", "Value"], None))],
        &[("Value", 1, ScalarType::String)],
    );
    p.root.iteration = Some(IterationPlan::owned_source(OwnedSourceRef {
        owner: named("Ref"),
        path: path(&["Rows"]),
    }));
    p.target = p.target.clone().repeating();
    p.root.repeating = true;
    let source = private_source("cohort", &p);
    assert!(source.contains("let cohort = context.walk_owned_source_cohort(SourceOwnerRef::Named(\"Ref\"), &[\"Rows\"])?;"));
    assert!(source.contains(
        "for candidate in cohort.iter() {\n        let item_context = candidate.context();"
    ));
    assert!(source.contains("declared: &'src [DeclaredNamedSourceRoot<'src>]"));
    assert!(source.contains("limits: OwnedSourceLimits"));
    assert!(source.contains("BasicSourceRoots::try_new(source, declared, limits)?"));
    assert!(!source.contains("OwnedSourceLimits {"));
    assert!(!source.contains("with_phase_position"));
    assert!(!source.contains("reindex_phase"));
    assert!(!source.contains("DynamicSourceLoader"));
    assert!(!source.contains("parse_json"));
    assert!(!source.contains("serialize_json"));
    let select = source.find("let selected = match selection").unwrap();
    let roots = source[select..].find("BasicSourceRoots::try_new").unwrap();
    let unknown = source[select..]
        .find("RuntimeError::UnknownTarget")
        .unwrap();
    assert!(unknown < roots);
}

#[test]
fn private_expression_seam_keeps_legacy_golden_bytes_and_public_errors() {
    let functions = BTreeMap::new();
    let expression = Expression::Const {
        value: Value::Int(7),
    };
    let ordinary = render_expression(9, &expression, "expression_", None, &functions, false, None);
    let inner = render_expression_in_context(
        9,
        &expression,
        "expression_",
        None,
        &functions,
        ExpressionEmission {
            basic: false,
            owned_leaf_nodes: false,
            filter_map: false,
            owner_function: None,
        },
    );
    eprintln!("RAW legacy ordinary={ordinary:?} common={inner:?}");
    assert_eq!(
        ordinary.unwrap(),
        "fn expression_9(context: &ScopeContext<'_>) -> Result<Value, RuntimeError> {\n    let _ = context;\n    Ok(Value::Int(7_i64))\n}\n\n"
    );
    assert_eq!(
        inner.unwrap(),
        "fn expression_9(context: &ScopeContext<'_>) -> Result<Value, RuntimeError> {\n    let _ = context;\n    Ok(Value::Int(7_i64))\n}\n\n"
    );
    let p = basic(
        vec![node(1, owned(SourceOwner::Primary, &["Value"], None))],
        &[("Value", 1, ScalarType::String)],
    );
    let actual = emit(
        &p,
        &Options {
            package_name: "valid_private_check".into(),
            runtime_dependency: RuntimeDependency::Version("0.0.0".into()),
        },
    );
    eprintln!("RAW public_gate={actual:?}");
    let expected = codegen::owned_expression_emission_error(
        codegen::OwnedSourceNamespace::Main,
        1,
        &p.expressions[0].expression,
    );
    match actual {
        Err(EmitError::InvalidProgram(error)) => assert_eq!(error, expected),
        other => panic!("captured public result differs: {other:?}"),
    }
    let invalid_package = emit(
        &p,
        &Options {
            package_name: "0invalid".into(),
            runtime_dependency: RuntimeDependency::Version("0.0.0".into()),
        },
    );
    eprintln!("RAW public_package={invalid_package:?}");
    assert!(
        matches!(invalid_package, Err(EmitError::InvalidPackageName(name)) if name == "0invalid")
    );
}

#[test]
fn private_basic_shape_and_control_denials_keep_full_typed_diagnostics() {
    let p = basic(
        vec![node(1, owned(named("Missing"), &["Value"], None))],
        &[("Value", 1, ScalarType::String)],
    );
    let actual = private_result("unknown-owner", &p);
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Structure {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 1,
                scope: None,
            },
            request: codegen::OwnedSourceDiagnosticRequest::Field {
                owner: codegen::OwnedSourceDiagnosticOwner::Named {
                    name: "Missing".into(),
                },
                path: path(&["Value"]),
                frame: None,
            },
            reason: codegen::OwnedSourceStructureReason::UnknownOwner,
        }),
    };
    match actual {
        Err(EmitError::InvalidProgram(error)) => assert_eq!(error, expected),
        other => panic!("captured structural result differs: {other:?}"),
    }
    let mut p = basic(
        vec![
            node(1, owned(SourceOwner::Primary, &["Rows", "Value"], None)),
            node(
                2,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
        ],
        &[("Value", 1, ScalarType::String)],
    );
    p.target = p.target.clone().repeating();
    p.root.repeating = true;
    p.root.iteration = Some(IterationPlan::new(
        codegen::OwnedSourceIteration::new(OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Rows"]),
        }),
        Some(2),
        None,
        Vec::new(),
        IterationOutput::Repeated,
    ));
    let actual = private_result("deferred-filter", &p);
    let route = mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary);
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Iteration {
                scope: route.clone(),
            },
            request: Some(codegen::OwnedSourceDiagnosticRequest::Iteration {
                owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                path: path(&["Rows"]),
            }),
            reason: codegen::OwnedSourceCapabilityReason::ScopeControl,
            origin: codegen::OwnedSourceCapabilityOrigin::Scope { scope: route },
        }),
    };
    match actual {
        Err(EmitError::InvalidProgram(error)) => assert_eq!(error, expected),
        other => panic!("captured control result differs: {other:?}"),
    }
}

#[test]
fn private_basic_omits_unused_valid_leaves_but_keeps_structural_admission() {
    let mut p = basic(
        vec![
            node(1, owned(SourceOwner::Primary, &["Value"], None)),
            node(99, owned(named("Ref"), &["Value"], None)),
        ],
        &[("Value", 1, ScalarType::String)],
    );
    p.user_functions.push(UserFunctionProgram {
        id: FunctionId::new(9),
        library: "unused".into(),
        name: "ordinary".into(),
        parameters: Vec::new(),
        output_type: ScalarType::Int,
        expressions: vec![node(
            1,
            Expression::Const {
                value: Value::Int(7),
            },
        )],
        output: 1,
    });
    let source = private_source("dead valid leaf", &p);
    assert!(source.contains("fn expression_1("));
    assert!(!source.contains("fn expression_99("));
    assert!(!source.contains("fn function_9("));
    p.expressions[1].expression = owned(named("Missing"), &["Value"], None);
    let actual = private_result("dead malformed owner", &p);
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Structure {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 99,
                scope: None,
            },
            request: codegen::OwnedSourceDiagnosticRequest::Field {
                owner: codegen::OwnedSourceDiagnosticOwner::Named {
                    name: "Missing".into(),
                },
                path: path(&["Value"]),
                frame: None,
            },
            reason: codegen::OwnedSourceStructureReason::UnknownOwner,
        }),
    };
    match actual {
        Err(EmitError::InvalidProgram(error)) => assert_eq!(error, expected),
        other => panic!("captured dead structural result differs: {other:?}"),
    }
}

/// Where one generated-host test builds and runs. Its evidence variable may
/// name a fresh absolute root, which is always kept; otherwise the run uses a
/// fresh temporary directory, removed unless `FERRULE_CODEGEN_KEEP_ARTIFACTS=1`.
/// `FERRULE_PRIVATE_RUST_BASIC_RUNTIME` may name another runtime source; it
/// defaults to this checkout's `codegen-runtime`. The cargo target is
/// `FERRULE_CODEGEN_HOST_TARGET_DIR` when set, else a directory inside the run
/// (see `host_target`).
struct HostRun {
    root: PathBuf,
    runtime: PathBuf,
    temporary: Option<PathBuf>,
}

impl HostRun {
    fn new(evidence: &str, label: &str) -> Self {
        let runtime = std::env::var_os("FERRULE_PRIVATE_RUST_BASIC_RUNTIME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../codegen-runtime")
                    .canonicalize()
                    .expect("checkout runtime source")
            });
        if let Some(root) = std::env::var_os(evidence) {
            return Self {
                root: root.into(),
                runtime,
                temporary: None,
            };
        }
        let parent = std::env::temp_dir().join(format!(
            "ferrule_codegen_rust_{label}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&parent);
        fs::create_dir_all(&parent).expect("temporary run directory");
        Self {
            root: parent.join("run"),
            runtime,
            temporary: Some(parent),
        }
    }
}

impl Drop for HostRun {
    fn drop(&mut self) {
        let Some(parent) = &self.temporary else {
            return;
        };
        if std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref() == Some("1".as_ref()) {
            eprintln!("retained generated host run at {}", parent.display());
        } else {
            let _ = fs::remove_dir_all(parent);
        }
    }
}

fn retained_file(path: &Path, bytes: &[u8]) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

// Source-only #541 prospective controls. The existing five tests and twenty-five
// generated recipes above remain exact; this bridge has a separate frozen corpus.
fn failure_rule(
    parts: &[&str],
    selection: codegen::FailureSelection,
    message: Option<u32>,
) -> codegen::FailureRule {
    codegen::FailureRule {
        iteration: codegen::FailureIteration::Source(codegen::SourceIteration::new(path(parts))),
        selection,
        message,
    }
}
fn failure_program(nodes: Vec<ExpressionNode>, rules: Vec<codegen::FailureRule>) -> Program {
    let mut nodes = nodes;
    nodes.push(node(100, owned(SourceOwner::Primary, &["Value"], None)));
    let mut program = basic(nodes, &[("Value", 100, ScalarType::String)]);
    program.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::group("Ref", vec![SchemaNode::scalar("Value", ScalarType::String)]),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Value", ScalarType::String),
                    SchemaNode::scalar("Flag", ScalarType::Bool),
                    SchemaNode::group(
                        "Rows",
                        vec![
                            SchemaNode::scalar("Value", ScalarType::String),
                            SchemaNode::scalar("Flag", ScalarType::Bool),
                        ],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    program.failure_rules = rules;
    program
}
fn failure_source_result(label: &str, program: &Program) -> Result<String, EmitError> {
    eprintln!("INPUT {label} complete Program={program:#?}");
    let actual = super::super::owned_basic::render_basic_owned_source(program);
    eprintln!("RAW {label} complete renderer Result={actual:#?}");
    if let Err(error) = &actual {
        eprintln!("RAW {label} complete cause={:?}", error.source());
    }
    actual
}
fn expected_failure_capability(
    index: usize,
    reason: codegen::OwnedSourceCapabilityReason,
) -> ProgramValidationError {
    let scope = mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary);
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Iteration { scope },
            request: None,
            reason,
            origin: codegen::OwnedSourceCapabilityOrigin::Failure { index },
        }),
    }
}
fn check_failure_source_error(label: &str, program: &Program, expected: ProgramValidationError) {
    eprintln!("EXPECTED {label} independent full diagnostic={expected:#?}");
    let actual = failure_source_result(label, program);
    match actual {
        Err(EmitError::InvalidProgram(actual)) => assert_eq!(actual, expected, "{label}"),
        other => panic!("{label}: complete captured source refusal differs: {other:#?}"),
    }
}

#[test]
fn private_source_failure_bridge_preserves_ordinary_bytes_and_pre_target_order() {
    let empty = basic(
        vec![node(
            1,
            Expression::Const {
                value: Value::Int(7),
            },
        )],
        &[("Value", 1, ScalarType::Int)],
    );
    eprintln!("INPUT S541-01 complete Program={empty:#?}");
    let old = super::super::failure::render(&empty);
    eprintln!("RAW S541-01 complete ordinary renderer={old:#?}");
    assert_eq!(
        old.unwrap(),
        "fn evaluate_failure_rules(context: &ScopeContext<'_>) -> Result<(), RuntimeError> {\n    let _ = context;\n    Ok(())\n}\n\n"
    );
    let rules = vec![failure_rule(
        &["Rows"],
        codegen::FailureSelection::WhenTrue(10),
        Some(11),
    )];
    let p = failure_program(
        vec![
            node(
                10,
                Expression::Const {
                    value: Value::Bool(false),
                },
            ),
            node(11, Expression::Raise { message: None }),
        ],
        rules,
    );
    let source = failure_source_result("S541-01 Basic order", &p).unwrap();
    let all = source
        .find("pub(crate) fn execute_private_owned_basic")
        .unwrap();
    let selected = source
        .find("pub(crate) fn execute_selected_private_owned_basic")
        .unwrap();
    let all_body = &source[all..selected];
    let selected_body = &source[selected..];
    assert!(
        all_body
            .find("evaluate_basic_failure_rules(&context)?;")
            .unwrap()
            < all_body.find("scope_root(&context)?").unwrap()
    );
    assert!(
        selected_body
            .find("evaluate_basic_failure_rules(&context)?;")
            .unwrap()
            < selected_body.find("scope_root(&context)?").unwrap()
    );
    assert!(
        selected_body.find("RuntimeError::UnknownTarget").unwrap()
            < selected_body.find("BasicSourceRoots::try_new").unwrap()
    );
    assert!(source.contains("let reached = context.walk_source_cohort(&[\"Rows\"]);"));
    assert!(source.contains("let cohort = reached?;"));
    assert!(source.contains(
        "for candidate in cohort.iter() {\n        let item_context = candidate.context();"
    ));
    assert!(source.contains("let reached_message = expression_11(item_context);"));
    assert!(source.find("if !selected").unwrap() < source.find("let reached_message").unwrap());
    assert!(source.contains("limits: OwnedSourceLimits"));
    assert!(!source.contains("OwnedSourceLimits {"));
}
fn checked_private_result(label: &str, program: &Program) -> Result<String, EmitError> {
    eprintln!("INPUT {label} complete_program={program:#?}");
    let actual = super::super::render_checked_private_owned_basic_source(program);
    eprintln!("RAW {label} complete_checked_source={actual:#?}");
    if let Err(error) = &actual {
        eprintln!(
            "RAW {label} complete_cause={:?}",
            error.source().map(ToString::to_string)
        );
    }
    actual
}
fn checked_catalogue_program() -> Program {
    let mut p = basic(
        vec![
            node(1, owned(SourceOwner::Primary, &["Value"], None)),
            node(
                2,
                Expression::SourceField {
                    path: path(&["Ref", "Value"]),
                    frame: None,
                },
            ),
            node(3, owned(named("ref"), &["Value"], None)),
            node(
                99,
                Expression::Const {
                    value: Value::String("unused.json".into()),
                },
            ),
        ],
        &[("Value", 1, ScalarType::String)],
    );
    p.extra_sources[0].dynamic = Some(codegen::DynamicSourceProgram {
        path: 99,
        driver: codegen::SourceIteration::new(Vec::new()).into(),
    });
    for (name, expression) in [("legacy", 2), ("owned", 3)] {
        p.extra_targets.push(NamedTargetProgram {
            name: name.into(),
            target: p.target.clone(),
            root: scope(&[("Value", expression, ScalarType::String)]),
        });
    }
    p
}
fn catalogue_rows<'a>(
    catalogue: &codegen::NamedInputCatalogue<'a>,
) -> Vec<(usize, &'a str, codegen::NamedInputAvailability)> {
    catalogue
        .entries
        .iter()
        .map(|entry| (entry.declaration_index, entry.name, entry.availability))
        .collect()
}

// Additive public Basic family. Each prospective result is retained before
// propagation or comparison; generated-host execution requires separate admission.
fn public519_options(name: &str) -> Options {
    Options {
        package_name: name.into(),
        runtime_dependency: RuntimeDependency::Version("0.1.0".into()),
    }
}
fn public519_result(
    label: &str,
    program: &Program,
    options: &Options,
) -> Result<ArtifactSet, EmitError> {
    eprintln!("INPUT {label} complete_program={program:#?} complete_options={options:#?}");
    let actual = emit_with_basic_owned_sources(program, options);
    eprintln!("RAW {label} complete_artifacts_or_error={actual:#?}");
    if let Err(error) = &actual {
        eprintln!("RAW {label} complete_cause={:?}", error.source());
    }
    actual
}
fn public519_outputs_program() -> Program {
    let mut p = basic(
        vec![
            node(1, owned(SourceOwner::Primary, &["Value"], None)),
            node(2, owned(named("Ref"), &["Value"], None)),
        ],
        &[("Value", 1, ScalarType::String)],
    );
    p.extra_targets.push(NamedTargetProgram {
        name: "Named".into(),
        target: SchemaNode::group(
            "Named",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        root: scope(&[("Value", 2, ScalarType::String)]),
    });
    p
}
fn public519_source(artifact: &ArtifactSet) -> &str {
    std::str::from_utf8(
        &artifact
            .files()
            .iter()
            .find(|f| f.path.as_str() == "src/lib.rs")
            .expect("captured artifact contains library")
            .contents,
    )
    .expect("generated library is UTF-8")
}
#[test]
fn public_basic_owned_family_has_exact_borrowed_policy_api_and_nonpublic_hooks() {
    let p = public519_outputs_program();
    let options = public519_options("public_basic_api");
    let private = super::super::owned_basic::render_checked_basic_owned_source(&p);
    eprintln!("INPUT A519-01 complete_program={p:#?} options={options:#?}");
    eprintln!("RAW A519-01 complete_private_renderer={private:#?}");
    let actual = public519_result("A519-01", &p, &options);
    let files = actual.expect("complete artifacts retained");
    let source = public519_source(&files);
    let private = private.expect("complete private renderer retained");
    assert_eq!(files.len(), 2);
    assert!(
        source.contains(
            "pub use codegen_runtime::{BasicNamedSourceInput, ExecutionContext, Instance,"
        )
    );
    assert!(source.contains("pub struct NamedOutput"));
    assert!(source.contains("pub struct ExecutionOutputs"));
    assert!(source.contains("pub enum SelectedTargetOutput"));
    assert!(source.contains("pub fn execute_with_basic_owned_sources<'name, 'src>("));
    assert!(
        source.contains("source: &'src Instance, supplied: &[BasicNamedSourceInput<'name, 'src>],")
    );
    assert!(
        source.contains("execution: Option<&ExecutionContext<'src>>, limits: OwnedSourceLimits,")
    );
    assert!(source.contains("pub fn execute_selected_with_basic_owned_sources<'name, 'src>("));
    assert!(source.contains("selection: TargetSelection<'_>,"));
    assert!(
        source.contains("execute_checked_private_owned_basic(source, supplied, execution, limits)")
    );
    assert!(source.contains("execute_selected_checked_private_owned_basic(source, supplied, execution, limits, selection)"));
    assert!(source.contains("pub(crate) fn execute_private_owned_basic"));
    assert!(source.contains("pub(crate) fn execute_checked_private_owned_basic"));
    assert!(!source.contains("pub fn private_checked_"));
    assert!(!source.contains("pub static PRIVATE_CHECKED"));
    assert_eq!(
        source
            .matches("BasicSourceRoots::try_from_named_inputs(")
            .count(),
        private
            .matches("BasicSourceRoots::try_from_named_inputs(")
            .count()
    );
    assert_eq!(
        source.matches("BasicSourceRoots::try_new(").count(),
        private.matches("BasicSourceRoots::try_new(").count()
    );
}
#[test]
fn public_basic_owned_literal_import_is_preserved_and_checked_import_is_unique() {
    let p = public519_literal_program();
    let options = public519_options("public_basic_literal_519");
    eprintln!(
        "INPUT A519-05 complete_program={p:#?} complete_options={options:#?} expected_literal={PUBLIC519_LITERAL_IMPORT:?}"
    );
    let private = super::super::owned_basic::render_checked_basic_owned_source(&p);
    eprintln!("RAW A519-05 complete_private_source_result={private:#?}");
    let actual = public519_result("A519-05", &p, &options);
    let private = private.expect("complete private renderer retained before propagation");
    let files = actual.expect("complete public artifacts or error retained before propagation");
    let source = public519_source(&files);
    let private_start = private.find("fn expression_1(").unwrap();
    let private_end =
        private[private_start..].find("\n}\n\n").unwrap() + private_start + "\n}\n\n".len();
    let public_start = source.find("fn expression_1(").unwrap();
    let public_end =
        source[public_start..].find("\n}\n\n").unwrap() + public_start + "\n}\n\n".len();
    eprintln!(
        "RAW A519-05 complete_private_const_function={:?} complete_public_const_function={:?}",
        &private[private_start..private_end],
        &source[public_start..public_end]
    );
    assert_eq!(
        &source[public_start..public_end],
        &private[private_start..private_end]
    );
    assert!(source[public_start..public_end].contains(&rust_string(PUBLIC519_LITERAL_IMPORT)));
    assert_eq!(
        source
            .lines()
            .filter(|line| *line == PUBLIC519_LITERAL_IMPORT)
            .count(),
        0
    );
    assert_eq!(
        source
            .lines()
            .filter(|line| *line
                == "use codegen_runtime::{BasicNamedSourceDeclaration, BasicNamedSourceMode};")
            .count(),
        1
    );
    assert_eq!(
        source
            .matches("pub use codegen_runtime::{BasicNamedSourceInput, ExecutionContext, Instance,")
            .count(),
        1
    );
    assert!(source.contains("pub struct NamedOutput"));
    assert!(source.contains("pub struct ExecutionOutputs"));
    assert!(source.contains("pub enum SelectedTargetOutput"));
}
#[test]
fn public_root_pin632_leaves_none_nonempty_positions_and_unused_policy_exact() {
    let mut program = root_pin632_program(None, false, false);
    program
        .expressions
        .push(node(99, owned(named("Ref"), &["Value"], Some(&[]))));
    let files = public519_result(
        "R632-unused",
        &program,
        &public519_options("root_pin_unused_632"),
    )
    .expect("full unused-leaf result retained");
    let source = public519_source(&files);
    assert!(!source.contains("basic_declared_root_nonrepeating"));
    assert!(!source.contains("owned_field_with_root_pin"));
    assert!(!source.contains("fn expression_99("));
    for (expression, expected) in [
        (
            owned(SourceOwner::Primary, &["Value"], None),
            "context.owned_field(SourceOwnerRef::Primary, &[\"Value\"], None)",
        ),
        (
            owned(named("Ref"), &["Rows", "Value"], Some(&["Rows"])),
            "context.owned_field(SourceOwnerRef::Named(\"Ref\"), &[\"Rows\", \"Value\"], Some(&[\"Rows\"][..]))",
        ),
        (
            position(SourceOwner::Primary, Some(&[])),
            "{ let position = context.owned_position(SourceOwnerRef::Primary, Some(&[][..]))?; Ok(Value::Int(position as i64)) }",
        ),
    ] {
        assert_eq!(
            super::super::owned_basic::render_owned_access(&expression),
            Some(expected.into())
        );
    }
}
#[test]
fn public_basic_owned_nodes_leave_enclosing_calls_maps_and_lazy_branches_exact() {
    let mut program = public582_lazy_program();
    program.expressions.push(node(
        7,
        Expression::Call {
            function: codegen::ScalarFunction::Upper,
            args: vec![6],
        },
    ));
    program.root.bindings[0].expression = 7;
    let private = super::super::owned_basic::render_checked_basic_owned_source(&program);
    eprintln!("RAW N650-nested complete_private={private:#?}");
    let files = public519_result(
        "N650-nested",
        &program,
        &public519_options("public_owned_nested_nodes"),
    )
    .expect("complete public artifacts retained");
    let private = private.expect("complete private rendering retained");
    let public = public519_source(&files);
    for id in [1, 3, 5, 6, 7, 70, 71, 72, 81, 82] {
        assert_eq!(
            public582_map_body(public, id),
            public582_map_body(&private, id),
            "enclosing/nonowned expression {id}"
        );
    }
    for id in [2, 4] {
        assert!(public582_map_body(public, id).contains(&format!(".with_owned_source_node({id})")));
        assert!(!public582_map_body(&private, id).contains("with_owned_source_node"));
    }
    assert_eq!(public.matches(".with_owned_source_node(").count(), 2);
    assert!(public582_map_body(public, 6).contains("expression_3(context)"));
    assert!(public582_map_body(public, 6).contains("expression_5(context)"));
}
#[test]
fn public_basic_owned_nodes_preserve_private_leaf_bytes_and_unannotated_setup_walks() {
    let program = root_pin632_rows_program();
    let prepared = super::super::owned_basic::render_basic_owned_source(&program);
    let checked = super::super::owned_basic::render_checked_basic_owned_source(&program);
    eprintln!("RAW N650-private complete_prepared={prepared:#?} complete_checked={checked:#?}");
    let files = public519_result(
        "N650-private",
        &program,
        &public519_options("public_owned_private_preservation"),
    )
    .expect("complete public artifacts retained");
    let prepared = prepared.expect("complete prepared rendering retained");
    let checked = checked.expect("complete checked rendering retained");
    assert!(!prepared.contains("with_owned_source_node"));
    assert!(!checked.contains("with_owned_source_node"));
    let public = public519_source(&files);
    for name in [
        "scope_root",
        "scope_extra_0",
        "execute_private_checked_basic_context",
    ] {
        let marker = format!("fn {name}(");
        let start = public.find(&marker).unwrap();
        let end = public[start..].find("\n}\n").unwrap() + start + "\n}\n".len();
        let private_start = checked.find(&marker).unwrap();
        let private_end =
            checked[private_start..].find("\n}\n").unwrap() + private_start + "\n}\n".len();
        assert_eq!(&public[start..end], &checked[private_start..private_end]);
        assert!(!public[start..end].contains("with_owned_source_node"));
    }
    let golden = "fn expression_5(context: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {\n    #[cfg(test)]\n    eprintln!(\"RAW PRIVATE_BASIC_EXPRESSION module={} node=5\", module_path!());\n    let _ = context;\n    { let position = context.owned_position(SourceOwnerRef::Primary, None)?; Ok(Value::Int(position as i64)) }\n}\n\n";
    assert_eq!(public582_map_body(&prepared, 5), golden);
    assert_eq!(public582_map_body(&checked, 5), golden);
}
#[test]
fn public_basic_filters_require_explicit_opt_in_and_keep_complete_default_private_refusals() {
    let p = filter644_program(SourceOwner::Primary);
    let options = public519_options("public_filter_opt_in_644");
    let expected = codegen::validate_program(&p).unwrap_err();
    let ordinary = emit(&p, &options);
    let default_basic = emit_with_basic_owned_sources(&p, &options);
    let prepared = super::super::owned_basic::render_basic_owned_source(&p);
    let checked = super::super::owned_basic::render_checked_basic_owned_source(&p);
    eprintln!(
        "RAW F644 default ordinary={ordinary:#?} basic={default_basic:#?} prepared={prepared:#?} checked={checked:#?} expected={expected:#?}"
    );
    filter644_invalid(ordinary, expected.clone());
    filter644_invalid(default_basic, expected.clone());
    for actual in [prepared, checked] {
        match actual {
            Err(EmitError::InvalidProgram(error)) => assert_eq!(error, expected),
            other => panic!("private filter gate changed: {other:#?}"),
        }
    }
    assert!(filter644_result("explicit filtered family", &p, &options).is_ok());
}
#[test]
fn public_basic_sort_requires_explicit_opt_in_and_preserves_full_refusal_priority() {
    let p = sort668_program(
        SourceOwner::Primary,
        SortFilterOrder::FilterThenSort,
        Some(4),
    );
    let options = public519_options("public_sort_opt_in_668");
    let expected = codegen::validate_program(&p).unwrap_err();
    let ordinary = emit(&p, &options);
    let basic = emit_with_basic_owned_sources(&p, &options);
    let prepared = super::super::owned_basic::render_basic_owned_source(&p);
    let checked = super::super::owned_basic::render_checked_basic_owned_source(&p);
    eprintln!(
        "RAW S668 prior gates ordinary={ordinary:#?} basic={basic:#?} prepared={prepared:#?} checked={checked:#?} expected={expected:#?}"
    );
    filter644_invalid(ordinary, expected.clone());
    filter644_invalid(basic, expected.clone());
    for actual in [prepared, checked] {
        match actual {
            Err(EmitError::InvalidProgram(error)) => assert_eq!(error, expected),
            other => panic!("full private sort refusal changed: {other:#?}"),
        }
    }
    let filter_expected = codegen::validate_basic_owned_filter_emission(&p).unwrap_err();
    filter644_invalid(
        filter644_result("sort remains outside filter-only profile", &p, &options),
        filter_expected,
    );
    assert!(sort668_result("explicit sorted profile", &p, &options).is_ok());
    let mut invalid = options;
    invalid.package_name = "0bad".into();
    match sort668_result("package precedes renderer", &p, &invalid) {
        Err(EmitError::InvalidPackageName(name)) => assert_eq!(name, "0bad"),
        other => panic!("full package refusal changed: {other:#?}"),
    }
    let mut malformed = p.clone();
    malformed.expressions.push(node(
        1,
        Expression::Const {
            value: Value::Int(9),
        },
    ));
    filter644_invalid(
        sort668_result("complete structure precedes package", &malformed, &invalid),
        ProgramValidationError::DuplicateExpression { node: 1 },
    );
    let mut missing = p;
    sort668_iteration(
        &mut missing,
        SortFilterOrder::SortThenFilter,
        Some(4),
        1,
        vec![codegen::SortKey {
            expression: 668668,
            descending: true,
        }],
    );
    filter644_invalid(
        sort668_result("missing secondary key precedes package", &missing, &invalid),
        ProgramValidationError::MissingSortExpression {
            target_path: Vec::new(),
            key: 1,
            expression: 668668,
        },
    );
}
