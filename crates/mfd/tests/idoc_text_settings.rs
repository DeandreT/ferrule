use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use mapping::{EdiBoundaryKind, Project};
use mfd::{ExportCompatibility, ExportCompatibilityFeature as Feature, ExportProfile};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_idoc_text_settings_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const VALIDATION_CASES: [(&str, &str); 16] = [
    ("missing-segment", "stop"),
    ("missing-group", "stop"),
    ("missing-field-or-composite", "report+reject"),
    ("extra-data", "report+reject"),
    ("invalid-field-value", "report+reject"),
    ("invalid-date", "report+reject"),
    ("invalid-time", "report+reject"),
    ("extra-repeat", "report+reject"),
    ("numeric-overflow", "report+reject"),
    ("data-element-too-short", "report+reject"),
    ("data-element-too-long", "report+reject"),
    ("unexpected-eof", "stop"),
    ("invalid-codelist-value", "report+reject"),
    ("semantic", "report+reject"),
    ("segment-not-in-message", "stop"),
    ("unrecognized-segment-id", "stop"),
];

fn config() -> &'static str {
    "BEGIN_SEGMENT_SECTION\nBEGIN_IDOC DEMO01\nBEGIN_GROUP 1\nLEVEL 01\nSTATUS MANDATORY\nLOOPMIN 0000000001\nLOOPMAX 0000000001\nBEGIN_SEGMENT E2LINE\nSEGMENTTYPE E1LINE\nQUALIFIED\nLEVEL 02\nSTATUS OPTIONAL\nLOOPMIN 0000000000\nLOOPMAX 0000000009\nBEGIN_FIELDS\nNAME CODE\nTEXT Synthetic code\nTYPE CHARACTER\nLENGTH 000004\nFIELD_POS 0001\nBYTE_FIRST 000064\nBYTE_LAST 000067\nVALUE 'A'\nVALUE_TEXT Allowed\nEND_FIELDS\nEND_SEGMENT\nEND_GROUP\nEND_IDOC\nEND_SEGMENT_SECTION\n"
}

fn native_settings() -> String {
    let mut xml = String::from(
        "<settings unpackedformat=\"false\" autocompletedata=\"true\" \
         terminatewithlinefeed=\"false\" syntaxversionnumber=\"2\" \
         controllingagency=\"Fixed\" syntaxlevel=\"A\" isidoc=\"true\">\
         <separators dataelement=\"%20\" component=\"%20\" subcomponent=\"\" \
         segment=\"%0A\" decimal=\".\" escape=\"%20\" repetition=\"%20\"/>\
         <validation>",
    );
    for (kind, action) in VALIDATION_CASES {
        write!(xml, "<case kind=\"{kind}\" action=\"{action}\"/>").unwrap();
    }
    xml.push_str("</validation></settings>");
    xml
}

fn write_design(dir: &Path, settings: &str) -> PathBuf {
    std::fs::write(dir.join("parser.txt"), config()).unwrap();
    let design = dir.join("original.mfd");
    let xml = format!(
        r#"<mapping version="22"><resources/><component name="defaultmap" uid="1">
          <structure><children>
            <component name="idoc" library="text" uid="2" kind="16"><properties/><data>
              <root><entry name="FileInstance"><entry name="document"><entry name="Envelope">
                <entry name="SG1"><entry name="E2LINE"><entry name="CODE" outkey="10"/></entry></entry>
              </entry></entry></entry></root>
              <text type="edi" kind="EDIFIXED" config="parser.txt" inputinstance="input.idoc"
                    encoding="1" byteorder="1" byteordermark="0">{settings}</text>
            </data></component>
            <component name="output" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1"/><data>
              <root><entry name="Outputs"><entry name="Value" inpkey="20"/></entry></root>
            </data></component>
          </children><graph directed="1"><vertices>
            <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
          </vertices></graph></structure>
        </component></mapping>"#
    );
    std::fs::write(&design, xml).unwrap();
    design
}

fn imported_project(dir: &Path) -> Project {
    let imported = mfd::import(&write_design(dir, &native_settings())).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_eq!(
        imported.project.source_options.edi_kind,
        Some(EdiBoundaryKind::Idoc)
    );
    assert!(imported.project.source_options.idoc.is_some());
    assert!(imported.project.source_options.idoc_native_config.is_some());
    assert!(
        imported
            .project
            .source_options
            .idoc_native_text_settings
            .is_some()
    );
    imported.project
}

fn idoc_text<'a>(xml: &'a str) -> (roxmltree::Document<'a>, roxmltree::NodeId) {
    let document = roxmltree::Document::parse(xml).unwrap();
    let id = document
        .descendants()
        .find(|node| node.has_tag_name("text") && node.attribute("kind") == Some("EDIFIXED"))
        .unwrap()
        .id();
    (document, id)
}

