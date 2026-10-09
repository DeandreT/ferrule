use ir::{ScalarType, SchemaNode};
use mfd::{ImportOptions, ImportProfile};

#[path = "../../format-db/tests/fixtures/readonly_fixture.rs"]
mod fixture;
use fixture::Fixture;

fn design(typed: bool) -> String {
    let id = if typed { " datatype=\"integer\"" } else { "" };
    let label = if typed { " datatype=\"string\"" } else { "" };
    format!(
        r#"<mapping version="26">
  <resources><datasources><datasource name="authored">
    <database_connection name="authored" database_kind="SQLite" import_kind="SQLite" ConnectionString="database.sqlite"/>
  </datasource></datasources></resources>
  <component name="map"><structure><children>
    <component name="database" library="db" kind="15"><data>
      <root><entry name="document"><entry name="Records" type="table" outkey="10">
        <entry name="Id"{id} outkey="11"/><entry name="Label"{label} outkey="12"/>
      </entry></entry></root><database ref="authored"/>
    </data></component>
    <component name="rows" library="text" kind="16"><properties XSLTDefaultOutput="1"/><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="Rows" inpkey="50">
        <entry name="Id" inpkey="51"/><entry name="Label" inpkey="52"/>
      </entry></entry></entry></root>
      <text type="csv"><settings separator="," quote="&quot;" firstrownames="true">
        <names root="rows" block="Rows"><field0 name="Id" type="integer"/><field1 name="Label" type="string"/></names>
      </settings></text>
    </data></component>
  </children><connections><edge from="10" to="50"/><edge from="11" to="51"/><edge from="12" to="52"/></connections></structure></component>
</mapping>"#
    )
}

fn selected_schema(typed: bool) -> SchemaNode {
    SchemaNode::group(
        "Records",
        vec![
            SchemaNode::scalar(
                "Id",
                if typed {
                    ScalarType::Int
                } else {
                    ScalarType::String
                },
            ),
            SchemaNode::scalar("Label", ScalarType::String),
        ],
    )
    .repeating()
}

fn physical_schema() -> SchemaNode {
    SchemaNode::group(
        "Records",
        vec![
            SchemaNode::scalar("Id", ScalarType::Int),
            SchemaNode::scalar("GroupId", ScalarType::Int),
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Score", ScalarType::Float),
            SchemaNode::scalar("Active", ScalarType::Bool),
        ],
    )
    .repeating()
}

fn import_original(fixture: &Fixture, typed: bool) -> Result<mfd::Imported, mfd::MfdError> {
    let path = fixture.directory().join("design.mfd");
    std::fs::write(&path, design(typed)).unwrap();
    let result = mfd::import(&path);
    let retained = result
        .as_ref()
        .map(|value| (&value.project, &value.warnings, &value.mapping_path));
    fixture.original("import.original.txt", &retained);
    if let Ok(value) = &result {
        fixture.retain(
            "project.original.json",
            serde_json::to_vec_pretty(&value.project).unwrap(),
        );
    }
    result
}

#[test]
fn mfd_embedded_types_keep_successful_physical_metadata_and_extra_columns() {
    let fixture = Fixture::new("mfd-clean");
    fixture.retain(
        "expected.txt",
        format!(
            "source={:#?}\nNo import warnings; database bytes unchanged.\n",
            physical_schema()
        ),
    );
    fixture.author_database();
    let before = fixture.snapshot("before");
    let result = import_original(&fixture, true);
    let after = fixture.snapshot("after");
    let imported = result.unwrap();
    assert_eq!(imported.project.source, physical_schema());
    assert!(imported.warnings.is_empty(), "{:#?}", imported.warnings);
    assert_eq!(after.get("database.sqlite"), before.get("database.sqlite"));
    assert_eq!(after.len(), before.len() + 1);
}

#[test]
fn mfd_missing_database_preserves_typed_and_untyped_fallback_without_creation() {
    for typed in [false, true] {
        let fixture = Fixture::new(if typed {
            "mfd-missing-typed"
        } else {
            "mfd-missing-untyped"
        });
        fixture.retain("expected.txt", format!("source={:#?}\nNo database created; typed fallback has no resolver warning, untyped fallback retains a resolver warning.\n", selected_schema(typed)));
        let result = import_original(&fixture, typed);
        let after = fixture.snapshot("after");
        let imported = result.unwrap();
        assert_eq!(imported.project.source, selected_schema(typed));
        assert_eq!(
            imported.warnings.is_empty(),
            typed,
            "{:#?}",
            imported.warnings
        );
        assert!(!fixture.database().exists());
        assert_eq!(after.len(), 1);
    }
}

#[test]
fn mfd_hot_journal_retains_typed_fallback_and_strict_refusal_without_recovery() {
    let fixture = Fixture::new("mfd-hot");
    fixture.retain("expected.txt", format!("source={:#?}\nIntrospection warning remains; Executable import refuses. Database and journal bytes are unchanged.\n", selected_schema(true)));
    fixture.author_hot_journal();
    let before = fixture.snapshot("before");
    let result = import_original(&fixture, true);
    let strict = mfd::import_with_profile(
        &fixture.directory().join("design.mfd"),
        &ImportOptions::default(),
        ImportProfile::Executable,
    )
    .map(|value| value.report);
    fixture.original("strict.original.txt", &strict);
    let after = fixture.snapshot("after");
    let imported = result.unwrap();
    assert_eq!(imported.project.source, selected_schema(true));
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("could not introspect")),
        "{:#?}",
        imported.warnings
    );
    assert!(matches!(strict, Err(mfd::MfdError::IncompatibleImport(_))));
    assert_eq!(after.get("database.sqlite"), before.get("database.sqlite"));
    assert_eq!(
        after.get("database.sqlite-journal"),
        before.get("database.sqlite-journal")
    );
    assert_eq!(after.len(), before.len() + 1);
}
