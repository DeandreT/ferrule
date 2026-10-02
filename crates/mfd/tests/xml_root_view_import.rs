use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use mfd::{ImportIssueKind, ImportOptions, ImportProfile, MfdError};

struct Directory(PathBuf);

impl Directory {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-root-view-import-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn write(&self, name: &str, text: &str) -> std::io::Result<PathBuf> {
        let path = self.0.join(name);
        std::fs::write(&path, text)?;
        Ok(path)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const TYPES: &str = r#"<xs:complexType name="Base"><xs:attribute name="Code" type="xs:string" use="required"/></xs:complexType>
<xs:complexType name="Derived"><xs:complexContent><xs:extension base="Base"><xs:attribute name="Extra" type="xs:string" use="required"/></xs:extension></xs:complexContent></xs:complexType>"#;
const TYPE_CONDITION: &str = r#"<condition><expression><function name="equal" library="core"><expression><attribute ns="http://www.w3.org/2001/XMLSchema-instance" name="type"/></expression><expression><constant datatype="QName" value="{}Derived"/></expression></function></expression></condition>"#;
const FALSE_CONDITION: &str = r#"<condition><expression><constant datatype="boolean" value="false"/></expression></condition>"#;

fn schema(directory: &Directory, nested: bool) -> std::io::Result<()> {
    let root = if nested {
        r#"<xs:element name="Root"><xs:complexType><xs:sequence><xs:element name="Address" type="Base"/></xs:sequence></xs:complexType></xs:element>"#
    } else {
        r#"<xs:element name="Root" type="Base"/>"#
    };
    directory.write(
        "schema.xsd",
        &format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">{TYPES}{root}</xs:schema>"#
        ),
    )?;
    Ok(())
}

fn mapping(source: &str, target: &str, wrapper_condition: &str, feeds: &str) -> String {
    format!(
        r#"<mapping version="26"><component name="main" uid="1"><structure><children>
<component name="Input" library="xml" kind="14" uid="2"><data><document schema="schema.xsd" inputinstance="input.xml" instanceroot="{{}}Root"/><root><header><namespaces><namespace uid=""/></namespaces></header><entry name="FileInstance">{wrapper_condition}<entry name="document">{source}</entry></entry></root></data></component>
<component name="Output" library="xml" kind="14" uid="3"><properties XSLTDefaultOutput="1"/><data><document schema="schema.xsd" outputinstance="output.xml" instanceroot="{{}}Root"/><root><header><namespaces><namespace uid=""/></namespaces></header><entry name="FileInstance"><entry name="document">{target}</entry></entry></root></data></component>
</children><graph directed="1"><edges><edge edgekey="1"><data><dataconnection type="2"/></data></edge><edge edgekey="2"><data><dataconnection type="2"/></data></edge></edges><vertices>{feeds}</vertices></graph></structure></component></mapping>"#
    )
}

fn root(condition: &str, source: bool) -> String {
    let (code, extra) = if source {
        ("outkey=\"2\"", "outkey=\"3\"")
    } else {
        ("inpkey=\"5\"", "inpkey=\"6\"")
    };
    format!(
        r#"<entry name="Root">{condition}<entry name="Code" type="attribute" {code}/><entry name="Extra" type="attribute" {extra}/></entry>"#
    )
}

const FEEDS: &str = r#"<vertex vertexkey="2"><edges><edge vertexkey="5" edgekey="1"/></edges></vertex><vertex vertexkey="3"><edges><edge vertexkey="6" edgekey="2"/></edges></vertex>"#;

fn assert_rejected(path: &Path, text: &str) -> Result<(), Box<dyn Error>> {
    match mfd::import_with_profile(path, &ImportOptions::default(), ImportProfile::Executable) {
        Err(MfdError::IncompatibleImport(report)) => {
            assert!(!report.executable);
            assert!(
                report
                    .issues
                    .iter()
                    .any(|issue| issue.kind == ImportIssueKind::ImportWarning
                        && issue.message.contains("unproved XML document-root view")
                        && issue.message.contains(text)),
                "{report:?}"
            );
        }
        _ => return Err("expected owner-scoped typed incompatible import".into()),
    }
    Ok(())
}

#[test]
fn root_type_conditions_reject_strict_and_preserve_best_effort_project()
-> Result<(), Box<dyn Error>> {
    let directory = Directory::new()?;
    schema(&directory, false)?;
    let plain = directory.write(
        "plain.mfd",
        &mapping(&root("", true), &root("", false), "", FEEDS),
    )?;
    let conditioned = directory.write(
        "conditioned.mfd",
        &mapping(
            &root(TYPE_CONDITION, true),
            &root(TYPE_CONDITION, false),
            "",
            FEEDS,
        ),
    )?;
    let ordinary =
        mfd::import_with_profile(&plain, &ImportOptions::default(), ImportProfile::Executable)?;
    let best = mfd::import(&conditioned)?;
    assert_eq!(
        serde_json::to_value(&ordinary.imported.project)?,
        serde_json::to_value(&best.project)?
    );
    assert_eq!(best.warnings.len(), 2, "{:?}", best.warnings);
    for warning in &best.warnings {
        assert!(
            warning.contains("type condition `{}Derived`")
                && warning.contains("default `Base`")
                && warning.contains("document-root condition"),
            "{warning}"
        );
    }
    assert!(best.warnings.iter().any(|warning| warning.contains("UID 2")
        && warning.contains("outkey=2 at Code")
        && warning.contains("role source")));
    assert!(best.warnings.iter().any(|warning| warning.contains("UID 3")
        && warning.contains("inpkey=6 at Extra")
        && warning.contains("role target")));
    assert_rejected(&conditioned, "{}Root")?;
    Ok(())
}

#[test]
fn non_type_root_predicate_and_wrapper_ancestry_are_not_silent() -> Result<(), Box<dyn Error>> {
    let directory = Directory::new()?;
    schema(&directory, false)?;
    for (name, source, ancestor, reason) in [
        (
            "root",
            root(FALSE_CONDITION, true),
            "",
            "document-root condition",
        ),
        (
            "wrapper",
            root("", true),
            FALSE_CONDITION,
            "wrapper ancestor condition",
        ),
    ] {
        let path = directory.write(
            &format!("{name}.mfd"),
            &mapping(&source, &root("", false), ancestor, FEEDS),
        )?;
        let imported = mfd::import(&path)?;
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains("UID 2") && warning.contains(reason)),
            "{:?}",
            imported.warnings
        );
        assert_rejected(&path, reason)?;
    }
    Ok(())
}

