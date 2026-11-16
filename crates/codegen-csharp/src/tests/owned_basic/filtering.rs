use super::*;

fn retain_public_filter_consumer(label: &str, program: &Program, body: &str) -> String {
    eprintln!("INPUT {label} complete filter Program={program:#?}");
    let actual = crate::emit_with_basic_owned_filters(program);
    eprintln!("RAW {label} complete filter artifact result={actual:#?}");
    let artifacts = actual.expect("complete artifact/refusal retained before propagation");
    let source = String::from_utf8(
        artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.cs")
            .unwrap()
            .contents
            .clone(),
    )
    .expect("complete emitted mapping is UTF-8");
    let host = public_basic_consumer(body, false);
    eprintln!("RAW {label} complete independent public consumer={host}");
    if let Some(base) = std::env::var_os("FERRULE_PUBLIC_BASIC_CSHARP_HOST_DIR") {
        let base = std::path::PathBuf::from(base);
        assert!(
            base.is_absolute(),
            "root supplies a fresh absolute evidence namespace"
        );
        let root = base.join(label);
        std::fs::create_dir(&root).expect("fresh consumer namespace; preserve failure evidence");
        std::fs::create_dir(root.join("generated")).unwrap();
        std::fs::create_dir(root.join("consumer")).unwrap();
        for file in artifacts.files() {
            let path = root.join("generated").join(file.path.as_str());
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut reached = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .unwrap();
            std::io::Write::write_all(&mut reached, &file.contents).unwrap();
            reached.sync_all().unwrap();
        }
        let input_original = format!("{program:#?}\n");
        for (name, bytes) in [
            (
                "consumer/PublicConsumer.csproj",
                PUBLIC_BASIC_CONSUMER_PROJECT.as_bytes(),
            ),
            ("consumer/Program.cs", host.as_bytes()),
            ("Program.ir.original.txt", input_original.as_bytes()),
        ] {
            let mut reached = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join(name))
                .unwrap();
            std::io::Write::write_all(&mut reached, bytes).unwrap();
            reached.sync_all().unwrap();
        }
    }
    source
}

fn filter_645(scope: &mut TargetScope, predicate: u32) {
    let input = scope.iteration.as_ref().unwrap().input().clone();
    scope.iteration = Some(IterationPlan::new(
        input,
        Some(predicate),
        None,
        vec![],
        IterationOutput::Repeated,
    ));
}
fn filter_645_program() -> Program {
    let mut root = owned_iteration(
        named("Ref"),
        &["Rows"],
        vec![
            binding("Value", 3, ScalarType::String),
            binding("OwnedPosition", 1, ScalarType::Int),
            binding("LegacyPosition", 4, ScalarType::Int),
        ],
    );
    filter_645(&mut root, 2);
    let mut p = basic(
        vec![
            expression(1, position(named("Ref"), Some(&["Rows"]))),
            expression(
                2,
                public582_map(
                    1,
                    Some(ScalarType::Int),
                    vec![
                        (Value::Int(4), Value::Bool(true)),
                        (Value::Int(5), Value::Bool(true)),
                    ],
                    Some(Value::Bool(false)),
                ),
            ),
            expression(3, field(named("Ref"), &["Rows", "Value"], Some(&["Rows"]))),
            expression(
                4,
                Expression::Position {
                    collection: path(&["Ref", "Rows"]),
                },
            ),
        ],
        root.clone(),
    );
    add_target(&mut p, "Filtered", root);
    p
}
fn filter_645_mapping(artifacts: codegen::ArtifactSet) -> String {
    String::from_utf8(
        artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.cs")
            .unwrap()
            .contents
            .clone(),
    )
    .unwrap()
}

