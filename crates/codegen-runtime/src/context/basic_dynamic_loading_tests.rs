//! Owned dynamic driver loading: exact drivers, document roots and callbacks.
use std::cell::RefCell;

use crate::{
    BasicScopeContext, BasicSourceRoots, DeclaredNamedSourceRoot, DynamicSourceLoader, Instance,
    OwnedSourceError, OwnedSourceLimits, RuntimeError, SourceOwnerRef, Value, field, group,
    repeated, scalar,
};

fn text(value: &str) -> Value {
    Value::String(value.into())
}
fn batch(rows: &[(Value, &str)]) -> Instance {
    group([field(
        "Files",
        repeated(rows.iter().map(|(path, label)| {
            group([
                field("Path", scalar(path.clone())),
                field("Label", scalar(text(label))),
            ])
        })),
    )])
}
fn doc(title: &str, lines: &[&str], parts: &[&str]) -> Instance {
    group([
        field("Title", scalar(text(title))),
        field(
            "Lines",
            repeated(
                lines
                    .iter()
                    .map(|code| group([field("Code", scalar(text(code)))])),
            ),
        ),
        field(
            "Parts",
            repeated(
                parts
                    .iter()
                    .map(|path| group([field("Path", scalar(text(path)))])),
            ),
        ),
    ])
}
fn limits() -> OwnedSourceLimits {
    OwnedSourceLimits {
        max_roots: 8,
        max_active_frames: 16,
        max_path_segments: 16,
        max_metadata_bytes: 65_536,
        max_walk_contexts: 64,
    }
}

#[derive(Default)]
struct Loader {
    calls: RefCell<Vec<String>>,
}
impl DynamicSourceLoader for Loader {
    fn load(&self, source: &str, path: &str) -> Result<Instance, String> {
        self.calls.borrow_mut().push(format!("{source}:{path}"));
        Ok(match (source, path) {
            ("Docs", "a.json") => doc("A", &["a1", "a2"], &["p1.json"]),
            ("Docs", "b.json") => doc("B", &["b1"], &["p1.json", "p2.json"]),
            ("Docs", "c.json") => doc("C", &[], &[]),
            ("Parts", "p1.json") => group([field("Name", scalar(text("P1")))]),
            ("Parts", "p2.json") => group([field("Name", scalar(text("P2")))]),
            _ => return Err(format!("no document {source}:{path}")),
        })
    }
}

const DOCS: SourceOwnerRef<'static> = SourceOwnerRef::Named("Docs");
const PARTS: SourceOwnerRef<'static> = SourceOwnerRef::Named("Parts");

fn docs(
    context: &BasicScopeContext<'_>,
    driver: SourceOwnerRef<'static>,
) -> Result<Vec<Vec<Value>>, RuntimeError> {
    let documents = context.load_owned_dynamic_documents("Docs", driver, &["Files"], 1, |row| {
        row.owned_field(driver, &["Files", "Path"], Some(&["Files"]))
    })?;
    let loading = documents.loading_context(context)?;
    let cohort = loading.walk_owned_source_cohort(DOCS, &[])?;
    let mut rows = Vec::new();
    for candidate in cohort.iter() {
        let item = candidate.context();
        let mut row = vec![
            item.owned_field(DOCS, &["Title"], None)?,
            item.owned_field(driver, &["Files", "Label"], Some(&["Files"]))?,
            Value::Int(item.owned_position(driver, Some(&["Files"]))? as i64),
        ];
        for line in item.walk_owned_source_cohort(DOCS, &["Lines"])?.iter() {
            let line = line.context();
            row.push(line.owned_field(DOCS, &["Lines", "Code"], Some(&["Lines"]))?);
            row.push(Value::Int(
                line.owned_position(DOCS, Some(&["Lines"]))? as i64
            ));
        }
        // Nested loading reads only this document's Parts rows.
        let parts = item.load_owned_dynamic_documents("Parts", DOCS, &["Parts"], 2, |part| {
            part.owned_field(DOCS, &["Parts", "Path"], Some(&["Parts"]))
        })?;
        let nested = parts.loading_context(item)?;
        for part in nested.walk_owned_source_cohort(PARTS, &[])?.iter() {
            let part = part.context();
            row.push(part.owned_field(PARTS, &["Name"], None)?);
            row.push(part.owned_field_with_root_pin(DOCS, &["Title"], Some(&[]), true)?);
            row.push(part.owned_field(driver, &["Files", "Label"], Some(&["Files"]))?);
        }
        rows.push(row);
    }
    Ok(rows)
}

fn roots<'a>(
    primary: &'a Instance,
    declared: &'a [DeclaredNamedSourceRoot<'a>],
) -> BasicSourceRoots<'a> {
    BasicSourceRoots::try_new(primary, declared, limits()).unwrap()
}