#[test]
fn active_nonfirst_root_sibling_cannot_hide_behind_plain_first_root() -> Result<(), Box<dyn Error>>
{
    let directory = Directory::new()?;
    schema(&directory, false)?;
    let second = root("", true)
        .replace("outkey=\"2\"", "outkey=\"22\"")
        .replace("outkey=\"3\"", "outkey=\"23\"");
    let feeds = FEEDS
        .replace("vertexkey=\"2\"", "vertexkey=\"22\"")
        .replace("vertexkey=\"3\"", "vertexkey=\"23\"");
    let source = format!("{}{second}", root("", true));
    let path = directory.write(
        "sibling.mfd",
        &mapping(&source, &root("", false), "", &feeds),
    )?;
    let imported = mfd::import(&path)?;
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("UID 2")
                && warning.contains("connected document-root sibling")
                && warning.contains("outkey=22")),
        "{:?}",
        imported.warnings
    );
    assert_rejected(&path, "connected document-root sibling")?;
    Ok(())
}

#[test]
fn duplicate_owner_and_dangling_feed_keep_all_other_diagnostics() -> Result<(), Box<dyn Error>> {
    let directory = Directory::new()?;
    schema(&directory, false)?;
    for (name, source, feeds) in [
        (
            "duplicate",
            root(TYPE_CONDITION, true).replace("outkey=\"3\"", "outkey=\"2\""),
            FEEDS.to_string(),
        ),
        (
            "dangling",
            root(TYPE_CONDITION, true),
            FEEDS.replace("vertexkey=\"2\"", "vertexkey=\"999\""),
        ),
    ] {
        let path = directory.write(
            &format!("{name}.mfd"),
            &mapping(&source, &root(TYPE_CONDITION, false), "", &feeds),
        )?;
        let imported = mfd::import(&path)?;
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| !warning.contains("unproved XML document-root view")),
            "{:?}",
            imported.warnings
        );
        if name == "duplicate" {
            assert!(
                imported
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("nonunique raw port owner"))
            );
        }
        assert_rejected(&path, "UID 3")?;
    }
    Ok(())
}

#[test]
fn ordinary_root_and_disconnected_view_remain_executable() -> Result<(), Box<dyn Error>> {
    let directory = Directory::new()?;
    schema(&directory, false)?;
    let ordinary = root("", true);
    let unused = format!(
        r#"<entry name="Root">{TYPE_CONDITION}<entry name="Code" type="attribute" outkey="250"/></entry>"#
    );
    for (name, source, feeds) in [
        ("ordinary", ordinary.clone(), FEEDS.to_string()),
        (
            "unused",
            format!("{ordinary}{unused}"),
            format!(r#"{FEEDS}<vertex vertexkey="250"><edges/></vertex>"#),
        ),
    ] {
        let path = directory.write(
            &format!("{name}.mfd"),
            &mapping(&source, &root("", false), "", &feeds),
        )?;
        let outcome =
            mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::Executable)?;
        assert!(outcome.report.executable && outcome.imported.warnings.is_empty());
    }
    Ok(())
}