pub(super) fn assert_public_basic_typed_artifacts_match(
    program: &Program,
    with_json: &codegen::ArtifactSet,
    typed: &codegen::ArtifactSet,
) {
    let core = crate::owned_basic::render_public_basic_owned_mapping(program).unwrap();
    let source = filter_645_mapping(with_json.clone());
    assert!(source.starts_with(&core));
    assert!(source.contains("ExecuteJsonOutputsWithBasicOwnedSources("));
    assert_eq!(filter_645_mapping(typed.clone()), core);
    assert_eq!(
        with_json
            .files()
            .iter()
            .filter(|file| file.path.as_str() != "GeneratedMapping.cs")
            .collect::<Vec<_>>(),
        typed
            .files()
            .iter()
            .filter(|file| file.path.as_str() != "GeneratedMapping.cs")
            .collect::<Vec<_>>(),
    );
}

#[test]
fn public_basic_filter_companion_preserves_control_free_bytes_and_closed_routes() {
    let p = filter_645_program();
    let old = crate::emit_with_basic_owned_sources(&p);
    let private = render_basic_owned_mapping(&p);
    let default = codegen::validate_basic_owned_emission(&p);
    let ordinary = codegen::validate_program(&p);
    let catalogue = codegen::legacy_named_input_admission_plan(&p);
    eprintln!(
        "N645 old public={old:#?}; private={private:#?}; basic={default:#?}; ordinary={ordinary:#?}; catalogue={catalogue:#?}; complete input={p:#?}"
    );
    for actual in [old.map(|_| ()), private.map(|_| ())] {
        assert!(
            matches!(actual,Err(EmitError::ProgramValidation(ProgramValidationError::OwnedSource{diagnostic})) if matches!(*diagnostic,codegen::OwnedSourceDiagnostic::Capability{reason:codegen::OwnedSourceCapabilityReason::ScopeControl,..}))
        );
    }
    for actual in [default, ordinary, catalogue.map(|_| ())] {
        assert!(
            matches!(actual,Err(ProgramValidationError::OwnedSource{diagnostic}) if matches!(*diagnostic,codegen::OwnedSourceDiagnostic::Capability{reason:codegen::OwnedSourceCapabilityReason::ScopeControl,..}))
        );
    }
    let plain = basic(
        vec![expression(1, field(SourceOwner::Primary, &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    let old = crate::owned_basic::render_public_basic_owned_mapping(&plain).unwrap();
    let with_json = filter_645_mapping(crate::emit_with_basic_owned_sources(&plain).unwrap());
    let explicit = filter_645_mapping(crate::emit_with_basic_owned_filters(&plain).unwrap());
    assert!(with_json.starts_with(&old));
    assert!(with_json.contains("ExecuteJsonOutputsWithBasicOwnedSources("));
    assert!(!explicit.contains("ExecuteJsonOutputsWithBasicOwnedSources("));
    assert_eq!(old, explicit, "control-free typed mapping bytes are exact");
}

fn filter_681_positive_pairing_program() -> Program {
    let mut program = filter_645_program();
    let paired_position = program
        .expressions
        .iter_mut()
        .find(|node| node.id == 4)
        .unwrap();
    paired_position.expression = Expression::Position {
        collection: path(&[]),
    };
    program
}

#[test]
fn public_basic_filter_streams_raw_predicates_compact_outputs_and_all_selected_targets() {
    // The original qualified-path model stays intact for closed-route and alias probes.
    // This positive pairing subject explicitly asks for the innermost compact cursor.
    let p = filter_681_positive_pairing_program();
    assert_eq!(
        p.expressions
            .iter()
            .find(|node| node.id == 4)
            .unwrap()
            .expression,
        Expression::Position {
            collection: path(&[])
        }
    );
    let source = retain_public_filter_consumer("public-filter-645-phases", &p, PUBLIC645_PHASES);
    assert_eq!(
        source.matches("BeginRetainedFilterPhase(cohort_").count(),
        2
    );
    assert_eq!(
        source.matches("RequireBoolean(Node_2(candidate_").count(),
        2
    );
    assert!(source.contains("phase_0.Produce((nuint)input_0)"));
    assert!(
        source.find("phase_0.Produce").unwrap()
            < source.find("ScopeItem_0(produced_0.Context)").unwrap()
    );
    assert!(!source.contains("WithPhasePosition("));
    assert!(!source.contains("ReindexRetainedPhase("));
    assert!(source.contains("error.WithOwnedSourceNode(3U)"));
}
const PUBLIC645_PHASES: &str = r#"
var input=G(("Value",S("P.root")),("Ref",G(("Rows",R(G(("Value",S("collision"))))))));
var named=G(("Rows",R(new FerruleScalar(FerruleValue.Null),G(("Value",S("N1"))),new FerruleScalar(FerruleValue.JsonNull),G(("Value",S("N2"))),G(("Value",S("N3"))))));
var limits=Policy();var supplied=new[]{new NamedInput("Ref",named)};
var primary=FerruleTargetSelection.Primary();var selected=FerruleTargetSelection.Named("Filtered");
var p0=new Probe(supplied);var a=Reach("N645-F01",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var p1=new Probe(supplied);var b=Reach("N645-F02",input,p1,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p1,limits,null));
var p2=new Probe(supplied);var c=Reach("N645-F03",input,p2,limits,null,selected,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selected,p2,limits,null));
var expected="[{\"Value\":\"N2\",\"OwnedPosition\":1,\"LegacyPosition\":1},{\"Value\":\"N3\",\"OwnedPosition\":2,\"LegacyPosition\":2}]";
Tree(a,expected);Tree(b,expected);Tree(c,expected);
var all=(ExecutionOutputs)a.Result!;Check(all.Extras.Count==1 && all.Extras[0].Name=="Filtered" && Json(Plain(all.Extras[0].Instance))==expected,"complete all-target extra");
Reads(a,1,1);Reads(b,1,1);Reads(c,1,1);
"#;

#[test]
fn public_basic_filter_target_raise_precedes_later_predicate_raise() {
    let mut root = owned_iteration(
        SourceOwner::Primary,
        &["Rows"],
        vec![binding("Value", 6, ScalarType::String)],
    );
    filter_645(&mut root, 5);
    let p = basic(
        vec![
            expression(1, position(SourceOwner::Primary, Some(&["Rows"]))),
            expression(
                2,
                public582_map(
                    1,
                    None,
                    vec![(Value::Int(1), Value::Bool(true))],
                    Some(Value::Bool(false)),
                ),
            ),
            expression(
                3,
                Expression::RuntimeParameter {
                    name: "keep_first".into(),
                    ty: ScalarType::Bool,
                },
            ),
            expression(4, Expression::Raise { message: Some(11) }),
            expression(
                5,
                Expression::If {
                    condition: 2,
                    then: 3,
                    else_: 4,
                },
            ),
            expression(6, Expression::Raise { message: Some(10) }),
            expression(
                10,
                Expression::Const {
                    value: Value::String("target first".into()),
                },
            ),
            expression(
                11,
                Expression::Const {
                    value: Value::String("later predicate".into()),
                },
            ),
        ],
        root,
    );
    retain_public_filter_consumer("public-filter-645-order", &p, PUBLIC645_ORDER);
}
const PUBLIC645_ORDER: &str = r#"
var input=Primary();var limits=Policy();var primary=FerruleTargetSelection.Primary();
var t=new[]{new KeyValuePair<string,FerruleValue>("keep_first",FerruleValue.FromBoolean(true))};var f=new[]{new KeyValuePair<string,FerruleValue>("keep_first",FerruleValue.FromBoolean(false))};
var yes=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(t));var no=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(f));
var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("N645-O01",input,p0,limits,yes,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p0,limits,yes),t);
var p1=new Probe(Array.Empty<NamedInput>());var b=Reach("N645-O02",input,p1,limits,no,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p1,limits,no),f);
var first=Runtime(a,FerruleRuntimeError.MappingException);Check(first.Node==6U && first.MappingExceptionMessage=="target first" && first.Message=="node 6: mapping exception: target first" && first.InnerException is null,"earlier target wins before later predicate");NullMetadata(first,"Node","MappingExceptionMessage");
var later=Runtime(b,FerruleRuntimeError.MappingException);Check(later.Node==4U && later.MappingExceptionMessage=="later predicate" && later.Message=="node 4: mapping exception: later predicate" && later.InnerException is null,"false first target is lazy, later predicate wins");NullMetadata(later,"Node","MappingExceptionMessage");Reads(a,1,0);Reads(b,1,0);
"#;