#[test]
fn loaded_documents_keep_their_driver_and_root_through_nested_loading() {
    let primary = batch(&[(text("a.json"), "first"), (text("b.json"), "second")]);
    let catalog = batch(&[(text("c.json"), "catalog")]);
    let declared = [
        DeclaredNamedSourceRoot {
            name: "Catalog",
            instance: Some(&catalog),
        },
        DeclaredNamedSourceRoot {
            name: "Docs",
            instance: None,
        },
        DeclaredNamedSourceRoot {
            name: "Parts",
            instance: None,
        },
    ];
    let roots = roots(&primary, &declared);
    let loader = Loader::default();
    let context = roots
        .root_context_with_dynamic_source_loader(None, &loader)
        .unwrap();
    assert_eq!(
        docs(&context, SourceOwnerRef::Primary).unwrap(),
        [
            vec![
                text("A"),
                text("first"),
                Value::Int(1),
                text("a1"),
                Value::Int(1),
                text("a2"),
                Value::Int(2),
                text("P1"),
                text("A"),
                text("first"),
            ],
            vec![
                text("B"),
                text("second"),
                Value::Int(2),
                text("b1"),
                Value::Int(1),
                text("P1"),
                text("B"),
                text("second"),
                text("P2"),
                text("B"),
                text("second"),
            ],
        ]
    );
    // Every reached path is one callback; generated code never caches.
    assert_eq!(
        *loader.calls.borrow(),
        [
            "Docs:a.json",
            "Docs:b.json",
            "Parts:p1.json",
            "Parts:p1.json",
            "Parts:p2.json"
        ]
    );
    // Outside its loading scope the dynamic owner has no root.
    assert_eq!(
        context.owned_field(DOCS, &["Title"], None),
        Err(RuntimeError::OwnedSource(
            OwnedSourceError::SourceUnavailable {
                name: "Docs".into()
            }
        ))
    );

    // The same-shaped named driver supplies only its own rows.
    let loader = Loader::default();
    let context = roots
        .root_context_with_dynamic_source_loader(None, &loader)
        .unwrap();
    assert_eq!(
        docs(&context, SourceOwnerRef::Named("Catalog")).unwrap(),
        [vec![text("C"), text("catalog"), Value::Int(1)]]
    );
    assert_eq!(*loader.calls.borrow(), ["Docs:c.json"]);
}

#[test]
fn paths_drivers_and_hosts_keep_their_typed_failures() {
    let declared_without = |catalog: Option<&'static Instance>| {
        [
            DeclaredNamedSourceRoot {
                name: "Catalog",
                instance: catalog,
            },
            DeclaredNamedSourceRoot {
                name: "Docs",
                instance: None,
            },
            DeclaredNamedSourceRoot {
                name: "Parts",
                instance: None,
            },
        ]
    };
    let declared = declared_without(None);
    // A missing loader fails before the driver walk, even when it is empty.
    let empty = batch(&[]);
    let roots_empty = roots(&empty, &declared);
    assert_eq!(
        docs(
            &roots_empty.root_context().unwrap(),
            SourceOwnerRef::Primary
        ),
        Err(RuntimeError::MissingDynamicSourceLoader { source: "Docs" })
    );
    // An unsupplied driver endpoint is never repaired by Primary's rows.
    let primary = batch(&[(text("a.json"), "first")]);
    let roots_unsupplied = roots(&primary, &declared);
    let loader = Loader::default();
    assert_eq!(
        docs(
            &roots_unsupplied
                .root_context_with_dynamic_source_loader(None, &loader)
                .unwrap(),
            SourceOwnerRef::Named("Catalog"),
        ),
        Err(RuntimeError::OwnedSource(
            OwnedSourceError::SourceUnavailable {
                name: "Catalog".into()
            }
        ))
    );
    assert!(loader.calls.borrow().is_empty());
    // Absent and null paths skip; another value stops before later drivers.
    let mixed = batch(&[
        (Value::Null, "skip"),
        (Value::json_null(), "skip"),
        (text("c.json"), "kept"),
        (Value::Int(7), "bad"),
        (text("a.json"), "late"),
    ]);
    let roots_mixed = roots(&mixed, &declared);
    let loader = Loader::default();
    assert_eq!(
        docs(
            &roots_mixed
                .root_context_with_dynamic_source_loader(None, &loader)
                .unwrap(),
            SourceOwnerRef::Primary,
        ),
        Err(RuntimeError::DynamicSourcePath {
            source: "Docs",
            node: 1,
            found: "int",
        })
    );
    assert_eq!(*loader.calls.borrow(), ["Docs:c.json"]);
    // A host failure keeps its source and logical path.
    let failing = batch(&[(text("a.json"), "first"), (text("missing.json"), "second")]);
    let roots_failing = roots(&failing, &declared);
    let loader = Loader::default();
    let error = docs(
        &roots_failing
            .root_context_with_dynamic_source_loader(None, &loader)
            .unwrap(),
        SourceOwnerRef::Primary,
    )
    .unwrap_err();
    assert_eq!(
        error,
        RuntimeError::DynamicSourceLoad {
            source: "Docs",
            path: "missing.json".into(),
            message: "no document Docs:missing.json".into(),
        }
    );
    assert_eq!(*loader.calls.borrow(), ["Docs:a.json", "Docs:missing.json"]);
}

#[test]
fn a_filtered_document_root_walk_compacts_its_driver_position() {
    let primary = batch(&[(text("a.json"), "first"), (text("b.json"), "second")]);
    let declared = [
        DeclaredNamedSourceRoot {
            name: "Docs",
            instance: None,
        },
        DeclaredNamedSourceRoot {
            name: "Parts",
            instance: None,
        },
    ];
    let roots = roots(&primary, &declared);
    let loader = Loader::default();
    let context = roots
        .root_context_with_dynamic_source_loader(None, &loader)
        .unwrap();
    let documents = context
        .load_owned_dynamic_documents("Docs", SourceOwnerRef::Primary, &["Files"], 1, |row| {
            row.owned_field(
                SourceOwnerRef::Primary,
                &["Files", "Path"],
                Some(&["Files"]),
            )
        })
        .unwrap();
    let loading = documents.loading_context(&context).unwrap();
    let cohort = loading.walk_owned_source_cohort(DOCS, &[]).unwrap();
    let mut accepted = Vec::new();
    loading
        .visit_filtered_source_cohort::<RuntimeError, _, _>(
            &cohort,
            |candidate| Ok(candidate.owned_field(DOCS, &["Title"], None)? == text("B")),
            |item| {
                accepted.push(item.owned_position(SourceOwnerRef::Primary, Some(&["Files"]))?);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(accepted, [1]);
}
