//! Exact-owner root selection for recursive collection walks.
use super::*;
use crate::{
    BasicScopeContext, BasicSourceRoots, DeclaredNamedSourceRoot, Instance, OwnedSourceError,
    OwnedSourceLimits, SourceOwnerRef, field, group, repeated, scalar,
};

fn text(value: &str) -> Instance {
    scalar(Value::String(value.into()))
}
fn directory(note: &str, files: &[&str], children: Vec<Instance>) -> Instance {
    group([
        field("Note", text(note)),
        field(
            "file",
            repeated(files.iter().map(|file| group([field("Note", text(file))]))),
        ),
        field("Dir", repeated(children)),
    ])
}
fn forest(note: &str, directories: Vec<Instance>) -> Instance {
    group([
        field("Note", text(note)),
        field("Dir", repeated(directories)),
    ])
}
fn limits() -> OwnedSourceLimits {
    OwnedSourceLimits {
        max_roots: 8,
        max_active_frames: 8,
        max_path_segments: 16,
        max_metadata_bytes: 65_536,
        max_walk_contexts: 64,
    }
}
fn collect(
    context: &BasicScopeContext<'_>,
    owner: SourceOwnerRef<'_>,
    collection: &[&str],
) -> Result<Vec<Value>, RuntimeError> {
    owned_recursive_collect(
        context,
        owner,
        RecursiveCollectPaths {
            collection,
            children: &["Dir"],
            descent_value: &["Note"],
            values: &["file"],
            value: &["Note"],
        },
        "",
        "/",
    )
}
fn strings(values: &[&str]) -> Vec<Value> {
    values
        .iter()
        .map(|value| Value::String((*value).into()))
        .collect()
}

#[test]
fn owned_roots_come_only_from_the_selected_endpoint() {
    let primary = forest(
        "P.note",
        vec![directory(
            "p",
            &["a"],
            vec![directory("sub", &["b"], Vec::new())],
        )],
    );
    let lookup = forest("N.note", vec![directory("n", &["x", "y"], Vec::new())]);
    let tree = directory("t", &["root.txt"], Vec::new());
    let empty = group([field("Note", text("E.note"))]);
    let declared = [
        DeclaredNamedSourceRoot {
            name: "Lookup",
            instance: Some(&lookup),
        },
        DeclaredNamedSourceRoot {
            name: "Tree",
            instance: Some(&tree),
        },
        DeclaredNamedSourceRoot {
            name: "Empty",
            instance: Some(&empty),
        },
        DeclaredNamedSourceRoot {
            name: "Missing",
            instance: None,
        },
    ];
    let roots = BasicSourceRoots::try_new(&primary, &declared, limits()).unwrap();
    let context = roots.root_context().unwrap();
    assert_eq!(
        collect(&context, SourceOwnerRef::Primary, &["Dir"]),
        Ok(strings(&["/p/a", "/p/sub/b"]))
    );
    assert_eq!(
        collect(&context, SourceOwnerRef::Named("Lookup"), &["Dir"]),
        Ok(strings(&["/n/x", "/n/y"]))
    );
    // A fresh named Group root is one identity root; relative Note paths read
    // the visited groups, never an endpoint field.
    assert_eq!(
        collect(&context, SourceOwnerRef::Named("Tree"), &[]),
        Ok(strings(&["/t/root.txt"]))
    );
    // An absent selected collection never borrows the Primary tree.
    assert_eq!(
        collect(&context, SourceOwnerRef::Named("Empty"), &["Dir"]),
        Ok(Vec::new())
    );
    assert_eq!(
        collect(&context, SourceOwnerRef::Named("Missing"), &["Dir"]),
        Err(RuntimeError::OwnedSource(
            OwnedSourceError::SourceUnavailable {
                name: "Missing".into()
            }
        ))
    );
    assert!(matches!(
        collect(&context, SourceOwnerRef::Named("Missing"), &["Dir"])
            .map_err(|error| error.with_owned_source_node(7)),
        Err(RuntimeError::OwnedSourceAtNode { node: 7, .. })
    ));
}

#[test]
fn owned_walks_keep_typed_leaf_and_depth_errors() {
    let null_leaf = forest(
        "P",
        vec![group([
            field("Note", text("d")),
            field(
                "file",
                repeated([group([field("Note", scalar(Value::Null))])]),
            ),
        ])],
    );
    let roots = BasicSourceRoots::try_new(&null_leaf, &[], limits()).unwrap();
    assert_eq!(
        collect(
            &roots.root_context().unwrap(),
            SourceOwnerRef::Primary,
            &["Dir"]
        ),
        Err(RuntimeError::Function(FunctionError::TypeMismatch {
            function: "recursive-collect",
            got: "null",
        }))
    );
    let mut deepest = directory("leaf", &[], Vec::new());
    for depth in 0..MAX_RECURSIVE_SEQUENCE_DEPTH {
        deepest = directory(&format!("level-{depth}"), &[], vec![deepest]);
    }
    let deep = forest("P", vec![deepest]);
    let roots = BasicSourceRoots::try_new(&deep, &[], limits()).unwrap();
    assert_eq!(
        collect(
            &roots.root_context().unwrap(),
            SourceOwnerRef::Primary,
            &["Dir"]
        ),
        Err(RuntimeError::RecursiveSequenceDepth {
            limit: MAX_RECURSIVE_SEQUENCE_DEPTH,
        })
    );
}
