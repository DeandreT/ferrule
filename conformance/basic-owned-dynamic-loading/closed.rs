//! Refusal controls: older generation profiles, unsupported loading Projects
//! and directly constructed Programs. Expected errors are authored here.
use super::*;
use codegen::{
    OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Reason,
    OwnedSourceDiagnostic as Diagnostic, OwnedSourceDiagnosticLocation as Location,
    OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
    OwnedSourceNamespace as Namespace, OwnedSourceStructureReason as Structure,
    ProgramValidationError,
};
use mapping::{
    OwnedLookup, OwnedSourceRef, SourceScopeLocation, SourceScopeStep, SourceScopeTarget,
};
use projects::{DOCS, TITLE, owner, path};

fn named(name: &str) -> Owner {
    Owner::Named { name: name.into() }
}
fn docs_scope() -> SourceScopeLocation {
    SourceScopeLocation::root(SourceScopeTarget::Primary).descendant(SourceScopeStep::Child(0))
}
fn docs_origin() -> Origin {
    Origin::DynamicSource {
        index: 1,
        name: DOCS.into(),
    }
}
fn capability(
    location: Location,
    request: Option<Request>,
    reason: Reason,
    origin: Origin,
) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(Diagnostic::Capability {
            location,
            request,
            reason,
            origin,
        }),
    }
}

/// Every earlier profile refuses L01's loading scope as a dynamic owner.
pub fn loading_scope_closed() -> ProgramValidationError {
    capability(
        Location::Iteration {
            scope: docs_scope(),
        },
        Some(Request::Iteration {
            owner: named(DOCS),
            path: Vec::new(),
        }),
        Reason::DynamicOwner,
        docs_origin(),
    )
}

pub fn old_lower(project: &Project) -> Vec<Result<codegen::Program, codegen::LowerError>> {
    vec![
        codegen::lower(project),
        codegen::lower_with_basic_owned_lookup_find(project),
        codegen::lower_with_basic_owned_dynamic_field(project),
        codegen::lower_with_basic_owned_xml_serialize(project),
        codegen::lower_with_basic_owned_collection_aggregates(project),
        codegen::lower_with_basic_owned_joins(project),
    ]
}
pub fn old_validators(program: &codegen::Program) -> Vec<Result<(), ProgramValidationError>> {
    vec![
        codegen::validate_program(program),
        codegen::validate_program_with_basic_owned_lookup_find(program),
        codegen::validate_program_with_basic_owned_dynamic_field(program),
        codegen::validate_program_with_basic_owned_xml_serialize(program),
        codegen::validate_program_with_basic_owned_xml_mixed_content(program),
        codegen::validate_program_with_basic_owned_collection_aggregates(program),
        codegen::validate_program_with_basic_owned_joins(program),
        codegen::validate_basic_owned_dynamic_field_emission(program),
        codegen::validate_basic_owned_joins_emission(program),
    ]
}
pub fn old_catalogues(program: &codegen::Program) -> Vec<Result<(), ProgramValidationError>> {
    vec![
        codegen::legacy_named_input_admission_plan(program).map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_dynamic_field(program)
            .map(drop),
        codegen::legacy_named_input_admission_plan_with_basic_owned_joins(program).map(drop),
    ]
}
pub fn old_rust(
    program: &codegen::Program,
    options: &codegen_rust::Options,
) -> Vec<Result<codegen::ArtifactSet, codegen_rust::EmitError>> {
    vec![
        codegen_rust::emit(program, options),
        codegen_rust::emit_with_basic_owned_dynamic_field(program, options),
        codegen_rust::emit_with_basic_owned_xml_serialize(program, options),
        codegen_rust::emit_with_basic_owned_joins(program, options),
    ]
}
pub fn old_csharp(
    program: &codegen::Program,
) -> Vec<Result<codegen::ArtifactSet, codegen_csharp::EmitError>> {
    vec![
        codegen_csharp::emit(program),
        codegen_csharp::emit_with_basic_owned_dynamic_field(program),
        codegen_csharp::emit_with_basic_owned_xml_serialize(program),
        codegen_csharp::emit_with_basic_owned_joins(program),
    ]
}

/// Projects the loading profile refuses before any artifact exists.
pub fn projects() -> Vec<(&'static str, Project, Vec<codegen::Diagnostic>)> {
    // Only exact field and position reads see a loaded document; an owned
    // Lookup over the loaded owner keeps the dynamic-owner refusal.
    let mut lookup = projects::project("L01-primary-driver");
    let collection = OwnedSourceRef {
        owner: owner(DOCS),
        path: path(&["Lines"]),
    };
    lookup.graph.nodes.insert(
        40,
        Node::OwnedLookup(OwnedLookup {
            collection: collection.clone(),
            key: path(&["Code"]),
            matches: TITLE,
            value: path(&["Code"]),
        }),
    );
    lookup.root.children[0].bindings[0].node = 40;
    let refusal = capability(
        Location::Expression {
            namespace: Namespace::Main,
            node: 40,
            scope: Some(docs_scope()),
        },
        Some(Request::Iteration {
            owner: named(DOCS),
            path: path(&["Lines"]),
        }),
        Reason::DynamicOwner,
        docs_origin(),
    );
    let ProgramValidationError::OwnedSource { diagnostic } = refusal else {
        unreachable!("authored owned refusal")
    };
    // Loaded documents are never grouped: each keeps only its own driver row.
    let mut grouped = projects::project("L01-primary-driver");
    grouped.root.children[0].group_by = Some(projects::LABEL);
    vec![
        (
            "lookup-over-loaded-owner",
            lookup,
            vec![codegen::Diagnostic::OwnedSource { diagnostic }],
        ),
        (
            "grouped-loading-scope",
            grouped,
            vec![codegen::Diagnostic::Validation {
                location: "scope `Docs`".into(),
                message: "an owned dynamic loading scope cannot group `Docs` documents".into(),
            }],
        ),
    ]
}

/// Directly constructed Programs the loading profile refuses.
pub fn programs(
    base: &codegen::Program,
) -> Vec<(&'static str, codegen::Program, ProgramValidationError)> {
    let driver = |owner_name: &str, parts: &[&str]| {
        let mut program = base.clone();
        program.extra_sources[1]
            .dynamic
            .as_mut()
            .expect("owned dynamic declaration")
            .driver = codegen::OwnedSourceIteration::new(OwnedSourceRef {
            owner: owner(owner_name),
            path: path(parts),
        })
        .into();
        program
    };
    let location = || Location::DynamicDriver {
        index: 1,
        name: DOCS.into(),
    };
    let structure =
        |owner: Owner, parts: &[&str], reason: Structure| ProgramValidationError::OwnedSource {
            diagnostic: Box::new(Diagnostic::Structure {
                location: location(),
                request: Request::Iteration {
                    owner,
                    path: path(parts),
                },
                reason,
            }),
        };
    vec![
        (
            "self-driven",
            driver(DOCS, &["Lines"]),
            structure(named(DOCS), &["Lines"], Structure::SelfDrivenDynamicSource),
        ),
        (
            "unknown-driver-owner",
            driver("Missing", &["Files"]),
            structure(named("Missing"), &["Files"], Structure::UnknownOwner),
        ),
    ]
}