#[test]
fn ordinary_nested_child_view_retains_existing_xml_output() -> Result<(), Box<dyn Error>> {
    let directory = Directory::new()?;
    schema(&directory, true)?;
    let child_condition = TYPE_CONDITION.replace("{}Derived", "Derived");
    let source = format!(
        r#"<entry name="Root"><entry name="Address" outkey="10" displayselectionmode="all"/><entry name="Address">{child_condition}<entry name="Code" type="attribute"/><entry name="Extra" type="attribute"/></entry></entry>"#
    );
    let target = r#"<entry name="Root"><entry name="Address" inpkey="20"/></entry>"#;
    let feeds =
        r#"<vertex vertexkey="10"><edges><edge vertexkey="20" edgekey="1"/></edges></vertex>"#;
    let path = directory.write("child.mfd", &mapping(&source, target, "", feeds))?;
    let outcome =
        mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::Executable)?;
    assert!(
        outcome.imported.warnings.is_empty(),
        "{:?}",
        outcome.imported.warnings
    );
    let input = format_xml::from_str(
        r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><Address xsi:type="Derived" Code="a" Extra="b"/></Root>"#,
        &outcome.imported.project.source,
    )?;
    let output = engine::run(&outcome.imported.project, &input)?;
    let xml = format_xml::to_string(&outcome.imported.project.target, &output)?;
    assert!(
        xml.contains("xsi:type=\"Derived\"") && xml.contains("Extra=\"b\""),
        "{xml}"
    );
    let readback = format_xml::from_str(&xml, &outcome.imported.project.target)?;
    assert_eq!(input, readback);
    Ok(())
}

#[test]
fn later_protocol_document_wrapper_keeps_owner_diagnostic() -> Result<(), Box<dyn Error>> {
    let directory = Directory::new()?;
    schema(&directory, false)?;
    let source = root("", true);
    let target = root("", false);
    let plain = directory.write("one-wrapper.mfd", &mapping(&source, &target, "", FEEDS))?;
    let second = target
        .replace("inpkey=\"5\"", "inpkey=\"205\"")
        .replace("inpkey=\"6\"", "inpkey=\"206\"");
    let feeds = format!(
        r#"{FEEDS}<vertex vertexkey="2"><edges><edge vertexkey="205" edgekey="1"/></edges></vertex><vertex vertexkey="3"><edges><edge vertexkey="206" edgekey="2"/></edges></vertex>"#
    );
    let needle = format!(
        r#"<entry name="FileInstance"><entry name="document">{target}</entry></entry></root>"#
    );
    let replacement = format!(
        r#"<entry name="FileInstance"><entry name="document">{target}</entry></entry><entry name="FileInstance"><entry name="document">{second}</entry></entry></root>"#
    );
    let design = mapping(&source, &target, "", &feeds);
    assert_eq!(design.matches(&needle).count(), 1);
    let path = directory.write("two-wrappers.mfd", &design.replace(&needle, &replacement))?;
    let ordinary = mfd::import(&plain)?;
    let imported = mfd::import(&path)?;
    assert_eq!(
        serde_json::to_value(&ordinary.project)?,
        serde_json::to_value(&imported.project)?
    );
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("UID 3")
                && warning.contains("inpkey=205")
                && warning.contains("connected document-root sibling")),
        "{:?}",
        imported.warnings
    );
    assert_rejected(&path, "inpkey=205")?;
    Ok(())
}

#[test]
fn primary_root_condition_metadata_uses_its_actual_owner() -> Result<(), Box<dyn Error>> {
    let directory = Directory::new()?;
    schema(&directory, false)?;
    let plain = directory.write(
        "plain.mfd",
        &mapping(&root("", true), &root("", false), "", FEEDS),
    )?;
    let ordinary = mfd::import(&plain)?;
    let ancestor_type = TYPE_CONDITION.replace("{}Derived", "{}AncestorType");
    for (name, ancestor, root_condition, expected_condition, expected_reason) in [
        (
            "different-qname",
            ancestor_type.as_str(),
            TYPE_CONDITION,
            "{}Derived",
            "document-root condition",
        ),
        (
            "non-type",
            FALSE_CONDITION,
            TYPE_CONDITION,
            "{}Derived",
            "document-root condition",
        ),
        (
            "ancestor-only",
            ancestor_type.as_str(),
            "",
            "{}AncestorType",
            "wrapper ancestor condition",
        ),
    ] {
        let path = directory.write(
            &format!("{name}.mfd"),
            &mapping(
                &root(root_condition, true),
                &root("", false),
                ancestor,
                FEEDS,
            ),
        )?;
        let best = mfd::import(&path)?;
        assert_eq!(
            serde_json::to_value(&ordinary.project)?,
            serde_json::to_value(&best.project)?
        );
        assert_eq!(best.warnings.len(), 1, "{:?}", best.warnings);
        let warning = &best.warnings[0];
        assert!(
            warning.contains(expected_reason)
                && warning.contains(&format!("type condition `{expected_condition}`")),
            "{warning}"
        );
        assert_rejected(&path, expected_reason)?;
    }
    Ok(())
}