#[test]
fn public_basic_filter_false_rows_do_not_read_target_and_non_bool_is_typed() {
    let mut root = owned_iteration(
        SourceOwner::Primary,
        &["Rows"],
        vec![binding("Value", 1, ScalarType::String)],
    );
    filter_645(&mut root, 2);
    let p = basic(
        vec![
            expression(1, field(named("Ref"), &["Value"], None)),
            expression(
                2,
                Expression::Const {
                    value: Value::Bool(false),
                },
            ),
        ],
        root.clone(),
    );
    retain_public_filter_consumer(
        "public-filter-645-drop-all",
        &p,
        r#"
var input=Primary();var limits=Policy();var probe=new Probe(Array.Empty<NamedInput>());
var a=Reach("N645-D01",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));Tree(a,"[]");Reads(a,1,0);
"#,
    );
    let p = basic(
        vec![
            expression(1, field(named("Ref"), &["Value"], None)),
            expression(
                2,
                Expression::Const {
                    value: Value::Int(7),
                },
            ),
        ],
        root,
    );
    retain_public_filter_consumer(
        "public-filter-645-non-bool",
        &p,
        r#"
var input=Primary();var limits=Policy();var probe=new Probe(Array.Empty<NamedInput>());
var a=Reach("N645-B01",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));
var e=Runtime(a,FerruleRuntimeError.NotABool);Check(e.Node==2U && e.FoundKind==FerruleValueKind.Int64 && e.Message=="Graph node 2 expected a bool, found int." && e.InnerException is null,"complete typed non-Bool predicate");NullMetadata(e,"Node","FoundKind");Reads(a,1,0);
"#,
    );
}

