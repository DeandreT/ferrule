//! Exact-owner frames for recursive filtering of an owned selector.
use super::*;
use crate::{
    BasicSourceRoots, DeclaredNamedSourceRoot, OwnedSourceError, OwnedSourceLimits, field, group,
    integer, repeated, scalar, string,
};

const LOOKUP: SourceOwnerRef<'static> = SourceOwnerRef::Named("Lookup");

fn file(name: &str, pos: i64, dirpos: i64, dir: &str) -> Instance {
    group([
        field("name", scalar(string(name))),
        field("pos", scalar(integer(pos))),
        field("dirpos", scalar(integer(dirpos))),
        field("dir", scalar(string(dir))),
    ])
}

fn directory(name: &str, files: Vec<Instance>, children: Vec<Instance>) -> Instance {
    group([
        field("name", scalar(string(name))),
        field("file", repeated(files)),
        field("Dir", repeated(children)),
    ])
}

fn input(suffix: &str, directories: Vec<Instance>) -> Instance {
    group([
        field("Suffix", scalar(string(suffix))),
        field("Dir", repeated(directories)),
    ])
}

fn limits() -> OwnedSourceLimits {
    OwnedSourceLimits {
        max_roots: 8,
        max_active_frames: 8,
        max_path_segments: 300,
        max_metadata_bytes: 1 << 20,
        max_walk_contexts: 64,
    }
}

fn names(instance: &Instance, collection: &str) -> Vec<String> {
    instance
        .field(collection)
        .and_then(Instance::as_repeated)
        .unwrap_or_default()
        .iter()
        .filter_map(
            |item| match item.field("name").and_then(Instance::as_scalar) {
                Some(Value::String(name)) => Some(name.clone()),
                _ => None,
            },
        )
        .collect()
}

/// Keep Lookup files whose name ends with the Primary endpoint's suffix.
fn primary_suffix(context: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
    let Value::String(name) = context.owned_field(LOOKUP, &["Dir", "file", "name"], None)? else {
        return Ok(Value::Bool(false));
    };
    let Value::String(suffix) = context.owned_field(SourceOwnerRef::Primary, &["Suffix"], None)?
    else {
        return Ok(Value::Bool(false));
    };
    Ok(Value::Bool(name.ends_with(&suffix)))
}

/// Item and group positions plus the innermost group's name must match.
fn positions(context: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
    let item = context.owned_position(LOOKUP, Some(&["Dir", "file"]))?;
    let group = context.owned_position(LOOKUP, Some(&["Dir"]))?;
    let latest = context.owned_position(LOOKUP, None)?;
    let expected = |path: &[&str]| context.owned_field(LOOKUP, path, None);
    Ok(Value::Bool(
        expected(&["Dir", "file", "pos"])? == Value::Int(item as i64)
            && expected(&["Dir", "file", "dirpos"])? == Value::Int(group as i64)
            && expected(&["Dir", "name"])? == expected(&["Dir", "file", "dir"])?
            && latest == item,
    ))
}

fn filter_each(
    context: &BasicScopeContext<'_>,
    predicate: OwnedRecursiveFilterPredicate,
) -> Result<Vec<Instance>, RuntimeError> {
    let cohort = context.walk_owned_source(LOOKUP, &["Dir"])?;
    cohort
        .iter()
        .map(|candidate| {
            owned_recursive_filter(
                candidate.context(),
                LOOKUP,
                &["Dir"],
                "Dir",
                "file",
                9,
                predicate,
            )
        })
        .collect()
}

