//! Owned join identities validate against their own endpoints; execution is
//! covered by `owned_join_tuples`.
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Graph, JoinId, NamedSource, OwnedJoinConditions, OwnedJoinKey, OwnedJoinPlan, OwnedJoinSource,
    OwnedSourceRef, Project, Scope, ScopeIteration, SourceOwner,
};

use crate::{run_with_sources, validate};

fn rows_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group("Rows", vec![SchemaNode::scalar("id", ScalarType::Int)]).repeating(),
            SchemaNode::group(
                "Header",
                vec![SchemaNode::scalar("code", ScalarType::String)],
            ),
        ],
    )
}

fn owned(owner: SourceOwner, path: &[&str]) -> OwnedSourceRef {
    OwnedSourceRef {
        owner,
        path: path.iter().map(|segment| (*segment).to_owned()).collect(),
    }
}

fn named(name: &str, path: &[&str]) -> OwnedSourceRef {
    owned(SourceOwner::Named { name: name.into() }, path)
}

fn primary(path: &[&str]) -> OwnedSourceRef {
    owned(SourceOwner::Primary, path)
}

fn conditions(left: OwnedSourceRef, left_path: &str, right_path: &str) -> OwnedJoinConditions {
    OwnedJoinConditions::new(OwnedJoinKey::new(
        left,
        vec![left_path.into()],
        vec![right_path.into()],
    ))
}

fn plan(right: OwnedSourceRef, right_path: &str) -> OwnedJoinPlan {
    OwnedJoinPlan::new(
        OwnedJoinSource::new(primary(&["Rows"])),
        OwnedJoinSource::new(right),
        conditions(primary(&["Rows"]), "id", right_path),
    )
    .unwrap()
}

fn project(plan: OwnedJoinPlan) -> Project {
    Project {
        source: rows_schema(),
        target: SchemaNode::group("Output", vec![SchemaNode::group("Row", vec![]).repeating()]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "lookup".into(),
            path: "lookup.json".into(),
            schema: rows_schema(),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::OwnedInnerJoin {
                    id: JoinId::new(3),
                    plan,
                },
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn messages(project: &Project) -> Vec<String> {
    validate(project)
        .into_iter()
        .map(|issue| issue.message)
        .collect()
}

#[test]
fn colliding_primary_and_named_rows_validate_as_two_sources() {
    assert_eq!(
        messages(&project(plan(named("lookup", &["Rows"]), "id"))),
        Vec::<String>::new()
    );
}

#[test]
fn validation_checks_each_owner_collection_and_key() {
    assert_eq!(
        messages(&project(plan(named("missing", &["Rows"]), "id"))),
        ["unknown source owner `missing`"]
    );
    assert_eq!(
        messages(&project(plan(named("lookup", &["Header"]), "code"))),
        ["join 3 collection named `lookup` `Header` is missing or not repeating"]
    );
    assert_eq!(
        messages(&project(plan(named("lookup", &["Rows"]), "code"))),
        ["join right key `code` is not a scalar under collection named `lookup` `Rows`"]
    );
    let singleton = OwnedJoinPlan::new(
        OwnedJoinSource::new(primary(&["Rows"])),
        OwnedJoinSource::singleton(named("lookup", &["Rows", "id"])),
        conditions(primary(&["Rows"]), "id", ""),
    )
    .unwrap();
    let issues = messages(&project(singleton));
    assert!(
        issues.contains(
            &"join 3 singleton collection named `lookup` `Rows/id` crosses a repeating source level; join the repeating collection with a key path instead".to_string()
        ),
        "{issues:?}"
    );
}

#[test]
fn execution_admits_owned_joins_and_keeps_the_owner_of_each_row() {
    let row =
        |id| Instance::Group(vec![("id".to_owned(), Instance::Scalar(Value::Int(id)))].into());
    let header = Instance::Group(
        vec![(
            "code".to_owned(),
            Instance::Scalar(Value::String("H".into())),
        )]
        .into(),
    );
    let input = |ids: &[i64]| {
        Instance::Group(
            vec![
                (
                    "Rows".to_owned(),
                    Instance::Repeated(ids.iter().copied().map(row).collect()),
                ),
                ("Header".to_owned(), header.clone()),
            ]
            .into(),
        )
    };
    // Two primary rows and one matching named row: one tuple.
    let output = run_with_sources(
        &project(plan(named("lookup", &["Rows"]), "id")),
        &input(&[1, 2]),
        vec![("lookup".into(), input(&[2]))],
    )
    .unwrap();
    assert_eq!(
        output,
        Instance::Group(
            vec![(
                "Row".to_owned(),
                Instance::Repeated(vec![Instance::Group(Default::default())])
            )]
            .into()
        )
    );
}