#[test]
fn public_basic_filter_retains_unsupported_potential_lazy_branch() {
    let mut root = owned_iteration(
        SourceOwner::Primary,
        &["Rows"],
        vec![binding("Value", 1, ScalarType::String)],
    );
    filter_645(&mut root, 5);
    let p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Rows", "Value"], None)),
            expression(
                2,
                Expression::Const {
                    value: Value::Bool(false),
                },
            ),
            expression(
                3,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
            expression(
                5,
                Expression::If {
                    condition: 2,
                    then: 9,
                    else_: 3,
                },
            ),
            expression(9, Expression::SourceDocumentPath),
        ],
        root,
    );
    let result = crate::emit_with_basic_owned_filters(&p);
    eprintln!("N645 potential complete input={p:#?}, full refusal={result:#?}");
    let Err(EmitError::ProgramValidation(ProgramValidationError::OwnedSource { diagnostic })) =
        result
    else {
        panic!("potential node9 XML operator must be refused before rendering");
    };
    assert_eq!(
        *diagnostic,
        codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 9,
                scope: Some(::mapping::SourceScopeLocation::root(
                    ::mapping::SourceScopeTarget::Primary
                )),
            },
            request: None,
            reason: codegen::OwnedSourceCapabilityReason::XmlOperator,
            origin: codegen::OwnedSourceCapabilityOrigin::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 9,
            },
        }
    );
}

