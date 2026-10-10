use std::collections::BTreeSet;

use codegen::{
    DocumentBoundaryOptions, NamedDocumentBoundaryOptions, StaticDocumentBoundaryError,
    StaticDocumentBoundaryPolicy, X12BoundaryOptions,
};
use mapping::{EdiBoundaryKind, FormatOptions, Project};

use super::StaticDocumentEmitError;
use crate::generated_artifact_evidence::Evidence;

fn project() -> Project {
    serde_json::from_str(include_str!(
        "../../../cli/tests/code_generation/static_document_adapters/fixtures/projects/base.json"
    ))
    .unwrap()
}

fn options(options: &FormatOptions) -> DocumentBoundaryOptions {
    if options.edi_kind == Some(EdiBoundaryKind::X12) {
        DocumentBoundaryOptions::X12(X12BoundaryOptions::from_format_options(options).unwrap())
    } else {
        assert!(options.json_document);
        DocumentBoundaryOptions::Json
    }
}

fn policy(project: &Project) -> StaticDocumentBoundaryPolicy {
    StaticDocumentBoundaryPolicy {
        source: options(&project.source_options),
        target: options(&project.target_options),
        extra_sources: project
            .extra_sources
            .iter()
            .map(|source| NamedDocumentBoundaryOptions {
                name: source.name.clone(),
                options: options(&source.options),
            })
            .collect(),
        extra_targets: project
            .extra_targets
            .iter()
            .map(|target| NamedDocumentBoundaryOptions {
                name: target.name.clone(),
                options: options(&target.options),
            })
            .collect(),
    }
}

#[test]
fn static_document_artifacts_preserve_ordinary_runtime_and_complete_own_descriptors() {
    let project = project();
    let program = codegen::lower(&project).unwrap();
    let policy = policy(&project);
    let evidence = Evidence::new("csharp-static-documents");
    evidence.debug("PROGRAM.original.txt", &program);
    evidence.debug("POLICY.original.txt", &policy);
    let ordinary = crate::emit(&program);
    let emitted = crate::emit_with_static_document_adapters(&program, &policy);
    evidence.debug("ORDINARY.original.txt", &ordinary);
    evidence.debug("EMITTED.original.txt", &emitted);
    evidence.artifacts("artifacts", &emitted);
    let ordinary = ordinary.unwrap();
    let emitted = emitted.unwrap();
    assert_eq!(emitted.len(), ordinary.len() + 9);
    let extra: BTreeSet<_> = emitted
        .files()
        .iter()
        .filter(|file| !ordinary.files().iter().any(|old| old.path == file.path))
        .map(|file| file.path.as_str())
        .collect();
    assert_eq!(
        extra,
        BTreeSet::from([
            "GeneratedMapping.Documents.cs",
            "Runtime/X12/FerruleX12.cs",
            "Runtime/X12/FerruleX12.Schema.cs",
            "Runtime/X12/FerruleX12.Reader.cs",
            "Runtime/X12/FerruleX12.Numeric.cs",
            "Runtime/X12/FerruleX12.Writer.cs",
            "Runtime/X12/FerruleX12.Completion.cs",
            "Runtime/X12/FerruleX12.Lexical.cs",
            "Runtime/X12/FerruleX12Exception.cs",
        ])
    );
    for file in ordinary.files() {
        if !matches!(
            file.path.as_str(),
            "GeneratedMapping.cs" | "Ferrule.Generated.csproj"
        ) {
            assert_eq!(
                &file.contents,
                &emitted
                    .files()
                    .iter()
                    .find(|copy| copy.path == file.path)
                    .unwrap()
                    .contents
            );
        }
    }
    let profile = codegen::prepare_static_document_boundary(&program, &policy).unwrap();
    let source = std::str::from_utf8(
        &emitted
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.Documents.cs")
            .unwrap()
            .contents,
    )
    .unwrap();
    for descriptor in [&profile.source, &profile.target]
        .into_iter()
        .chain(profile.extra_sources.iter().map(|source| &source.boundary))
        .chain(profile.extra_targets.iter().map(|target| &target.boundary))
    {
        assert!(source.contains(&crate::literal::string(&descriptor.descriptor)));
    }
    let repeated = crate::emit_with_static_document_adapters(&program, &policy).unwrap();
    assert_eq!(emitted, repeated);
}

#[test]
fn static_document_policy_refusal_precedes_ordinary_emission() {
    let project = project();
    let mut program = codegen::lower(&project).unwrap();
    let mut policy = policy(&project);
    policy.extra_sources[1].name = "Undeclared".into();
    program.root.bindings[0].expression = u32::MAX;
    let evidence = Evidence::new("csharp-static-document-refusal");
    evidence.debug("PROGRAM.original.txt", &program);
    evidence.debug("POLICY.original.txt", &policy);
    let result = crate::emit_with_static_document_adapters(&program, &policy);
    evidence.debug("OUTCOME.original.txt", &result);
    assert!(matches!(
        result,
        Err(StaticDocumentEmitError::Policy(
            StaticDocumentBoundaryError::PolicyNames {
                field: "extra_sources"
            }
        ))
    ));
}
