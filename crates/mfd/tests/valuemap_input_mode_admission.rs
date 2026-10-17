//! Admission tests, not a claim about native input-token binding or casts.
//! An explicit unsupported mode must survive every lowering route as a diagnostic.
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, Value};
use mapping::{Node, Project};

#[derive(Clone, Copy, Debug)]
enum Route {
    Direct,
    Scalar,
    MultiOutput,
    Nested,
    Structured,
}

const ROUTES: [Route; 5] = [
    Route::Direct,
    Route::Scalar,
    Route::MultiOutput,
    Route::Nested,
    Route::Structured,
];

struct Directory(PathBuf);

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture(route: Route, kind: &str, mode: &str, literal: &str) -> Directory {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let directory = loop {
        let path = std::env::temp_dir().join(format!(
            "ferrule_valuemap_admission_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&path) {
            Ok(()) => break Directory(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("cannot create fixture: {error}"),
        }
    };
    let structured = matches!(route, Route::Structured);
    for (name, field) in [
        (
            "Source",
            r#"<xs:element name="sourceVal" type="xs:string"/>"#,
        ),
        (
            "Target",
            r#"<xs:element name="targetVal" type="xs:string" minOccurs="0"/>"#,
        ),
    ] {
        let mut fields = field.to_string();
        if name == "Target" && matches!(route, Route::MultiOutput) {
            fields.push_str(r#"<xs:element name="echoVal" type="xs:string"/>"#);
        }
        if structured {
            fields = format!(
                r#"<xs:element name="Record"><xs:complexType><xs:sequence>{fields}</xs:sequence></xs:complexType></xs:element>"#
            );
        }
        fs::write(
            directory.0.join(format!("{}.xsd", name.to_lowercase())),
            format!(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="{name}"><xs:complexType><xs:sequence>{fields}</xs:sequence></xs:complexType></xs:element></xs:schema>"#
            ),
        )
        .unwrap();
    }

    // Internal uid 101 is the input, 102 the mapped output, and 103 the map.
    // Public componentid values refer to those uids, never to graph port keys.
    let map = format!(
        r#"<component name="value-map" library="core" uid="103" kind="{kind}"><sources><datapoint pos="0" key="202"/></sources><targets><datapoint pos="0" key="203"/></targets><data><valuemap {mode}><valuemapTable><entry from="A" to="B"/></valuemapTable><input name="input" type="simpleString"/><result name="result" type="String" {literal}/></valuemap></data></component>"#
    );
    let mut body = format!(
        r#"<component name="raw" library="core" uid="101" kind="6"><targets><datapoint pos="0" key="201"/></targets><data><input datatype="string"/><parameter usageKind="input" name="raw"/></data></component>{map}<component name="mapped" library="core" uid="102" kind="7"><sources><datapoint pos="0" key="204"/></sources><data><output datatype="string"/><parameter usageKind="output" name="mapped"/></data></component>"#
    );
    let mut internal_edges =
        String::from(r#"<edge from="201" to="202"/><edge from="203" to="204"/>"#);
    if matches!(route, Route::MultiOutput) {
        // Two declared outputs deliberately prevent first-class scalar lowering.
        body.push_str(
            r#"<component name="echo" library="core" uid="104" kind="7"><sources><datapoint pos="0" key="205"/></sources><data><output datatype="string"/><parameter usageKind="output" name="echo"/></data></component>"#,
        );
        internal_edges.push_str(r#"<edge from="201" to="205"/>"#);
    }
    if structured {
        body = format!(
            r#"<component name="Record" library="xml" uid="101" kind="14"><properties UsageKind="input"/><data><root><entry name="Record"><entry name="sourceVal" outkey="201"/></entry></root><document schema="source.xsd" instanceroot="{{}}Source/{{}}Record"/><parameter usageKind="input" name="Record"/></data></component>{map}<component name="Record" library="xml" uid="102" kind="14"><properties UsageKind="output"/><data><root><entry name="Record"><entry name="targetVal" inpkey="204"/></entry></root><document schema="target.xsd" instanceroot="{{}}Target/{{}}Record"/><parameter usageKind="output" name="Record"/></data></component>"#
        );
    }
    let mut definitions = format!(
        r#"<component name="MapCode" library="user" editable="1"><structure><children>{body}</children><connections>{internal_edges}</connections></structure></component>"#
    );
    let (name, input_id, output_id) = if matches!(route, Route::Nested) {
        definitions.push_str(
            r#"<component name="WrappedCode" library="user" editable="1"><structure><children>
          <component name="raw" library="core" uid="301" kind="6"><targets><datapoint pos="0" key="401"/></targets><data><input datatype="string"/><parameter usageKind="input" name="raw"/></data></component>
          <component name="MapCode" library="user" uid="302" kind="19"><data><root><entry name="raw" inpkey="402" componentid="101"/></root><root rootindex="1"><entry name="mapped" outkey="403" componentid="102"/></root></data></component>
          <component name="mapped" library="core" uid="303" kind="7"><sources><datapoint pos="0" key="404"/></sources><data><output datatype="string"/><parameter usageKind="output" name="mapped"/></data></component>
        </children><connections><edge from="401" to="402"/><edge from="403" to="404"/></connections></structure></component>"#,
        );
        ("WrappedCode", 301, 303)
    } else {
        ("MapCode", 101, 102)
    };
    let echo_port = if matches!(route, Route::MultiOutput) {
        r#"<entry name="echo" outkey="32" componentid="104"/>"#
    } else {
        ""
    };
    let mut call = format!(
        r#"<component name="{name}" library="user" kind="19"><data><root><entry name="raw" inpkey="30" componentid="{input_id}"/></root><root rootindex="1"><entry name="mapped" outkey="31" componentid="{output_id}"/>{echo_port}</root></data></component>"#
    );
    let mut source_root = String::from(
        r#"<entry name="Source" outkey="10"><entry name="sourceVal" outkey="11"/></entry>"#,
    );
    let mut target_fields = String::from(r#"<entry name="targetVal" inpkey="21"/>"#);
    let mut edges = String::from(
        r#"<edge from="10" to="20"/><edge from="11" to="30"/><edge from="31" to="21"/>"#,
    );
    if matches!(route, Route::MultiOutput) {
        target_fields.push_str(r#"<entry name="echoVal" inpkey="22"/>"#);
        edges.push_str(r#"<edge from="32" to="22"/>"#);
    }
    if structured {
        source_root = String::from(
            r#"<entry name="Source"><entry name="Record" outkey="10"><entry name="sourceVal" outkey="11"/></entry></entry>"#,
        );
        target_fields = format!(r#"<entry name="Record" inpkey="20">{target_fields}</entry>"#);
        call = String::from(
            r#"<component name="MapCode" library="user" kind="19"><data><root><entry name="Record" inpkey="30" componentid="101"/></root><root rootindex="1"><entry name="Record" outkey="31" componentid="102"/></root></data></component>"#,
        );
        edges = String::from(r#"<edge from="10" to="30"/><edge from="31" to="20"/>"#);
    }
    if matches!(route, Route::Direct) {
        call = map;
        definitions.clear();
        edges = String::from(
            r#"<edge from="10" to="20"/><edge from="11" to="202"/><edge from="203" to="21"/>"#,
        );
    }
    let target_root = if structured {
        format!(r#"<entry name="Target">{target_fields}</entry>"#)
    } else {
        format!(r#"<entry name="Target" inpkey="20">{target_fields}</entry>"#)
    };
    fs::write(
        directory.0.join("mapping.mfd"),
        format!(
            r#"<mapping version="26" xmlns:foreign="urn:ferrule:test:foreign"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root>{source_root}</root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{{}}Source"/></data></component>
          {call}
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root>{target_root}</root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{{}}Target"/></data></component>
        </children></structure><connections>{edges}</connections></component>{definitions}</mapping>"#
        ),
    )
    .unwrap();
    directory
}

fn mode_warning(message: &str, route: Route) -> bool {
    let owner = match route {
        Route::Direct => "value-map",
        Route::Nested => "WrappedCode",
        _ => "MapCode",
    };
    // Deliberately do not freeze punctuation or the still-evolving refusal reason.
    message.contains("defaultValueMode") && message.contains("input") && message.contains(owner)
}

fn assert_input_mode_refused(path: &Path, route: Route) {
    let imported = mfd::import(path).unwrap();
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| mode_warning(warning, route)),
        "{route:?}, {}: {:?}",
        path.display(),
        imported.warnings
    );
    let expected_report = mfd::assess_import(&imported);
    let Err(mfd::MfdError::IncompatibleImport(report)) = mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    ) else {
        panic!("{route:?}: expected diagnostic profile refusal, not success or an IO/XML error");
    };
    assert!(!report.executable);
    assert!(report.issues.iter().any(|issue| {
        issue.kind == mfd::ImportIssueKind::ImportWarning && mode_warning(&issue.message, route)
    }));
    assert_eq!(*report, expected_report);
    // No execution or export: the partial repair project is not a faithful
    // mode representation.
}

fn assert_values(project: &Project, route: Route, miss: Value) {
    assert!(engine::validate(project).is_empty());
    for (raw, mapped) in [("A", Value::String("B".into())), ("MISS", miss)] {
        let mut input = Instance::Group(
            vec![(
                "sourceVal".into(),
                Instance::Scalar(Value::String(raw.into())),
            )]
            .into(),
        );
        let mut fields = vec![("targetVal".into(), Instance::Scalar(mapped))];
        if matches!(route, Route::MultiOutput) {
            fields.push((
                "echoVal".into(),
                Instance::Scalar(Value::String(raw.into())),
            ));
        }
        let mut expected = Instance::Group(fields.into());
        if matches!(route, Route::Structured) {
            input = Instance::Group(vec![("Record".into(), input)].into());
            expected = Instance::Group(vec![("Record".into(), expected)].into());
        }
        assert_eq!(
            engine::run(project, &input).unwrap(),
            expected,
            "{route:?}: {raw}"
        );
    }
}

fn assert_control_route(project: &Project, route: Route, default: Option<Value>) {
    let maps: Vec<_> = project
        .graph
        .nodes
        .values()
        .chain(
            project
                .user_functions
                .values()
                .flat_map(|function| function.body.nodes.values()),
        )
        .filter_map(|node| match node {
            Node::ValueMap {
                input_type,
                table,
                default,
                ..
            } => Some((input_type, table, default)),
            _ => None,
        })
        .collect();
    assert!(
        !maps.is_empty(),
        "{route:?}: fixture did not reach value-map lowering"
    );
    for (ty, table, actual_default) in maps {
        assert_eq!(*ty, Some(ScalarType::String));
        assert_eq!(
            table,
            &vec![(Value::String("A".into()), Value::String("B".into()))]
        );
        assert_eq!(actual_default, &default);
    }
    if matches!(route, Route::Scalar | Route::Nested) {
        let name = if matches!(route, Route::Nested) {
            "WrappedCode"
        } else {
            "MapCode"
        };
        assert!(project.user_functions.values().any(|function| {
            function.library == "user"
                && function.name == name
                && function.output_type == ScalarType::String
        }));
        assert!(
            project
                .graph
                .nodes
                .values()
                .any(|node| matches!(node, Node::UserFunctionCall { .. }))
        );
    } else {
        assert!(
            project.user_functions.is_empty(),
            "{route:?}: expected inline/direct lowering"
        );
    }
}

fn assert_control(path: &Path, route: Route, default: Option<Value>) {
    let imported = mfd::import(path).unwrap();
    assert!(
        imported.warnings.is_empty(),
        "{route:?}, {}: {:?}",
        path.display(),
        imported.warnings
    );
    let strict = mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    assert!(strict.report.executable && strict.report.issues.is_empty());
    assert!(strict.imported.warnings.is_empty());
    assert_control_route(&imported.project, route, default.clone());
    assert_values(
        &imported.project,
        route,
        default.clone().unwrap_or(Value::Null),
    );
    assert_values(
        &strict.imported.project,
        route,
        default.clone().unwrap_or(Value::Null),
    );
    for (name, profile) in [
        ("roundtrip.mfd", None),
        ("native-roundtrip.mfd", Some(mfd::ExportProfile::NativeMfd)),
    ] {
        let exported = path.with_file_name(name);
        let warnings = match profile {
            Some(profile) => {
                mfd::export_with_profile(&imported.project, &exported, profile)
                    .unwrap()
                    .warnings
            }
            None => mfd::export(&imported.project, &exported).unwrap(),
        };
        assert!(warnings.is_empty(), "{route:?}, {name}: {warnings:?}");
        let restored = mfd::import_with_profile(
            &exported,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        )
        .unwrap();
        assert!(restored.imported.warnings.is_empty());
        assert!(restored.report.executable && restored.report.issues.is_empty());
        assert_values(
            &restored.imported.project,
            route,
            default.clone().unwrap_or(Value::Null),
        );
    }
}

#[test]
fn explicit_input_mode_warns_and_refuses_executable_import_on_every_route() {
    for route in ROUTES {
        for (mode, literal) in [
            (r#"defaultValueMode="input""#, ""),
            (r#"defaultValueMode="input""#, r#"defaultValue="D""#),
            // An old enabled literal must not turn an explicit input mode into custom.
            (
                r#"defaultValueMode="input" enableDefaultValue="1""#,
                r#"defaultValue="D""#,
            ),
        ] {
            let directory = fixture(route, "23", mode, literal);
            assert_input_mode_refused(&directory.0.join("mapping.mfd"), route);
        }
    }
}

#[test]
fn custom_legacy_and_absent_modes_keep_complete_values_and_roundtrip() {
    for route in ROUTES {
        for (mode, literal, default) in [
            (
                r#"defaultValueMode="custom""#,
                r#"defaultValue="D""#,
                Some(Value::String("D".into())),
            ),
            (
                r#"enableDefaultValue="1""#,
                r#"defaultValue="D""#,
                Some(Value::String("D".into())),
            ),
            ("", r#"defaultValue="D""#, None),
            ("", "", None),
            (
                r#"defaultValueMode="custom""#,
                r#"defaultValue="""#,
                Some(Value::String(String::new())),
            ),
        ] {
            let directory = fixture(route, "23", mode, literal);
            assert_control(&directory.0.join("mapping.mfd"), route, default);
        }
    }
}

#[test]
fn numeric_kind_spellings_keep_custom_controls_and_input_mode_refusals() {
    for route in ROUTES {
        for kind in ["023", "+23"] {
            let control = fixture(
                route,
                kind,
                r#"defaultValueMode="custom""#,
                r#"defaultValue="D""#,
            );
            assert_control(
                &control.0.join("mapping.mfd"),
                route,
                Some(Value::String("D".into())),
            );
            let unsupported = fixture(
                route,
                kind,
                r#"defaultValueMode="input" enableDefaultValue="1""#,
                r#"defaultValue="D""#,
            );
            assert_input_mode_refused(&unsupported.0.join("mapping.mfd"), route);
        }
    }
}

#[test]
fn foreign_metadata_and_literal_mode_text_are_not_executable_mode_declarations() {
    for route in ROUTES {
        for (mode, default) in [
            (
                r#"defaultValueMode="custom" foreign:defaultValueMode="input""#,
                Some(Value::String("defaultValueMode=\"input\"".into())),
            ),
            (r#"foreign:defaultValueMode="input""#, None),
        ] {
            let directory = fixture(
                route,
                "23",
                mode,
                r#"defaultValue="defaultValueMode=&quot;input&quot;""#,
            );
            let path = directory.0.join("mapping.mfd");
            let design = fs::read_to_string(&path).unwrap();
            assert_eq!(
                design.matches("<valuemap ").count(),
                1,
                "{route:?}: expected exactly one value-map replacement site"
            );
            let design = design.replace(
                "<valuemap ",
                "<foreign:metadata defaultValueMode=\"input\">defaultValueMode=\"input\"</foreign:metadata><valuemap ",
            );
            assert_eq!(
                design
                    .matches("<foreign:metadata defaultValueMode=\"input\">defaultValueMode=\"input\"</foreign:metadata>")
                    .count(),
                1,
                "{route:?}: expected exactly one inserted foreign metadata element"
            );
            assert_eq!(
                design.matches("<valuemap ").count(),
                1,
                "{route:?}: expected exactly one actual value-map after replacement"
            );
            fs::write(&path, design).unwrap();
            let imported = mfd::import(&path).unwrap();
            assert!(
                imported.warnings.is_empty(),
                "{route:?}: {:?}",
                imported.warnings
            );
            let outcome = mfd::import_with_profile(
                &path,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable,
            )
            .unwrap();
            assert!(outcome.report.executable && outcome.report.issues.is_empty());
            assert_control_route(&outcome.imported.project, route, default.clone());
            assert_values(
                &outcome.imported.project,
                route,
                default.unwrap_or(Value::Null),
            );
        }
    }
}
