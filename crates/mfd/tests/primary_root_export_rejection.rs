use ir::{ScalarType, SchemaNode};
use mapping::{Binding, Graph, Node, Project, Scope};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-primary-root-export-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn bytes(&self) -> BTreeMap<String, Vec<u8>> {
        std::fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                (
                    path.file_name().unwrap().to_str().unwrap().to_owned(),
                    std::fs::read(path).unwrap(),
                )
            })
            .collect()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn project() -> Project {
    let schema = || SchemaNode::group("Root", vec![SchemaNode::scalar("Code", ScalarType::String)]);
    Project {
        source: schema(),
        target: schema(),
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [(
                0,
                Node::SourceField {
                    path: vec!["Code".into()],
                    frame: None,
                },
            )]
            .into(),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Code".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    }
}
#[test]
fn unqualified_new_primitives_reject_native_export_before_any_publication() {
    let directory = Directory::new();
    let baseline = project();
    let original = directory.0.join("ordinary.mfd");
    mfd::preflight_export(&baseline, &original).unwrap();
    mfd::export(&baseline, &original).unwrap();
    let before = directory.bytes();
    for node in [
        Node::SourceRootXmlTypeEquals {
            canonical_expanded_type: "Derived".into(),
        },
        Node::SourceRootField {
            path: vec!["Code".into()],
        },
    ] {
        let mut candidate = baseline.clone();
        candidate.graph.nodes.insert(7, node);
        let destination = directory.0.join("unqualified.mfd");
        for result in [
            mfd::preflight_export(&candidate, &destination).map(|_| ()),
            mfd::export(&candidate, &destination).map(|_| ()),
        ] {
            assert!(
                matches!(result,Err(mfd::MfdError::Unsupported(reason)) if reason=="primary-root primitive node 7 has no qualified native export representation")
            );
        }
        assert!(!destination.exists());
        assert_eq!(directory.bytes(), before);
    }
}
