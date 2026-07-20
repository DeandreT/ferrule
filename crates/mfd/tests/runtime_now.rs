use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::{Node, RuntimeValue};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_runtime_now_{}_{}",
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

fn write_design(path: &Path, name: &str, library: &str) {
    let design = format!(
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Value" outkey="10"/></entry></root><document inputinstance="source.xml" instanceroot="{{}}Source"/></data></component>
          <component name="{name}" library="{library}" kind="5"><targets><datapoint pos="0" key="20"/></targets></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="First" inpkey="30"/><entry name="Second" inpkey="31"/></entry></root><document outputinstance="target.xml" instanceroot="{{}}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="20"><edges><edge vertexkey="30"/><edge vertexkey="31"/></edges></vertex></vertices></graph></structure></component></mapping>"#
    );
    std::fs::write(path, design).unwrap();
}

fn run(project: &mapping::Project) -> Instance {
    let execution = engine::ExecutionContext::new(Path::new("/maps/main.ferrule.json"))
        .with_current_datetime("2026-07-12T12:01:02.345-07:00");
    engine::run_with_context(project, &Instance::Group(Vec::new()), &execution).unwrap()
}

fn assert_now(output: &Instance) {
    for field in ["First", "Second"] {
        assert_eq!(
            output.field(field).and_then(Instance::as_scalar),
            Some(&Value::String("2026-07-12T12:01:02.345-07:00".into()))
        );
    }
}

#[test]
fn now_imports_as_one_stable_runtime_value_and_round_trips() {
    let dir = TempDir::new();
    let design = dir.0.join("now.mfd");
    write_design(&design, "now", "lang");

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported
            .project
            .graph
            .nodes
            .values()
            .filter(|node| matches!(
                node,
                Node::RuntimeValue {
                    value: RuntimeValue::CurrentDateTime
                }
            ))
            .count(),
        1
    );
    assert_now(&run(&imported.project));

    let exported = dir.0.join("round-trip.mfd");
    assert!(
        mfd::export(&imported.project, &exported)
            .unwrap()
            .is_empty()
    );
    let reimported = mfd::import(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_now(&run(&reimported.project));
}

#[test]
fn xpath2_current_datetime_uses_the_stable_execution_clock() {
    let dir = TempDir::new();
    let design = dir.0.join("current-datetime.mfd");
    write_design(&design, "current-dateTime", "xpath2");

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_now(&run(&imported.project));
}
