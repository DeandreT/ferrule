use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use mapping::{FormatOptions, Json5InterchangeDependency, Project};
use mfd::{ImportOptions, ImportProfile, MfdError};

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-json5-interchange-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::copy(
            fixtures().join("json5-boundary.schema.json"),
            path.join("json5-boundary.schema.json"),
        )
        .unwrap();
        Self(path)
    }

    fn design(&self, xml: &str) -> PathBuf {
        let path = self.0.join("design.mfd");
        std::fs::write(&path, xml).unwrap();
        path
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn options(project: &Project, boundary: usize) -> &FormatOptions {
    match boundary {
        0 => &project.source_options,
        1 => &project.extra_sources[0].options,
        2 => &project.target_options,
        3 => &project.extra_targets[0].options,
        _ => unreachable!(),
    }
}

fn path(project: &Project, boundary: usize) -> Option<&str> {
    match boundary {
        0 => project.source_path.as_deref(),
        1 => Some(project.extra_sources[0].path.as_str()),
        2 => project.target_path.as_deref(),
        3 => project.extra_targets[0].path.as_deref(),
        _ => unreachable!(),
    }
}

#[test]
fn component_settings_and_extensions_remain_distinct_on_every_boundary() {
    let original = std::fs::read_to_string(fixtures().join("json5-boundary.mfd")).unwrap();
    for (boundary, role, name) in [
        (0, "inputinstance", "source"),
        (1, "inputinstance", "auxiliary"),
        (2, "outputinstance", "target"),
        (3, "outputinstance", "additional"),
    ] {
        for setting in [
            None,
            Some("1"),
            Some("0"),
            Some("true"),
            Some("false"),
            Some(""),
            Some("invalid"),
        ] {
            for extension in ["json", "JSON5"] {
                for nested in [false, true] {
                    let directory = Directory::new();
                    let filename = format!("{name}.{extension}");
                    let setting_attr = setting
                        .map(|value| format!(" json5=\"{value}\""))
                        .unwrap_or_default();
                    // Place a modern file node in the existing component root, not a second root.
                    let old = format!(
                        "<json schema=\"json5-boundary.schema.json\" {role}=\"{name}.json\"/>"
                    );
                    let xml = if nested {
                        let component = original.split(&old).next().unwrap();
                        let root_end = component.rfind("</root>").unwrap();
                        let mut xml = original.clone();
                        xml.replace_range(
                            root_end..root_end,
                            &format!("<file role=\"{role}\" name=\"{filename}\"/>"),
                        );
                        xml.replace(
                            &old,
                            &format!("<json schema=\"json5-boundary.schema.json\"{setting_attr}/>"),
                        )
                    } else {
                        original.replace(&old, &format!(
                            "<json schema=\"json5-boundary.schema.json\" {role}=\"{filename}\"{setting_attr}/>"
                        ))
                    };
                    let design = directory.design(&xml);
                    let imported = mfd::import(&design).unwrap();
                    let project = &imported.project;
                    assert_eq!(project.extra_sources.len(), 1);
                    assert_eq!(project.extra_targets.len(), 1);
                    assert_eq!(path(project, boundary), Some(filename.as_str()));
                    assert!(!options(project, boundary).json5);
                    let expected = setting
                        .map(|value| Json5InterchangeDependency::ComponentSetting {
                            value: value.into(),
                        })
                        .or_else(|| {
                            (extension == "JSON5")
                                .then_some(Json5InterchangeDependency::InstanceExtension {})
                        });
                    assert_eq!(
                        options(project, boundary).json5_interchange_dependency,
                        expected
                    );
                    let restored = mapping::project_file::decode_str(
                        &mapping::project_file::encode_pretty(project).unwrap(),
                    )
                    .unwrap();
                    assert_eq!(
                        serde_json::to_value(&restored).unwrap(),
                        serde_json::to_value(project).unwrap()
                    );
                    let strict = mfd::import_with_profile(
                        &design,
                        &ImportOptions::default(),
                        ImportProfile::Executable,
                    );
                    if expected.is_some() {
                        assert!(imported.warnings.iter().any(|warning| {
                            warning.contains("native JSON5 interchange is not executable")
                        }));
                        assert!(!engine::validate(&restored).is_empty());
                        assert!(matches!(strict, Err(MfdError::IncompatibleImport(_))));
                        let destination = directory.0.join("saved.mfd");
                        let sibling = directory.0.join("saved-source.schema.json");
                        std::fs::write(&destination, "existing design").unwrap();
                        std::fs::write(&sibling, "existing schema").unwrap();
                        assert!(mfd::export(&restored, &destination).is_err());
                        assert_eq!(
                            std::fs::read_to_string(&destination).unwrap(),
                            "existing design"
                        );
                        assert_eq!(
                            std::fs::read_to_string(&sibling).unwrap(),
                            "existing schema"
                        );
                    } else {
                        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
                        assert!(strict.is_ok());
                    }
                }
            }
        }
    }
}

#[test]
fn legacy_paths_take_precedence_over_nested_paths_without_inventing_a_setting() {
    let original = std::fs::read_to_string(fixtures().join("json5-boundary.mfd")).unwrap();
    let xml = original.replacen(
        "</root>",
        "<file role=\"inputinstance\" name=\"ignored.json5\"/></root>",
        1,
    );
    let directory = Directory::new();
    let imported = mfd::import(&directory.design(&xml)).unwrap();
    assert_eq!(imported.project.source_path.as_deref(), Some("source.json"));
    assert_eq!(
        imported.project.source_options.json5_interchange_dependency,
        None
    );
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
}
