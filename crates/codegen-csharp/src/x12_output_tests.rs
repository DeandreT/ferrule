use crate::{X12EmitError, emit, emit_with_x12};
use codegen::{
    Binding, Expression, ExpressionNode, Program, TargetConstruction, TargetScope,
    X12BoundaryOptions, X12BoundaryPolicy,
};
use ir::{ScalarType, SchemaKind, SchemaNode};

use crate::generated_artifact_evidence::Evidence;

use crate::x12_line_ending_fixture as line_ending_fixture;

#[test]
fn crlf_companion_is_selected_once_and_manifest_covers_actual_runtime_inventory() {
    use codegen::{CompatibilityManifest, ManifestSelection, RuntimeIdentity, X12EnvelopeProfile};
    let program = codegen::lower(&line_ending_fixture::project(
        X12EnvelopeProfile::SingleTransaction,
    ))
    .unwrap();
    let lf = line_ending_fixture::policy(
        X12EnvelopeProfile::SingleTransaction,
        mapping::X12SegmentLineEnding::Lf,
        false,
    );
    let crlf = line_ending_fixture::policy(
        X12EnvelopeProfile::SingleTransaction,
        mapping::X12SegmentLineEnding::CrLf,
        false,
    );
    let before = emit_with_x12(&program, &lf).unwrap();
    let selected = emit_with_x12(&program, &crlf).unwrap();
    assert_eq!(before, emit_with_x12(&program, &lf).unwrap());
    assert_eq!(selected, emit_with_x12(&program, &crlf).unwrap());
    let path = "Runtime/X12/FerruleX12.OutputPolicy.cs";
    assert!(!before.files().iter().any(|file| file.path.as_str() == path));
    assert_eq!(
        selected
            .files()
            .iter()
            .filter(|file| file.path.as_str() == path)
            .count(),
        1
    );
    for original in before
        .files()
        .iter()
        .filter(|file| file.path.as_str().starts_with("Runtime/"))
    {
        assert_eq!(
            selected
                .files()
                .iter()
                .find(|file| file.path == original.path)
                .unwrap()
                .contents,
            original.contents
        );
    }
    assert!(
        !before
            .files()
            .iter()
            .any(|file| file.path.as_str() == "ferrule-manifest.json")
    );
    let options = crate::Options {
        manifest: ManifestSelection::Included,
        ..Default::default()
    };
    let files = crate::Generator::new(&options)
        .emit_with_x12(&program, &crlf)
        .unwrap();
    let manifest = files
        .files()
        .iter()
        .find(|file| file.path.as_str() == "ferrule-manifest.json")
        .unwrap();
    let manifest =
        CompatibilityManifest::from_json(std::str::from_utf8(&manifest.contents).unwrap()).unwrap();
    let actual = RuntimeIdentity::of_sources(
        files
            .files()
            .iter()
            .filter(|file| {
                file.path.as_str().starts_with("Runtime/")
                    && file.path.as_str() != "Runtime/FerruleRuntimeIdentity.cs"
            })
            .map(|file| (file.path.as_str(), file.contents.as_slice())),
    );
    assert_eq!(manifest.runtime.identity, actual);
    assert_eq!(manifest.runtime.package, "Ferrule.Runtime");
    assert_eq!(manifest.runtime.binding, codegen::RuntimeBinding::Vendored);
    assert_eq!(manifest.profile, codegen::GenerationProfile::X12);
    assert!(companion(&selected).contains("SerializeEmbeddedBytesWithLineEnding"));
    // New private names do not become global ordinary identity restrictions.
    assert!(crate::Identity::new("Toy.Assembly", "SegmentOutputProfile.Toy", "ToyMapping").is_ok());
}

#[test]
fn generated_cr_terminators_stay_refused_for_both_line_ending_policies() {
    let profile = codegen::X12EnvelopeProfile::SingleTransaction;
    let program = codegen::lower(&line_ending_fixture::project(profile)).unwrap();
    for ending in [
        mapping::X12SegmentLineEnding::Lf,
        mapping::X12SegmentLineEnding::CrLf,
    ] {
        let mut policy = line_ending_fixture::policy(profile, ending, false);
        policy.target.as_mut().unwrap().separators = Some(mapping::X12Separators {
            element: '*',
            component: ':',
            segment: '\r',
            repetition: None,
            release: None,
        });
        assert!(matches!(
            emit_with_x12(&program, &policy),
            Err(X12EmitError::Policy(
                codegen::X12BoundaryPolicyError::Separators { .. }
            ))
        ));
    }
}

