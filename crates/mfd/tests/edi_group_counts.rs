use std::path::PathBuf;

#[path = "edi_group_counts/hosts.rs"]
mod hosts;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/edi-group-counts")
        .join(name)
}

fn replace_one(text: &str, from: &str, to: &str) -> String {
    assert_eq!(text.matches(from).count(), 1, "fixture fragment: {from}");
    let replaced = text.replace(from, to);
    assert_ne!(replaced, text);
    replaced
}

fn wire(case: &serde_json::Value) -> String {
    let groups = case["groups"].as_array().unwrap();
    let mut text = String::from(
        "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *261001*1200*^*00501*000000001*0*T*:~",
    );
    for (index, group) in groups.iter().enumerate() {
        let control = index + 1;
        text.push_str(&format!(
            "GS*OW*SENDER*RECEIVER*20261001*1200*{control}*X*005010~"
        ));
        for message in group.as_array().unwrap() {
            text.push_str(message.as_str().unwrap());
        }
        text.push_str(&format!(
            "GE*{}*{control}~",
            group.as_array().unwrap().len()
        ));
    }
    text.push_str(&format!("IEA*{}*000000001~", groups.len()));
    text
}

fn assert_typed_output(actual: &ir::Instance, expected: &serde_json::Value) {
    match expected {
        serde_json::Value::Object(fields) => {
            let ir::Instance::Group(actual_fields) = actual else {
                panic!("expected object: {actual:?}");
            };
            assert_eq!(actual_fields.len(), fields.len());
            for (name, expected) in fields {
                assert_typed_output(actual.field(name).unwrap(), expected);
            }
        }
        serde_json::Value::Array(items) => {
            let actual = actual.as_repeated().unwrap();
            assert_eq!(actual.len(), items.len());
            for (actual, expected) in actual.iter().zip(items) {
                assert_typed_output(actual, expected);
            }
        }
        serde_json::Value::Number(number) => {
            assert_eq!(
                actual.as_scalar(),
                Some(&ir::Value::Int(number.as_i64().unwrap()))
            );
        }
        serde_json::Value::String(text) => {
            assert_eq!(actual.as_scalar(), Some(&ir::Value::String(text.clone())));
        }
        _ => panic!("outside the authored output domain"),
    }
}