fn assert_native_text_settings(xml: &str) -> String {
    let (document, id) = idoc_text(xml);
    let text = document.get_node(id).unwrap();
    assert_eq!(text.attribute("type"), Some("edi"));
    assert_eq!(text.attribute("kind"), Some("EDIFIXED"));
    let input_instance = text.attribute("inputinstance").or_else(|| {
        document
            .descendants()
            .find(|node| {
                node.has_tag_name("file") && node.attribute("role") == Some("inputinstance")
            })
            .and_then(|node| node.attribute("name"))
    });
    assert_eq!(input_instance, Some("input.idoc"));
    assert_eq!(text.attribute("encoding"), Some("1"));
    assert_eq!(text.attribute("byteorder"), Some("1"));
    assert_eq!(text.attribute("byteordermark"), Some("0"));
    let config = text.attribute("config").unwrap().to_string();
    let relative = Path::new(&config);
    assert!(!relative.is_absolute());
    assert!(
        relative
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
    );
    assert!(config.ends_with(".txt"));

    let settings = text
        .children()
        .find(|node| node.has_tag_name("settings"))
        .unwrap();
    for (name, expected) in [
        ("unpackedformat", "false"),
        ("autocompletedata", "true"),
        ("terminatewithlinefeed", "false"),
        ("syntaxversionnumber", "2"),
        ("controllingagency", "Fixed"),
        ("syntaxlevel", "A"),
        ("isidoc", "true"),
    ] {
        assert_eq!(settings.attribute(name), Some(expected), "{name}");
    }
    let children = settings
        .children()
        .filter(|node| node.is_element())
        .collect::<Vec<_>>();
    assert_eq!(
        children
            .iter()
            .map(|node| node.tag_name().name())
            .collect::<Vec<_>>(),
        ["separators", "validation"]
    );
    let separators = children[0];
    for (name, expected) in [
        ("dataelement", "%20"),
        ("component", "%20"),
        ("subcomponent", ""),
        ("segment", "%0A"),
        ("decimal", "."),
        ("escape", "%20"),
        ("repetition", "%20"),
    ] {
        assert_eq!(separators.attribute(name), Some(expected), "{name}");
    }
    let cases = children[1]
        .children()
        .filter(|node| node.is_element())
        .map(|node| {
            assert!(node.has_tag_name("case"));
            (
                node.attribute("kind").unwrap(),
                node.attribute("action").unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(cases, VALIDATION_CASES);
    config
}

#[test]
fn native_text_settings_survive_project_export_and_relocated_reimport() {
    let dir = TempDir::new();
    let project = imported_project(&dir.0);
    let encoded = serde_json::to_string(&project).unwrap();
    let decoded: Project = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        decoded.source_options.idoc_native_text_settings,
        project.source_options.idoc_native_text_settings
    );

    let bundle = dir.0.join("portable");
    let output = bundle.join("design.mfd");
    let warnings = mfd::export(&project, &output).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let xml = std::fs::read_to_string(&output).unwrap();
    let config = assert_native_text_settings(&xml);
    assert!(bundle.join(config).is_file());

    let moved = dir.0.join("moved");
    std::fs::rename(&bundle, &moved).unwrap();
    std::fs::remove_file(dir.0.join("parser.txt")).unwrap();
    let reimported = mfd::import(&moved.join("design.mfd")).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(reimported.project.source, project.source);
    assert_eq!(reimported.project.source_options, project.source_options);
    assert!(engine::validate(&reimported.project).is_empty());

    let native_output = moved.join("native-rejected.mfd");
    let report = mfd::preflight_export(&reimported.project, &native_output).unwrap();
    assert_eq!(report.compatibility, ExportCompatibility::FerruleExtensions);
    for feature in [
        Feature::EdiSchema,
        Feature::EdiLayout,
        Feature::EdiConfigDescriptor,
    ] {
        assert!(report.issues.iter().any(|issue| issue.feature == feature));
    }
    assert!(matches!(
        mfd::export_with_profile(
            &reimported.project,
            &native_output,
            ExportProfile::NativeMfd
        ),
        Err(mfd::MfdError::IncompatibleExport(_))
    ));
    assert!(!native_output.exists());
}

#[test]
fn malformed_duplicate_missing_or_unknown_settings_drop_only_text_certificate() {
    let dir = TempDir::new();
    let settings = native_settings();
    let variants = [
        (
            "malformed",
            settings.replacen("action=\"stop\"", "action=\"unsupported\"", 1),
        ),
        ("duplicate-settings", format!("{settings}{settings}")),
        (
            "duplicate-case",
            settings.replacen(
                "</validation>",
                "<case kind=\"missing-segment\" action=\"stop\"/></validation>",
                1,
            ),
        ),
        (
            "excess-text-children",
            format!("{settings}{}", "<ferrule-layout kind=\"idoc\"/>".repeat(3)),
        ),
        ("unknown-text-child", format!("{settings}<unknown/>")),
        (
            "missing-case",
            settings.replacen("<case kind=\"semantic\" action=\"report+reject\"/>", "", 1),
        ),
        (
            "unknown-directive",
            settings.replacen("</settings>", "<unknown/></settings>", 1),
        ),
    ];
    for (index, (name, variant)) in variants.into_iter().enumerate() {
        let case_dir = dir.0.join(format!("{index}-{name}"));
        std::fs::create_dir_all(&case_dir).unwrap();
        let imported = mfd::import(&write_design(&case_dir, &variant)).unwrap();
        assert!(
            imported
                .project
                .source_options
                .idoc_native_text_settings
                .is_none(),
            "{name}"
        );
        assert!(
            imported.project.source_options.idoc_native_config.is_some(),
            "{name}"
        );
        assert!(imported.project.source_options.idoc.is_some(), "{name}");
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains("settings certificate was ignored")),
            "{name}: {:?}",
            imported.warnings
        );
        if let Some(parent) = match name {
            "duplicate-case" => Some("validation"),
            "excess-text-children" => Some("text"),
            _ => None,
        } {
            assert!(
                imported.warnings.iter().any(|warning| warning
                    .contains(&format!("too many child elements inside `{parent}`"))),
                "{name}: {:?}",
                imported.warnings
            );
        }
        assert!(engine::validate(&imported.project).is_empty(), "{name}");
    }
}

