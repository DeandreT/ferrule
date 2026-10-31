//! `.mfd` export writes valid wildcard exclusions into the generated XSD
//! siblings of XML document components and refuses every other placement
//! before writing anything.

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

fn qualified(uri: &str) -> XmlNamespace {
    XmlNamespace::qualified(uri).unwrap()
}

fn names(namespace: XmlNamespace, local: &str) -> XmlWildcardExcludedNames {
    XmlWildcardExcludedNames::new([XmlExpandedName::new(namespace, local).unwrap()]).unwrap()
}

#[test]
fn misplaced_or_unexportable_exclusions_are_refused_before_any_artifact_is_written()
-> Result<(), Box<dyn Error>> {
    let plain = || wildcard(XmlWildcardNamespaceConstraint::Any);
    let mut cases = Vec::new();

    // Excluded names on a node that is not a wildcard group.
    let mut head = SchemaNode::scalar("Head", ScalarType::String);
    head.xml_wildcard_excluded_names = Some(names(XmlNamespace::Unqualified, "Head"));
    cases.push(project(
        SchemaNode::group("Source", vec![head, plain()]),
        document("Target", plain()),
    ));
    // A negated namespace set on an ordinary group.
    let mut ordinary =
        SchemaNode::group("Body", vec![SchemaNode::scalar("Line", ScalarType::String)]);
    ordinary.xml_wildcard_namespace = Some(negated());
    cases.push(project(
        document("Source", plain()),
        SchemaNode::group("Target", vec![ordinary]),
    ));
    // An excluded name in a namespace the wildcard's own constraint rejects.
    let mut listed =
        wildcard(XmlWildcardNamespaceConstraint::list([qualified("urn:ferrule:wx:a")]).unwrap());
    listed.xml_wildcard_excluded_names = Some(names(qualified("urn:ferrule:wx:b"), "Other"));
    cases.push(project(
        document("Source", listed),
        document("Target", plain()),
    ));
    // Valid exclusions on boundaries that export without an XSD sibling.
    let mut json = project(document("Source", excluding()), document("Target", plain()));
    json.source_path = Some("source.json".into());
    cases.push(json);
    let mut named = project(document("Source", plain()), document("Target", plain()));
    named.extra_sources.push(NamedSource {
        name: "lookup".into(),
        path: "lookup.csv".into(),
        schema: document("Lookup", excluding()),
        options: Default::default(),
        dynamic_path: None,
    });
    cases.push(named);
    // An XML serializer schema.
    let mut serializer = project(document("Source", plain()), document("Target", plain()));
    serializer.graph.nodes.insert(
        0,
        mapping::Node::XmlSerialize {
            path: vec!["Head".into()],
            frame: None,
            schema: Box::new(document("Head", excluding())),
            declaration: false,
            indent: false,
            namespace: None,
        },
    );
    cases.push(serializer);

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
    Ok(())
}

#[test]
fn valid_exclusions_on_xml_documents_export_into_their_schema_siblings()
-> Result<(), Box<dyn Error>> {
    let plain = || wildcard(XmlWildcardNamespaceConstraint::Any);
    let mut named = project(document("Source", plain()), document("Target", plain()));
    named.extra_sources.push(NamedSource {
        name: "lookup".into(),
        path: "lookup.xml".into(),
        schema: document("Lookup", excluding()),
        options: Default::default(),
        dynamic_path: None,
    });
    let cases = [
        (
            project(
                document("Source", wildcard(negated())),
                document("Target", plain()),
            ),
            "notNamespace",
        ),
        (
            project(document("Source", plain()), document("Target", excluding())),
            "notQName",
        ),
        (named, "notQName"),
    ];
    for (index, (project, attribute)) in cases.iter().enumerate() {
        let directory = TempDir::new()?;
        let design = directory.0.join("mapping.mfd");
        assert!(
            !refused(mfd::preflight_export(project, &design)),
            "case {index}"
        );
        mfd::export(project, &design)?;
        // Every generated XSD sibling together carries the exclusions.
        let siblings = std::fs::read_dir(&directory.0)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        let written = siblings
            .iter()
            .filter(|path| path.extension().is_some_and(|extension| extension == "xsd"))
            .map(std::fs::read_to_string)
            .collect::<Result<String, _>>()?;
        assert!(written.contains(attribute), "case {index}: {written}");
    }
    Ok(())
}