#[test]
fn public_basic_filter_companion_keeps_other_controls_closed() {
    let base = filter_645_program();
    for variant in 0..4 {
        let mut p = base.clone();
        let input = p.root.iteration.as_ref().unwrap().input().clone();
        p.expressions.push(expression(
            20,
            Expression::Const {
                value: Value::Int(1),
            },
        ));
        let sort = (variant == 0).then(|| {
            codegen::SortPlan::new(
                codegen::SortKey {
                    expression: 1,
                    descending: false,
                },
                vec![],
                codegen::SortFilterOrder::SortThenFilter,
            )
        });
        let windows = if variant == 1 {
            vec![codegen::SequenceWindow::First { count: 20 }]
        } else {
            vec![]
        };
        let output = match variant {
            2 => IterationOutput::First,
            3 => IterationOutput::MappedSequence,
            _ => IterationOutput::Repeated,
        };
        p.root.iteration = Some(IterationPlan::new(input, Some(2), sort, windows, output));
        if variant == 2 {
            p.root.repeating = false;
            p.target = target_schema(&p.root, "Output");
        } else if variant == 3 {
            let mut child = p.root.clone();
            child.target_field = "Mapped".into();
            child.repeating = false;
            p.root = group(vec![]);
            p.root.children.push(child);
            p.target = target_schema(&p.root, "Output");
        }
        let result = crate::emit_with_basic_owned_filters(&p);
        eprintln!("N645 closed variant={variant}, complete Program={p:#?}, refusal={result:#?}");
        assert!(
            matches!(result,Err(EmitError::ProgramValidation(ProgramValidationError::OwnedSource{diagnostic})) if matches!(*diagnostic,codegen::OwnedSourceDiagnostic::Capability{reason:codegen::OwnedSourceCapabilityReason::ScopeControl,..}))
        );
    }

    // Malformed cardinality/location still wins before Basic capability refusal.
    for output in [IterationOutput::First, IterationOutput::MappedSequence] {
        let mut p = base.clone();
        let input = p.root.iteration.as_ref().unwrap().input().clone();
        p.root.iteration = Some(IterationPlan::new(input, Some(2), None, vec![], output));
        let result = crate::emit_with_basic_owned_filters(&p);
        eprintln!(
            "N645 malformed output={output:?}, complete Program={p:#?}, full refusal={result:#?}"
        );
        assert!(matches!(result,
            Err(EmitError::ProgramValidation(ProgramValidationError::InvalidIterationOutput { target_path, output: actual }))
            if target_path.is_empty() && actual == output));
    }
    // A nonselected named target cannot bring dynamic-document artifacts into
    // the explicit Basic package; all target inventories are checked eagerly.
    let mut p = base.clone();
    p.expressions.push(expression(
        20,
        Expression::Const {
            value: Value::String("unused.xml".into()),
        },
    ));
    let mut dynamic = plain_iteration(
        &["Ref", "Rows"],
        vec![binding("Value", 20, ScalarType::String)],
    );
    dynamic.iteration = Some(IterationPlan::dynamic_documents(path(&["Ref", "Rows"]), 20));
    p.extra_targets[0].target = target_schema(&dynamic, "Filtered");
    p.extra_targets[0].root = dynamic;
    let result = crate::emit_with_basic_owned_filters(&p);
    eprintln!("N645 nonselected dynamic complete Program={p:#?}, full refusal={result:#?}");
    let Err(EmitError::ProgramValidation(ProgramValidationError::OwnedSource { diagnostic })) =
        result
    else {
        panic!("nonselected named dynamic documents must be refused before artifacts");
    };
    let scope = ::mapping::SourceScopeLocation::root(::mapping::SourceScopeTarget::Named {
        index: 0,
        name: "Filtered".into(),
    });
    assert_eq!(
        *diagnostic,
        codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Iteration {
                scope: scope.clone()
            },
            request: None,
            reason: codegen::OwnedSourceCapabilityReason::Document,
            origin: codegen::OwnedSourceCapabilityOrigin::Scope { scope },
        }
    );
}