#[test]
fn static_named_x12_targets_select_one_shared_output_partial() {
    use codegen::{
        DocumentBoundaryOptions as Boundary, NamedDocumentBoundaryOptions,
        StaticDocumentBoundaryPolicy, X12EnvelopeProfile,
    };
    let mut project = line_ending_fixture::project(X12EnvelopeProfile::SingleTransaction);
    project.extra_targets.push(mapping::NamedTarget {
        name: "Copy".into(),
        path: None,
        schema: project.target.clone(),
        options: Default::default(),
        root: project.root.clone(),
    });
    let program = codegen::lower(&project).unwrap();
    let selected = X12BoundaryOptions {
        segment_line_ending: mapping::X12SegmentLineEnding::CrLf,
        ..Default::default()
    };
    let policy = StaticDocumentBoundaryPolicy {
        source: Boundary::X12(Default::default()),
        target: Boundary::X12(selected.clone()),
        extra_sources: vec![],
        extra_targets: vec![NamedDocumentBoundaryOptions {
            name: "Copy".into(),
            options: Boundary::X12(selected),
        }],
    };
    let artifacts = crate::emit_with_static_document_adapters(&program, &policy).unwrap();
    assert_eq!(
        artifacts
            .files()
            .iter()
            .filter(|f| f.path.as_str() == "Runtime/X12/FerruleX12.OutputPolicy.cs")
            .count(),
        1
    );
    let source = &artifacts
        .files()
        .iter()
        .find(|f| f.path.as_str() == "GeneratedMapping.Documents.cs")
        .unwrap()
        .contents;
    assert!(
        std::str::from_utf8(source)
            .unwrap()
            .contains("SerializeEmbeddedBytesWithLineEnding")
    );
}

fn segment(name: &str, width: usize) -> SchemaNode {
    SchemaNode::group(
        name,
        (1..=width)
            .map(|index| SchemaNode::scalar(format!("{name}{index:02}"), ScalarType::String))
            .collect(),
    )
}

fn schema() -> SchemaNode {
    let mut isa = segment("ISA", 16);
    let SchemaKind::Group { children, .. } = &mut isa.kind else {
        unreachable!()
    };
    children[11].fixed = Some("00401".into());
    let mut gs = segment("GS", 8);
    let SchemaKind::Group { children, .. } = &mut gs.kind else {
        unreachable!()
    };
    children[7].fixed = Some("004010".into());
    SchemaNode::group(
        "Order",
        vec![
            isa,
            gs,
            segment("ST", 2),
            segment("W05", 3),
            segment("SE", 2),
            segment("GE", 2),
            segment("IEA", 2),
        ],
    )
}

fn program() -> Program {
    Program {
        execution_contract: None,
        xml_boundary: None,
        source: schema(),
        extra_sources: vec![],
        target: SchemaNode::group(
            "Result",
            vec![SchemaNode::scalar("OrderNumber", ScalarType::String)],
        ),
        expressions: vec![ExpressionNode {
            id: 1,
            expression: Expression::SourceField {
                frame: None,
                path: vec!["W05".into(), "W0502".into()],
            },
        }],
        user_functions: vec![],
        failure_rules: vec![],
        extra_targets: vec![],
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: Default::default(),
            bindings: vec![Binding {
                target_field: "OrderNumber".into(),
                expression: 1,
                target_domain: ScalarType::String.into(),
                repeating: false,
            }],
            children: vec![],
        },
    }
}

fn companion(artifacts: &codegen::ArtifactSet) -> String {
    String::from_utf8(
        artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.X12.cs")
            .unwrap()
            .contents
            .clone(),
    )
    .unwrap()
}

