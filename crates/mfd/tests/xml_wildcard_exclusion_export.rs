//! `.mfd` export refuses wildcard exclusion metadata before writing anything.

use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{
    ScalarType, SchemaNode, XML_ELEMENTS_FIELD, XML_LOCAL_NAME_FIELD, XML_NAMESPACE_URI_FIELD,
    XmlExpandedName, XmlNamespace, XmlWildcardExcludedNames, XmlWildcardNamespaceConstraint,
    XmlWildcardNamespaceList, XmlWildcardProcessContents,
};
use mapping::{Graph, NamedSource, Project, Scope};
use mfd::{ExportProfile, MfdError};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-wildcard-exclusions-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn is_empty(&self) -> std::io::Result<bool> {
        Ok(std::fs::read_dir(&self.0)?.next().is_none())
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn wildcard(constraint: XmlWildcardNamespaceConstraint) -> SchemaNode {
    let mut group = SchemaNode::group(
        XML_ELEMENTS_FIELD,
        vec![
            SchemaNode::scalar(XML_LOCAL_NAME_FIELD, ScalarType::String),
            SchemaNode::scalar(XML_NAMESPACE_URI_FIELD, ScalarType::String),
        ],
    )
    .repeating()
    .with_xml_wildcard_namespace(constraint)
    .unwrap();
    group.xml_wildcard_process_contents = XmlWildcardProcessContents::Lax;
    group
}

fn document(name: &str, wildcard: SchemaNode) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![SchemaNode::scalar("Head", ScalarType::String), wildcard],
    )
}

fn project(source: SchemaNode, target: SchemaNode) -> Project {
    Project {
        source,
        target,
        source_path: Some("source.xml".into()),
        target_path: Some("target.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope::default(),
    }
}

fn negated() -> XmlWildcardNamespaceConstraint {
    XmlWildcardNamespaceConstraint::Not {
        namespaces: XmlWildcardNamespaceList::new([XmlNamespace::Unqualified]).unwrap(),
    }
}

fn excluding() -> SchemaNode {
    wildcard(XmlWildcardNamespaceConstraint::Any)
        .with_xml_wildcard_excluded_names(
            XmlWildcardExcludedNames::new([XmlExpandedName::new(
                XmlNamespace::qualified("urn:ferrule:wx:a").unwrap(),
                "Secret",
            )
            .unwrap()])
            .unwrap(),
        )
        .unwrap()
}

fn refused(result: Result<impl std::fmt::Debug, MfdError>) -> bool {
    matches!(result, Err(MfdError::Unsupported(message)) if message.contains("wildcard exclusions"))
}

#[test]
fn exclusion_metadata_is_refused_before_any_artifact_is_written() -> Result<(), Box<dyn Error>> {
    let plain = || wildcard(XmlWildcardNamespaceConstraint::Any);
    let mut cases = vec![
        project(
            document("Source", wildcard(negated())),
            document("Target", plain()),
        ),
        project(document("Source", plain()), document("Target", excluding())),
    ];
    let mut named = project(document("Source", plain()), document("Target", plain()));
    named.extra_sources.push(NamedSource {
        name: "lookup".into(),
        path: "lookup.xml".into(),
        schema: document("Lookup", excluding()),
        options: Default::default(),
        dynamic_path: None,
    });
    cases.push(named);

    for (index, project) in cases.iter().enumerate() {
        let directory = TempDir::new()?;
        let design = directory.0.join("mapping.mfd");
        assert!(
            refused(mfd::preflight_export(project, &design)),
            "case {index}"
        );
        assert!(refused(mfd::export(project, &design)), "case {index}");
        assert!(
            refused(mfd::export_with_profile(
                project,
                &design,
                ExportProfile::NativeMfd
            )),
            "case {index}"
        );
        assert!(
            refused(
                mfd::export_package(project, "mapping.mfd", ExportProfile::default())
                    .map(|package| package.report)
            ),
            "case {index}"
        );
        assert!(directory.is_empty()?, "case {index} wrote artifacts");
    }

    // The same designs without exclusions are not refused for this reason.
    let control = project(document("Source", plain()), document("Target", plain()));
    let directory = TempDir::new()?;
    assert!(!refused(mfd::preflight_export(
        &control,
        &directory.0.join("mapping.mfd")
    )));
    Ok(())
}
