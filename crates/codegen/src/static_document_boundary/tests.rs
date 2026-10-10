use super::*;
use crate::{DynamicSourceProgram, IterationPlan, SourceIteration};
use ir::{ScalarType, SchemaKind};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const BASE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../cli/tests/code_generation/static_document_adapters/fixtures/projects/base.json"
));
const INVERSE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../cli/tests/code_generation/static_document_adapters/fixtures/projects/inverse.json"
));
const COMPLETION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../cli/tests/code_generation/static_document_adapters/fixtures/projects/completion.json"
));
const NO_SOURCES: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../cli/tests/code_generation/static_document_adapters/fixtures/projects/no_sources.json"
));

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

fn record(label: &str, original: &impl std::fmt::Debug) -> TestResult<PathBuf> {
    let root = std::env::temp_dir().join(format!(
        "ferrule-static-documents274-{label}-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    std::fs::create_dir_all(&root)?;
    std::fs::write(
        root.join("COMPLETE-ORIGINAL.txt"),
        format!("{original:#?}\n"),
    )?;
    eprintln!("STATIC_DOCUMENTS274_ORIGINAL={}", root.display());
    Ok(root)
}

fn options(options: &mapping::FormatOptions) -> DocumentBoundaryOptions {
    if options.edi_kind == Some(mapping::EdiBoundaryKind::X12) {
        DocumentBoundaryOptions::X12(X12BoundaryOptions::from_format_options(options).unwrap())
    } else {
        crate::validate_x12_json_format_options(options).unwrap();
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
            .map(|side| NamedDocumentBoundaryOptions {
                name: side.name.clone(),
                options: options(&side.options),
            })
            .collect(),
        extra_targets: project
            .extra_targets
            .iter()
            .map(|side| NamedDocumentBoundaryOptions {
                name: side.name.clone(),
                options: options(&side.options),
            })
            .collect(),
    }
}

fn deep() -> SchemaNode {
    let mut schema = SchemaNode::scalar("Leaf", ScalarType::String);
    for index in (1..=128).rev() {
        schema = SchemaNode::group(format!("Level{index:03}"), vec![schema]);
    }
    schema
}

fn depth_refusal(
    result: &Result<StaticDocumentBoundaryProfile, StaticDocumentBoundaryError>,
    owner: &StaticDocumentBoundaryOwner,
) -> bool {
    matches!(result, Err(StaticDocumentBoundaryError::EmbeddedSchema { owner: actual,
        error: CodecError::DepthLimit { depth:129,max:128 } }) if actual == owner)
}

#[test]
fn complete_static_tables_preserve_own_schema_syntax_and_completion() -> TestResult<()> {
    let mut originals = Vec::new();
    for (name, text) in [
        ("base", BASE),
        ("inverse", INVERSE),
        ("completion", COMPLETION),
        ("no_sources", NO_SOURCES),
    ] {
        let project: Project = serde_json::from_str(text)?;
        let selected = policy(&project);
        let borrowed = prepare_static_document_project_boundaries(&project, &selected);
        let lowered = crate::lower(&project);
        let complete = lowered
            .as_ref()
            .ok()
            .map(|program| prepare_static_document_boundary(program, &selected));
        originals.push((name, project, selected, borrowed, lowered, complete));
    }
    record("complete-tables", &originals)?;
    for (name, project, _, borrowed, _, complete) in &originals {
        let before = borrowed
            .as_ref()
            .map_err(|error| format!("{name}: {error}"))?;
        let after = complete
            .as_ref()
            .ok_or("lowering did not complete")?
            .as_ref()
            .map_err(|error| format!("{name}: {error}"))?;
        assert_eq!(before, after);
        assert_eq!(
            before
                .extra_sources
                .iter()
                .map(|side| side.name.as_str())
                .collect::<Vec<_>>(),
            project
                .extra_sources
                .iter()
                .map(|side| side.name.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            before
                .extra_targets
                .iter()
                .map(|side| side.name.as_str())
                .collect::<Vec<_>>(),
            project
                .extra_targets
                .iter()
                .map(|side| side.name.as_str())
                .collect::<Vec<_>>()
        );
        let source_is_x12 = *name == "inverse" || *name == "no_sources";
        assert_eq!(
            before.source.format == DocumentBoundaryFormat::X12,
            source_is_x12
        );
        if let Some(reference) = before
            .extra_sources
            .iter()
            .find(|side| side.name == "Reference")
        {
            let descriptor: serde_json::Value =
                serde_json::from_str(&reference.boundary.descriptor)?;
            assert_eq!(
                descriptor["separators"],
                serde_json::json!({"element":"*","component":":","segment":"~"})
            );
        }
        if let Some(advice) = before
            .extra_targets
            .iter()
            .find(|side| side.name == "Advice")
        {
            let descriptor: serde_json::Value = serde_json::from_str(&advice.boundary.descriptor)?;
            assert_eq!(
                descriptor["separators"],
                serde_json::json!({"element":"|","component":">","segment":"!"})
            );
            assert_eq!(descriptor["autocomplete"].is_null(), *name != "completion");
        }
    }
    Ok(())
}

#[test]
fn borrowed_json_depth_refuses_before_lower_or_recursive_program_validation() -> TestResult<()> {
    let mut project: Project = serde_json::from_str(BASE)?;
    let selected = policy(&project);
    let mut program = crate::lower(&project)?;
    project.extra_sources[2].schema = deep();
    project.root.bindings[0].node = 999;
    program.extra_sources[2].source = deep();
    program.root.bindings[0].expression = 999;
    let borrowed = prepare_static_document_project_boundaries(&project, &selected);
    let direct = prepare_static_document_boundary(&program, &selected);
    let source_owner = StaticDocumentBoundaryOwner::NamedSource {
        index: 2,
        name: "Unused".into(),
    };
    record(
        "depth-before-graph",
        &(&project, &program, &borrowed, &direct, &source_owner),
    )?;
    assert!(depth_refusal(&borrowed, &source_owner));
    assert!(depth_refusal(&direct, &source_owner));

    let mut unselected: Project = serde_json::from_str(BASE)?;
    let mut unselected_program = crate::lower(&unselected)?;
    unselected.extra_targets[1].schema = deep();
    unselected_program.extra_targets[1].target = deep();
    let borrowed = prepare_static_document_project_boundaries(&unselected, &selected);
    let direct = prepare_static_document_boundary(&unselected_program, &selected);
    let target_owner = StaticDocumentBoundaryOwner::NamedTarget {
        index: 1,
        name: "Audit".into(),
    };
    record(
        "unselected-depth",
        &(
            &unselected,
            &unselected_program,
            &borrowed,
            &direct,
            &target_owner,
        ),
    )?;
    assert!(depth_refusal(&borrowed, &target_owner));
    assert!(depth_refusal(&direct, &target_owner));
    Ok(())
}

#[test]
fn complete_policy_membership_and_x12_requirement_are_explicit() -> TestResult<()> {
    let project: Project = serde_json::from_str(BASE)?;
    let program = crate::lower(&project)?;
    let selected = policy(&project);
    let mut outcomes = Vec::new();
    for category in [
        "missing",
        "unknown",
        "duplicate",
        "reordered",
        "target-missing",
    ] {
        let mut changed = selected.clone();
        match category {
            "missing" => {
                changed.extra_sources.pop();
            }
            "unknown" => changed.extra_sources[0].name = "Unknown".into(),
            "duplicate" => changed.extra_sources[1].name = "Supplement".into(),
            "reordered" => changed.extra_sources.swap(0, 1),
            _ => {
                changed.extra_targets.pop();
            }
        }
        let original = prepare_static_document_boundary(&program, &changed);
        outcomes.push((category, changed, original));
    }
    let mut only_json = selected.clone();
    only_json.source = DocumentBoundaryOptions::Json;
    only_json.target = DocumentBoundaryOptions::Json;
    for side in only_json
        .extra_sources
        .iter_mut()
        .chain(&mut only_json.extra_targets)
    {
        side.options = DocumentBoundaryOptions::Json;
    }
    let no_x12 = prepare_static_document_boundary(&program, &only_json);
    record("membership", &(&program, &outcomes, &only_json, &no_x12))?;
    for (category, _, original) in &outcomes {
        assert!(
            matches!(original, Err(StaticDocumentBoundaryError::PolicyNames {field})
            if *field == if *category == "target-missing" {"extra_targets"} else {"extra_sources"})
        );
    }
    assert!(matches!(
        no_x12,
        Err(StaticDocumentBoundaryError::NoX12Boundary)
    ));
    Ok(())
}

#[test]
fn dynamic_physical_sources_and_output_documents_remain_refused() -> TestResult<()> {
    let project: Project = serde_json::from_str(BASE)?;
    let program = crate::lower(&project)?;
    let selected = policy(&project);
    let mut originals = Vec::new();
    let mut dynamic = program.clone();
    dynamic.extra_sources[0].dynamic = Some(DynamicSourceProgram {
        path: 41,
        driver: SourceIteration::new(vec!["Items".into()]),
    });
    originals.push((
        "dynamic_source",
        prepare_static_document_boundary(&dynamic, &selected),
    ));
    let mut iterated = program.clone();
    iterated.extra_targets[0].root.iteration = Some(IterationPlan::source(vec!["Items".into()]));
    originals.push((
        "document_iteration",
        prepare_static_document_boundary(&iterated, &selected),
    ));
    let mut repeated = program.clone();
    repeated.target.repeating = true;
    originals.push((
        "document_iteration",
        prepare_static_document_boundary(&repeated, &selected),
    ));
    record("physical-plans", &(&program, &originals))?;
    for (expected, original) in originals {
        assert!(
            matches!(original,Err(StaticDocumentBoundaryError::ProgramField {field}) if field == expected)
        );
    }
    Ok(())
}

#[test]
fn unselected_x12_options_and_versions_are_not_discarded() -> TestResult<()> {
    let project: Project = serde_json::from_str(BASE)?;
    let selected = policy(&project);
    let mut program = crate::lower(&project)?;
    if let SchemaKind::Group { children, .. } = &mut program.extra_targets[0].target.kind {
        let isa = children
            .iter_mut()
            .find(|child| child.name == "ISA")
            .unwrap();
        if let SchemaKind::Group { children, .. } = &mut isa.kind {
            children
                .iter_mut()
                .find(|child| child.name == "ISA12")
                .unwrap()
                .fixed = Some("00502".into());
        }
    }
    let original = prepare_static_document_boundary(&program, &selected);
    record("unselected-version", &(&program, &selected, &original))?;
    assert!(matches!(original,Err(StaticDocumentBoundaryError::X12 {
        owner:StaticDocumentBoundaryOwner::NamedTarget {index:0,name},
        error,
    }) if name == "Advice" && matches!(error.as_ref(), X12BoundaryPolicyError::Schema {..})));
    Ok(())
}

#[test]
fn ordinary_copy_and_computed_json_constructions_remain_admitted() -> TestResult<()> {
    let project: Project = serde_json::from_str(BASE)?;
    let program = crate::lower(&project)?;
    let selected = policy(&project);
    let mut copied = program.clone();
    copied.target = copied.source.clone();
    copied.root.bindings.clear();
    copied.root.children.clear();
    copied.root.construction = TargetConstruction::CopyCurrentSource;
    let copied_original = prepare_static_document_boundary(&copied, &selected);

    let mut computed = program;
    computed.target = SchemaNode::group("Computed", vec![])
        .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::String))
        .ok_or("authored computed object schema")?;
    computed.root.bindings.clear();
    computed.root.children.clear();
    computed.root.construction = TargetConstruction::DynamicGroup {
        fixed_fields: vec![],
        bindings: vec![crate::DynamicTargetBinding {
            key: 114,
            value: 127,
            target_domain: ScalarType::String.into(),
        }],
        children: vec![],
        merge: false,
    };
    let computed_original = prepare_static_document_boundary(&computed, &selected);
    record(
        "ordinary-constructed-objects",
        &(&copied, &copied_original, &computed, &computed_original),
    )?;
    assert!(copied_original.is_ok());
    assert!(computed_original.is_ok());
    Ok(())
}