#[test]
fn public_basic_filter_nested_plain_identity_keeps_outer_phase_and_raw_null_gaps() {
    let mut kids = owned_iteration(
        SourceOwner::Primary,
        &["Rows", "Rows"],
        vec![
            binding("Value", 3, ScalarType::String),
            binding("ParentPosition", 1, ScalarType::Int),
            binding("Position", 2, ScalarType::Int),
        ],
    );
    kids.target_field = "Kids".into();
    filter_645(&mut kids, 4);
    let mut identity = owned_iteration(
        SourceOwner::Primary,
        &["Rows"],
        vec![
            binding("ParentPosition", 1, ScalarType::Int),
            binding("Value", 5, ScalarType::String),
        ],
    );
    identity.target_field = "Identity".into();
    filter_645(&mut identity, 4);
    let mut root = owned_iteration(SourceOwner::Primary, &["Rows"], vec![]);
    root.children = vec![kids, identity];
    filter_645(&mut root, 6);
    let p = basic(
        vec![
            expression(1, position(SourceOwner::Primary, Some(&["Rows"]))),
            expression(2, position(SourceOwner::Primary, Some(&["Rows", "Rows"]))),
            expression(
                3,
                field(SourceOwner::Primary, &["Rows", "Rows", "Value"], None),
            ),
            expression(
                4,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
            expression(5, field(SourceOwner::Primary, &["Rows", "Value"], None)),
            expression(
                6,
                public582_map(
                    1,
                    None,
                    vec![(Value::Int(2), Value::Bool(true))],
                    Some(Value::Bool(false)),
                ),
            ),
        ],
        root,
    );
    retain_public_filter_consumer(
        "public-filter-645-nested",
        &p,
        r#"
// XmlNil is a real scalar candidate in strict owned walks; only Null/JsonNull
// are absent. Preserve this original input and its complete typed traversal.
var input=G(("Rows",R(G(("Value",S("r1")),("Rows",R(G(("Value",S("unused")))))),G(("Value",S("r2")),("Rows",R(new FerruleScalar(FerruleValue.Null),G(("Value",S("K2"))),new FerruleScalar(FerruleValue.XmlNil),G(("Value",S("K3")))))))));
var nullInput=G(("Rows",R(G(("Value",S("r1")),("Rows",R(G(("Value",S("unused")))))),G(("Value",S("r2")),("Rows",R(new FerruleScalar(FerruleValue.Null),G(("Value",S("K2"))),new FerruleScalar(FerruleValue.JsonNull),G(("Value",S("K3")))))))));
var limits=Policy();var probe=new Probe(Array.Empty<NamedInput>());var nullProbe=new Probe(Array.Empty<NamedInput>());
var a=Reach("N645-N01",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));
var b=Reach("N645-N02",nullInput,nullProbe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(nullInput,nullProbe,limits,null));
var outer=Runtime(a,FerruleRuntimeError.OwnedSource);var cause=outer.OwnedSourceFailure;
Check(outer.Node==3U && outer.Message=="owned source access failed" && cause is not null && ReferenceEquals(cause,outer.InnerException),"XmlNil exact node3 typed inner identity");NullMetadata(outer,"Node","OwnedSourceFailure");
var c=cause!;Check(c.Error==FerruleOwnedSourceErrorKind.Traversal && c.Message=="Owned source operation failed." && c.Owner?.IsPrimary==true && c.Owner?.Name is null && c.RequestedName is null && c.Path!.SequenceEqual(new[]{"Rows","Rows","Value"}) && c.Segment==2 && c.FoundInstance==FerruleOwnedSourceInstanceKind.Scalar,"XmlNil complete primary traversal metadata");
Check(c.DeclarationIndices is null && c.Frame is null && c.Collection is null && c.RequestedIndex is null && c.LimitKind is null && c.Used is null && c.Requested is null && c.Maximum is null,"XmlNil absent owned metadata");
Check(c.InnerException is FerruleOwnedSourceTraversalException,"XmlNil exact nested traversal type");var t=(FerruleOwnedSourceTraversalException)c.InnerException!;
Check(t.Error==FerruleOwnedSourceTraversalKind.CannotTraverse && t.Message=="Owned source traversal failed." && t.Path.SequenceEqual(new[]{"Rows","Rows","Value"}) && t.Segment==2 && t.Found==FerruleOwnedSourceInstanceKind.Scalar && t.InnerException is null,"XmlNil complete traversal cause");
Tree(b,"[{\"Kids\":[{\"Value\":\"K2\",\"ParentPosition\":1,\"Position\":1},{\"Value\":\"K3\",\"ParentPosition\":1,\"Position\":2}],\"Identity\":[{\"ParentPosition\":1,\"Value\":\"r2\"}]}]");Reads(a,1,0);Reads(b,1,0);
"#,
    );
}