#[test]
fn stale_idoc_text_settings_reject_export_without_artifacts() {
    let dir = TempDir::new();
    let base = imported_project(&dir.0);
    type ProjectMutation = fn(&mut Project);
    let mutations: [(&str, ProjectMutation); 5] = [
        ("stale-schema", |project| {
            project.source.name = "Altered".into()
        }),
        ("missing-layout", |project| {
            project.source_options.idoc = None
        }),
        ("wrong-dialect", |project| {
            project.source_options.edi_kind = Some(EdiBoundaryKind::X12)
        }),
        ("missing-autocomplete", |project| {
            project.source_options.edi_autocomplete = None
        }),
        ("wrong-autocomplete", |project| {
            project.source_options.edi_autocomplete = Some(mapping::EdiAutocomplete::Hl7)
        }),
    ];
    for (name, mutate) in mutations {
        let mut project = base.clone();
        mutate(&mut project);
        let output_dir = dir.0.join(name);
        let output = output_dir.join("design.mfd");
        assert!(mfd::export(&project, &output).is_err(), "{name}");
        assert!(!output_dir.exists(), "{name} published artifacts");
    }
}

#[test]
#[ignore = "needs the local ignored ReferenceSamples corpus; informational only"]
fn local_idoc_order_preserves_native_text_settings_after_relocation() {
    let sample =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples/IDoc_Order.mfd");
    if !sample.is_file() {
        return;
    }
    let imported = mfd::import(&sample).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let settings = imported
        .project
        .source_options
        .idoc_native_text_settings
        .as_ref()
        .unwrap()
        .clone();

    let dir = TempDir::new();
    let bundle = dir.0.join("bundle");
    let output = bundle.join("portable.mfd");
    let warnings = mfd::export(&imported.project, &output).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let xml = std::fs::read_to_string(&output).unwrap();
    let (document, id) = idoc_text(&xml);
    let config = document.get_node(id).unwrap().attribute("config").unwrap();
    assert!(config.ends_with(".idoc-config.txt"));
    assert!(
        Path::new(config)
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
    );
    assert!(bundle.join(config).is_file());

    let moved = dir.0.join("relocated");
    std::fs::rename(&bundle, &moved).unwrap();
    let reimported = mfd::import(&moved.join("portable.mfd")).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(
        reimported.project.source_options.idoc_native_text_settings,
        Some(settings)
    );
    let native_output = moved.join("native-rejected.mfd");
    let report = mfd::preflight_export(&reimported.project, &native_output).unwrap();
    assert!(!report.is_native_compatible());
    assert!(matches!(
        mfd::export_with_profile(
            &reimported.project,
            &native_output,
            ExportProfile::NativeMfd
        ),
        Err(mfd::MfdError::IncompatibleExport(_))
    ));
    assert!(!native_output.exists());
}
