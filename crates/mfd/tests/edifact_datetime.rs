use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_edifact_datetime_{}_{}",
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

fn write_design(path: &Path) {
    std::fs::write(
        path,
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Date" outkey="10"/><entry name="Format" outkey="11"/></entry></entry></entry></root><document inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="to-datetime" library="edifact" kind="5"><sources><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/></sources><targets><datapoint pos="0" key="22"/></targets></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target"><entry name="Result" inpkey="30"/></entry></entry></entry></root><document outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex><vertex vertexkey="22"><edges><edge vertexkey="30"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    )
    .unwrap();
}

fn input() -> Instance {
    Instance::Group(vec![
        (
            "Date".to_string(),
            Instance::Scalar(Value::String("202402291305PDT".to_string())),
        ),
        (
            "Format".to_string(),
            Instance::Scalar(Value::String("303".to_string())),
        ),
    ])
}

fn assert_output(project: &mapping::Project) {
    let output = engine::run(project, &input()).unwrap();
    assert_eq!(
        output.field("Result").and_then(Instance::as_scalar),
        Some(&Value::String("2024-02-29T13:05:00-09:00".to_string()))
    );
}

#[test]
fn edifact_to_datetime_imports_executes_and_round_trips() {
    let dir = TempDir::new();
    let design = dir.0.join("edifact-datetime.mfd");
    write_design(&design);

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_output(&imported.project);

    let exported = dir.0.join("round-trip.mfd");
    assert!(
        mfd::export(&imported.project, &exported)
            .unwrap()
            .is_empty()
    );
    let xml = std::fs::read_to_string(&exported).unwrap();
    assert!(xml.contains("name=\"to-datetime\" library=\"edifact\""));

    let reimported = mfd::import(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_output(&reimported.project);
}