#[test]
fn structured_edi_counts_preserve_complete_outputs_and_integer_types() {
    let imported = mfd::import(&fixture("counts.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );
    assert!(mfd::assess_import(&imported).executable);
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/edi-group-counts/cases.json")).unwrap();
    for case in cases {
        let input =
            format_edi::x12::from_str(&wire(&case), &imported.project.source, false).unwrap();
        let output = engine::run(&imported.project, &input).unwrap();
        let actual: serde_json::Value = serde_json::from_str(
            &format_json::to_string(&imported.project.target, &output).unwrap(),
        )
        .unwrap();
        assert_eq!(actual, case["expected"], "{}", case["name"]);
        assert_typed_output(&output, &case["expected"]);
    }
}

#[test]
fn malformed_whole_x12_is_not_an_empty_sequence() {
    let project = mfd::import(&fixture("counts.mfd")).unwrap().project;
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/edi-group-counts/cases.json")).unwrap();
    let valid = wire(&cases[0]);
    for malformed in [
        valid.replace("*00501*", "*0050A*"),
        valid.replace("W05*N*ALPHA~", ""),
        valid.replace("SE*3*0001~", ""),
    ] {
        assert!(
            format_edi::x12::from_str(&malformed, &project.source, false).is_err(),
            "{malformed}"
        );
    }
}

struct Package(PathBuf);

impl Package {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "ferrule_edi_group_counts_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        std::fs::create_dir(&directory).unwrap();
        for name in ["Envelope.Config", "940.Config", "target.schema.json"] {
            std::fs::copy(fixture(name), directory.join(name)).unwrap();
        }
        Self(directory)
    }
}

impl Drop for Package {
    fn drop(&mut self) {
        if std::thread::panicking()
            || std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                == Some(std::ffi::OsStr::new("1"))
        {
            eprintln!("retained EDI count package: {}", self.0.display());
            return;
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn ambiguous_and_unresolved_count_feeds_keep_executable_refusals() {
    let original = include_str!("fixtures/edi-group-counts/counts.mfd");
    for (label, text, expected_warning) in [
        (
            "disconnected",
            replace_one(original, "<edge from=\"11\" to=\"201\"/>", ""),
            "has an unresolvable sequence input",
        ),
        (
            "conflicting",
            replace_one(
                original,
                "</connections>",
                "<edge from=\"13\" to=\"201\"/></connections>",
            ),
            "has an unresolvable sequence input",
        ),
        (
            "duplicate producer",
            replace_one(
                original,
                "name=\"Party\" outkey=\"13\"",
                "name=\"Party\" outkey=\"11\"",
            ),
            "requires one unambiguous physical group feed",
        ),
        (
            "unknown feed",
            replace_one(
                original,
                "from=\"11\" to=\"201\"",
                "from=\"999\" to=\"201\"",
            ),
            "has an unresolvable sequence input",
        ),
        (
            "extra argument",
            replace_one(
                &replace_one(
                    original,
                    "key=\"201\"/></sources>",
                    "key=\"201\"/><datapoint pos=\"2\" key=\"209\"/></sources>",
                ),
                "</connections>",
                "<edge from=\"12\" to=\"209\"/></connections>",
            ),
            "does not accept an extra argument",
        ),
    ] {
        let package = Package::new();
        let mapping = package.0.join("mapping.mfd");
        std::fs::write(&mapping, text).unwrap();
        let imported = mfd::import(&mapping).unwrap();
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains("aggregate `count`")
                    && warning.contains(expected_warning)),
            "{label}: {:?}",
            imported.warnings
        );
        assert!(!mfd::assess_import(&imported).executable, "{label}");
        assert!(
            mfd::import_with_profile(
                &mapping,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable
            )
            .is_err(),
            "{label}"
        );
    }
}

#[test]
fn non_count_reductions_keep_the_non_scalar_value_guard() {
    let package = Package::new();
    let mapping = package.0.join("mapping.mfd");
    let original = include_str!("fixtures/edi-group-counts/counts.mfd");
    assert_eq!(
        original
            .matches("<component name=\"count\" library=\"core\" kind=\"5\">")
            .count(),
        4
    );
    std::fs::write(
        &mapping,
        original.replacen(
            "<component name=\"count\" library=\"core\" kind=\"5\">",
            "<component name=\"sum\" library=\"core\" kind=\"5\">",
            1,
        ),
    )
    .unwrap();
    let imported = mfd::import(&mapping).unwrap();
    assert!(
        engine::validate(&imported.project)
            .iter()
            .any(|issue| issue.message.contains("not a scalar"))
    );
    assert!(!mfd::assess_import(&imported).executable);
}

#[test]
fn secondary_edi_counts_keep_their_source_identity_inside_primary_scopes() {
    let package = Package::new();
    let mapping = package.0.join("mapping.mfd");
    let text = replace_one(
        include_str!("fixtures/edi-group-counts/counts.mfd"),
        "<component name=\"target\"",
        r#"<component name="secondary" library="text" kind="16"><data>
          <root><entry name="document"><entry name="Envelope"><entry name="Group"><entry name="Message"><entry name="W05" outkey="31"/></entry></entry></entry></entry></root>
          <text type="edi" kind="EDIX12" config="940.Config" inputinstance="secondary.x12"/>
        </data></component><component name="target""#,
    );
    std::fs::write(
        &mapping,
        replace_one(&text, "from=\"11\" to=\"201\"", "from=\"31\" to=\"201\""),
    )
    .unwrap();
    let imported = mfd::import(&mapping).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(mfd::assess_import(&imported).executable);
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/edi-group-counts/cases.json")).unwrap();
    let project = &imported.project;
    let source = format_edi::x12::from_str(&wire(&cases[0]), &project.source, false).unwrap();
    let secondary = &project.extra_sources[0];
    let extra =
        format_edi::x12::from_str(&wire(cases.last().unwrap()), &secondary.schema, false).unwrap();
    let output =
        engine::run_with_sources(project, &source, vec![(secondary.name.clone(), extra)]).unwrap();
    let expected = serde_json::json!({"Headers":3,"Parties":0,"Notes":0,"Orders":[{"Order":"ALPHA","Headers":3,"Parties":0,"Lines":0,"Notes":0,"Details":[]}]});
    assert_typed_output(&output, &expected);
}