#[test]
fn owned_filter_reads_the_selected_owner_and_its_recursive_positions() {
    let primary = input(".keep", vec![directory("p", Vec::new(), Vec::new())]);
    let lookup = input(
        ".lk",
        vec![
            directory(
                "a",
                vec![
                    file("x.keep", 1, 1, "a"),
                    file("y.lk", 9, 1, "a"),
                    file("z.keep", 3, 1, "a"),
                ],
                vec![
                    directory(
                        "b",
                        vec![file("b.keep", 1, 1, "b"), file("c.keep", 2, 9, "b")],
                        vec![directory("c", vec![file("d.keep", 1, 1, "a")], Vec::new())],
                    ),
                    directory("d", vec![file("e.keep", 1, 2, "d")], Vec::new()),
                ],
            ),
            directory("e", vec![file("f.keep", 1, 2, "e")], Vec::new()),
        ],
    );
    let declared = [DeclaredNamedSourceRoot {
        name: "Lookup",
        instance: Some(&lookup),
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declared, limits()).unwrap();
    let context = roots.root_context().unwrap();

    let suffixed = filter_each(&context, primary_suffix).unwrap();
    assert_eq!(names(&suffixed[0], "file"), ["x.keep", "z.keep"]);
    let nested = suffixed[0]
        .field("Dir")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(names(&nested[0], "file"), ["b.keep", "c.keep"]);
    assert_eq!(names(&nested[1], "file"), ["e.keep"]);
    assert_eq!(names(&suffixed[1], "file"), ["f.keep"]);

    let positioned = filter_each(&context, positions).unwrap();
    assert_eq!(names(&positioned[0], "file"), ["x.keep", "z.keep"]);
    let nested = positioned[0]
        .field("Dir")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(names(&nested[0], "file"), ["b.keep"]);
    let deepest = nested[0]
        .field("Dir")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert!(names(&deepest[0], "file").is_empty());
    assert_eq!(names(&nested[1], "file"), ["e.keep"]);
    assert_eq!(names(&positioned[1], "file"), ["f.keep"]);

    // The lookup filter never supplies the Primary groups, and an unsupplied
    // owner is a typed owned failure.
    let missing = [DeclaredNamedSourceRoot {
        name: "Lookup",
        instance: None,
    }];
    let roots = BasicSourceRoots::try_new(&primary, &missing, limits()).unwrap();
    assert_eq!(
        owned_recursive_filter(
            &roots.root_context().unwrap(),
            LOOKUP,
            &[],
            "Dir",
            "file",
            9,
            primary_suffix,
        ),
        Err(RuntimeError::OwnedSource(
            OwnedSourceError::SourceUnavailable {
                name: "Lookup".into()
            }
        ))
    );
}

#[test]
fn identity_roots_inactive_collections_and_legacy_errors() {
    fn keep(_: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
        Ok(Value::Bool(true))
    }
    fn text(_: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
        Ok(string("no"))
    }
    // A named Group root is one identity group without an occurrence.
    let tree = directory("t", vec![file("t.keep", 1, 1, "t")], Vec::new());
    let primary = input("", Vec::new());
    let declared = [DeclaredNamedSourceRoot {
        name: "Lookup",
        instance: Some(&tree),
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declared, limits()).unwrap();
    let context = roots.root_context().unwrap();
    assert_eq!(
        owned_recursive_filter(&context, LOOKUP, &[], "Dir", "file", 9, keep),
        Ok(tree.clone())
    );
    // Without an active occurrence of the collection there is no group.
    assert_eq!(
        owned_recursive_filter(&context, LOOKUP, &["Dir"], "Dir", "file", 9, keep),
        Err(RuntimeError::RecursiveFilterRequiresGroup {
            found: "missing context"
        })
    );
    assert_eq!(
        owned_recursive_filter(&context, LOOKUP, &[], "Dir", "file", 9, text),
        Err(RuntimeError::NotABool {
            node: 9,
            found: "string"
        })
    );
    let malformed = group([field("file", scalar(string("x")))]);
    let declared = [DeclaredNamedSourceRoot {
        name: "Lookup",
        instance: Some(&malformed),
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declared, limits()).unwrap();
    assert_eq!(
        owned_recursive_filter(
            &roots.root_context().unwrap(),
            LOOKUP,
            &[],
            "Dir",
            "file",
            9,
            keep
        ),
        Err(RuntimeError::RecursiveFilterRequiresCollection {
            field: "file".into(),
            found: "scalar"
        })
    );

    let nest = |levels: usize| {
        let mut directory = directory("leaf", Vec::new(), Vec::new());
        for _ in 0..levels {
            directory = self::directory("n", Vec::new(), vec![directory]);
        }
        directory
    };
    for (levels, expected) in [
        (MAX_RECURSIVE_SEQUENCE_DEPTH - 1, None),
        (
            MAX_RECURSIVE_SEQUENCE_DEPTH,
            Some(RuntimeError::RecursiveFilterDepth {
                limit: MAX_RECURSIVE_SEQUENCE_DEPTH,
            }),
        ),
    ] {
        let deep = nest(levels);
        let declared = [DeclaredNamedSourceRoot {
            name: "Lookup",
            instance: Some(&deep),
        }];
        let roots = BasicSourceRoots::try_new(&primary, &declared, limits()).unwrap();
        let result = owned_recursive_filter(
            &roots.root_context().unwrap(),
            LOOKUP,
            &[],
            "Dir",
            "file",
            9,
            keep,
        );
        match expected {
            None => assert_eq!(result, Ok(deep.clone())),
            Some(error) => assert_eq!(result, Err(error)),
        }
    }
}