#[test]
fn x12_opt_in_is_deterministic_and_preserves_every_ordinary_runtime_source() {
    let candidate = program();
    let policy = X12BoundaryPolicy {
        source: Some(X12BoundaryOptions::default()),
        target: None,
    };
    let evidence = Evidence::new("csharp-x12-selection");
    evidence.debug("PROGRAM.original.txt", &candidate);
    evidence.debug("POLICY.original.txt", &policy);
    let before = emit(&candidate);
    let optional = emit_with_x12(&candidate, &policy);
    let repeated = emit_with_x12(&candidate, &policy);
    let after = emit(&candidate);
    evidence.artifacts("ordinary-before", &before);
    evidence.artifacts("optional", &optional);
    evidence.artifacts("repeated", &repeated);
    evidence.artifacts("ordinary-after", &after);
    let before = before.unwrap();
    let optional = optional.unwrap();
    evidence.assert_retained("optional", &optional);
    assert_eq!(before, after.unwrap());
    assert_eq!(optional, repeated.unwrap());
    assert_eq!(
        optional.files().len(),
        before.files().len() + 1 + super::x12_output::X12_SOURCES.len()
    );
    for original in before.files() {
        let expected = match original.path.as_str() {
            "GeneratedMapping.cs" => String::from_utf8(original.contents.clone())
                .unwrap()
                .replacen(
                    "public static class GeneratedMapping\n{",
                    "public static partial class GeneratedMapping\n{",
                    1,
                )
                .into_bytes(),
            "Ferrule.Generated.csproj" => String::from_utf8(original.contents.clone())
                .unwrap()
                .replacen(
                    "    <Compile Include=\"GeneratedMapping.cs\" />\n",
                    "    <Compile Include=\"GeneratedMapping.cs\" />\n    <Compile Include=\"GeneratedMapping.X12.cs\" />\n",
                    1,
                )
                .into_bytes(),
            _ => original.contents.clone(),
        };
        assert_eq!(
            optional
                .files()
                .iter()
                .find(|file| file.path == original.path)
                .unwrap()
                .contents,
            expected,
            "{}",
            original.path.as_str()
        );
    }
    for (path, source) in super::x12_output::X12_SOURCES {
        assert!(!before.files().iter().any(|file| file.path.as_str() == path));
        assert_eq!(
            optional
                .files()
                .iter()
                .find(|file| file.path.as_str() == path)
                .unwrap()
                .contents,
            source.as_bytes()
        );
    }
    let source = companion(&optional);
    assert!(source.contains("ParseX12(string source)"));
    assert!(source.contains("ParseX12Bytes(byte[] source)"));
    assert_eq!(
        source
            .matches("public static string ExecuteX12ToJson(")
            .count(),
        2
    );
    assert_eq!(
        source
            .matches("public static byte[] ExecuteX12ToJsonBytes(")
            .count(),
        2
    );
    assert!(!source.contains("SerializeX12("));
    assert!(!source.contains("ExecuteJsonToX12"));
    assert!(!source.contains("WithSources"));
}

#[test]
fn x12_direction_selection_exposes_only_its_owned_raw_boundaries() {
    let mut candidate = program();
    candidate.target = schema();
    candidate.expressions.clear();
    candidate.root.bindings.clear();
    candidate.root.construction = TargetConstruction::CopyCurrentSource;
    let evidence = Evidence::new("csharp-x12-directions");
    evidence.debug("PROGRAM.original.txt", &candidate);
    for source_x12 in [false, true] {
        let policy = X12BoundaryPolicy {
            source: source_x12.then(X12BoundaryOptions::default),
            target: Some(X12BoundaryOptions::default()),
        };
        evidence.debug(&format!("POLICY-{source_x12}.original.txt"), &policy);
        let observed = emit_with_x12(&candidate, &policy);
        evidence.artifacts(&format!("direction-{source_x12}"), &observed);
        let source = companion(&observed.unwrap());
        assert!(source.contains("SerializeX12(global::Ferrule.Runtime.FerruleInstance target)"));
        assert!(
            source.contains("SerializeX12Bytes(global::Ferrule.Runtime.FerruleInstance target)")
        );
        assert_eq!(source.contains("ParseX12(string source)"), source_x12);
        let method = if source_x12 {
            "ExecuteX12ToX12"
        } else {
            "ExecuteJsonToX12"
        };
        assert_eq!(
            source
                .matches(&format!("public static string {method}("))
                .count(),
            2
        );
        assert_eq!(
            source
                .matches(&format!("public static byte[] {method}Bytes("))
                .count(),
            2
        );
        assert_eq!(source.matches("var target = Execute(input);").count(), 2);
        assert_eq!(
            source
                .matches("var target = Execute(input, executionContext);")
                .count(),
            2
        );
        assert!(
            source.contains("SerializeEmbedded(X12TargetDescriptor, target, executionContext)")
        );
        assert!(
            source
                .contains("SerializeEmbeddedBytes(X12TargetDescriptor, target, executionContext)")
        );
        assert_eq!(
            source.matches("public static string SerializeX12(").count(),
            2
        );
        assert_eq!(
            source
                .matches("public static byte[] SerializeX12Bytes(")
                .count(),
            2
        );
    }
}

#[test]
fn x12_policy_refusal_precedes_artifacts_and_keeps_the_original_error() {
    let mut candidate = program();
    candidate.source.repeating = true;
    let policy = X12BoundaryPolicy {
        source: Some(X12BoundaryOptions::default()),
        target: None,
    };
    let evidence = Evidence::new("csharp-x12-refusal");
    evidence.debug("PROGRAM.original.txt", &candidate);
    evidence.debug("POLICY.original.txt", &policy);
    let observed = emit_with_x12(&candidate, &policy);
    evidence.artifacts("typed-refusal", &observed);
    assert!(
        evidence
            .path()
            .join("typed-refusal/ERROR.original.json")
            .is_file()
    );
    assert!(matches!(observed, Err(X12EmitError::Policy(_))));
}