#[test]
fn public_basic_filter_predicate_owned_error_keeps_leaf_node_identity_and_full_owner() {
    let mut root = owned_iteration(
        SourceOwner::Primary,
        &["Rows"],
        vec![binding("Value", 1, ScalarType::String)],
    );
    filter_645(&mut root, 2);
    let p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Rows", "Value"], None)),
            expression(2, field(named("Ref"), &["Value"], None)),
        ],
        root,
    );
    retain_public_filter_consumer(
        "public-filter-645-leaf",
        &p,
        r#"
var input=Primary();var limits=Policy();var probe=new Probe(Array.Empty<NamedInput>());
var a=Reach("N645-L01",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));
OwnedAbsent(a,"Ref",2U);Reads(a,1,0);
"#,
    );
}

#[test]
fn public_basic_filter_plain_named_walk_keeps_raw_predicates_and_compact_paired_outputs() {
    let mut p = filter_645_program();
    p.root.iteration = Some(IterationPlan::new(
        codegen::SourceIteration::new(path(&["Ref", "Rows"])),
        Some(2),
        None,
        vec![],
        IterationOutput::Repeated,
    ));
    p.extra_targets[0].root.iteration = p.root.iteration.clone();
    // Legacy selection prefers the real primary Ref collision. It cannot
    // borrow NamedRef ownership from the path spelling.
    let source =
        retain_public_filter_consumer("public-filter-645-plain", &p, PUBLIC645_PLAIN_COLLISION);
    let positive = filter_645_plain_success_program();
    let positive_source = retain_public_filter_consumer(
        "public-filter-645-plain-success",
        &positive,
        PUBLIC645_PLAIN_SUCCESS,
    );
    assert_eq!(
        positive_source
            .matches("WalkSourceCohort(new string[] { \"Ref\", \"Rows\" })")
            .count(),
        2
    );
    assert!(!positive_source.contains("WalkOwnedSourceCohort("));
    assert!(positive_source.contains("ResolveOwnedFieldWithRootPin"));
    assert_eq!(
        source
            .matches("WalkSourceCohort(new string[] { \"Ref\", \"Rows\" })")
            .count(),
        2
    );
    assert!(!source.contains("WalkOwnedSourceCohort("));
    assert_eq!(
        source.matches("BeginRetainedFilterPhase(cohort_").count(),
        2
    );
    assert_eq!(
        source.matches("RequireBoolean(Node_2(candidate_").count(),
        2
    );
}

fn filter_645_plain_success_program() -> Program {
    let mut root = plain_iteration(
        &["Ref", "Rows"],
        vec![
            binding("Value", 3, ScalarType::String),
            binding("Position", 1, ScalarType::Int),
            binding("RootValue", 5, ScalarType::String),
        ],
    );
    filter_645(&mut root, 2);
    let mut p = basic(
        vec![
            expression(
                1,
                Expression::Position {
                    collection: path(&["Ref", "Rows"]),
                },
            ),
            expression(
                2,
                public582_map(
                    1,
                    Some(ScalarType::Int),
                    vec![
                        (Value::Int(4), Value::Bool(true)),
                        (Value::Int(5), Value::Bool(true)),
                    ],
                    Some(Value::Bool(false)),
                ),
            ),
            expression(
                3,
                Expression::SourceField {
                    path: path(&["Ref", "Rows", "Value"]),
                    frame: None,
                },
            ),
            expression(5, field(SourceOwner::Primary, &["Value"], Some(&[]))),
        ],
        root.clone(),
    );
    add_target(&mut p, "Filtered", root);
    p
}

const PUBLIC645_PLAIN_COLLISION: &str = r#"
var input=G(("Value",S("P.root")),("Ref",G(("Rows",R(G(("Value",S("collision"))))))));
var named=G(("Rows",R(new FerruleScalar(FerruleValue.Null),G(("Value",S("N1"))),new FerruleScalar(FerruleValue.JsonNull),G(("Value",S("N2"))),G(("Value",S("N3"))))));
var limits=Policy();var supplied=new[]{new NamedInput("Ref",named)};var primary=FerruleTargetSelection.Primary();var selected=FerruleTargetSelection.Named("Filtered");
var p0=new Probe(supplied);var a=Reach("N645-P01",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var p1=new Probe(supplied);var b=Reach("N645-P02",input,p1,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p1,limits,null));
var p2=new Probe(supplied);var c=Reach("N645-P03",input,p2,limits,null,selected,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selected,p2,limits,null));
foreach(var actual in new[]{a,b,c}) {
    var outer=Runtime(actual,FerruleRuntimeError.OwnedSource);var cause=outer.OwnedSourceFailure;
    Check(outer.Node==1U && outer.Message=="owned source access failed" && cause is not null && ReferenceEquals(cause,outer.InnerException),"plain primary collision cannot invent named frame");NullMetadata(outer,"Node","OwnedSourceFailure");
    var e=cause!;Check(e.Error==FerruleOwnedSourceErrorKind.InactiveSourceFrame && e.Message=="Owned source operation failed." && e.Owner?.IsPrimary==false && e.Owner?.Name=="Ref" && e.RequestedName=="Ref" && e.Collection!.SequenceEqual(new[]{"Rows"}),"complete Ref Rows inactive frame");
    Check(e.DeclarationIndices is null && e.Path is null && e.Frame is null && e.Segment is null && e.FoundInstance is null && e.RequestedIndex is null && e.LimitKind is null && e.Used is null && e.Requested is null && e.Maximum is null && e.InnerException is null,"complete absent inactive-frame metadata");Reads(actual,1,1);
}
"#;

const PUBLIC645_PLAIN_SUCCESS: &str = r#"
// The plain cohort retains all five raw rows. False raw1/2/3 predicates keep
// both null occurrences and the unselected group from evaluating their targets.
var input=G(("Value",S("P.root")),("Ref",G(("Rows",R(new FerruleScalar(FerruleValue.Null),G(("Value",S("P1"))),new FerruleScalar(FerruleValue.JsonNull),G(("Value",S("P2"))),G(("Value",S("P3"))))))));
var named=G(("Rows",R(G(("Value",S("N1"))),G(("Value",S("N2"))),G(("Value",S("N3"))))));
var limits=Policy();var supplied=new[]{new NamedInput("Ref",named)};var primary=FerruleTargetSelection.Primary();var selected=FerruleTargetSelection.Named("Filtered");
var p0=new Probe(supplied);var a=Reach("N645-P04",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var p1=new Probe(supplied);var b=Reach("N645-P05",input,p1,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p1,limits,null));
var p2=new Probe(supplied);var c=Reach("N645-P06",input,p2,limits,null,selected,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selected,p2,limits,null));
var expected="[{\"Value\":\"P2\",\"Position\":1,\"RootValue\":\"P.root\"},{\"Value\":\"P3\",\"Position\":2,\"RootValue\":\"P.root\"}]";
Tree(a,expected);Tree(b,expected);Tree(c,expected);
var all=(ExecutionOutputs)a.Result!;Check(all.Extras.Count==1 && all.Extras[0].Name=="Filtered" && Json(Plain(all.Extras[0].Instance))==expected,"complete plain all-target extra");Reads(a,1,1);Reads(b,1,1);Reads(c,1,1);
"#;

// Public sort companion (#669): filter/sort phase order, stable multi-key
// ordering, sorted positions and typed errors. Each test retains the generated
// source and a complete consumer; other emitters' output stays unchanged.
