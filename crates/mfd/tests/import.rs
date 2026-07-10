use std::path::{Path, PathBuf};

use ir::{Instance, ScalarType, SchemaKind, Value};
use mapping::Node;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A scratch dir for export roundtrips, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ferrule_mfd_{tag}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scalar(instance: &Instance, field: &str) -> Value {
    instance
        .field(field)
        .and_then(Instance::as_scalar)
        .cloned()
        .unwrap_or_else(|| panic!("no scalar field `{field}`"))
}

#[test]
fn imports_schemas_scopes_and_functions() {
    let imported = mfd::import(&fixture("people.mfd")).unwrap();
    let project = &imported.project;

    // Schemas come from the referenced XSDs (typed, repeating).
    assert_eq!(project.source.name, "Company");
    assert!(project.source.child("Staff").unwrap().repeating);
    assert_eq!(project.target.name, "People");
    assert!(project.target.child("Person").unwrap().repeating);

    // The Staff -> Person repeating connection becomes a scope.
    assert_eq!(project.root.children.len(), 1);
    let person = &project.root.children[0];
    assert_eq!(person.target_field, "Person");
    assert_eq!(person.source, Some(vec!["Staff".to_string()]));

    // Name <- concat(First, " ", Last); Age <- Age.
    assert_eq!(person.bindings.len(), 2);
    let name_binding = person
        .bindings
        .iter()
        .find(|b| b.target_field == "Name")
        .unwrap();
    let Node::Call { function, args } = &project.graph.nodes[&name_binding.node] else {
        panic!("Name should be bound to a call");
    };
    assert_eq!(function, "concat");
    assert_eq!(args.len(), 3);
    assert!(matches!(
        &project.graph.nodes[&args[0]],
        Node::SourceField { path } if path == &["First"]
    ));
    assert!(matches!(
        &project.graph.nodes[&args[1]],
        Node::Const { value: Value::String(s) } if s == " "
    ));

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
}

#[test]
fn imported_project_runs() {
    let imported = mfd::import(&fixture("people.mfd")).unwrap();
    let source = format_xml::read(&fixture("people.xml"), &imported.project.source).unwrap();
    let target = engine::run(&imported.project, &source).unwrap();

    let people = target
        .field("Person")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(people.len(), 2);
    assert_eq!(
        people[0].field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("Alice Carter".into()))
    );
    assert_eq!(
        people[1].field("Age").and_then(Instance::as_scalar),
        Some(&Value::Int(41))
    );
}

#[test]
fn xsd_includes_supply_component_schemas_and_the_project_runs() {
    let imported = mfd::import(&fixture("includes.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    let item = project.source.child("Item").unwrap();
    assert!(item.repeating);
    assert!(matches!(
        item.child("Qty").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
    let line = project.target.child("Line").unwrap();
    assert!(line.repeating);
    assert!(matches!(
        line.child("Amount").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));

    let source = format_xml::read(&fixture("includes.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let lines = target
        .field("Line")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(scalar(&lines[0], "Code"), Value::String("A-10".into()));
    assert_eq!(scalar(&lines[1], "Amount"), Value::Int(7));
}

#[test]
fn export_then_import_roundtrips_semantically() {
    let imported = mfd::import(&fixture("people.mfd")).unwrap();
    let dir = std::env::temp_dir().join(format!("ferrule_mfd_roundtrip_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("people.mfd");

    let warnings = mfd::export(&imported.project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = mfd::import(&out).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();

    let a = &imported.project;
    let b = &reimported.project;
    assert_eq!(a.source, b.source);
    assert_eq!(a.target, b.target);
    // Scope shape survives.
    assert_eq!(b.root.children.len(), 1);
    assert_eq!(b.root.children[0].source, a.root.children[0].source);
    assert_eq!(
        b.root.children[0].bindings.len(),
        a.root.children[0].bindings.len()
    );
    // The reimported project must still run and produce the same output.
    let source = format_xml::read(&fixture("people.xml"), &b.source).unwrap();
    let out_a = engine::run(a, &source).unwrap();
    let out_b = engine::run(b, &source).unwrap();
    assert_eq!(out_a, out_b);
}

#[test]
fn xml_attributes_import_run_and_roundtrip() {
    let imported = mfd::import(&fixture("books.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    let book = project.source.child("Book").unwrap();
    assert!(book.repeating);
    assert!(book.child("isbn").unwrap().attribute);
    assert!(book.child("pages").unwrap().attribute);
    assert!(matches!(
        book.child("pages").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
    assert!(!book.child("Title").unwrap().attribute);
    assert!(
        project
            .target
            .child("Entry")
            .unwrap()
            .child("id")
            .unwrap()
            .attribute
    );

    let source = format_xml::read(&fixture("books.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let entries = target
        .field("Entry")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(scalar(&entries[0], "id"), Value::String("978-1".into()));
    assert_eq!(scalar(&entries[0], "Name"), Value::String("Systems".into()));
    assert_eq!(scalar(&entries[1], "Pages"), Value::Int(180));

    let dir = TempDir::new("books");
    let out = dir.0.join("books.mfd");
    let warnings = mfd::export(project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = mfd::import(&out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(project.source, reimported.project.source);
    assert_eq!(project.target, reimported.project.target);
    // Binding order may differ (the exporter keys ports in schema order),
    // so compare the written documents, whose field order the schema fixes.
    let out_b = engine::run(&reimported.project, &source).unwrap();
    let write = |name: &str, instance: &Instance| {
        let path = dir.0.join(name);
        format_xml::write(&path, &project.target, instance).unwrap();
        std::fs::read_to_string(path).unwrap()
    };
    assert_eq!(write("a.xml", &target), write("b.xml", &out_b));
}

#[test]
fn xml_simple_content_imports_runs_and_roundtrips() {
    let imported = mfd::import(&fixture("simple-content.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    let source_price = project
        .source
        .child("Item")
        .unwrap()
        .child("Price")
        .unwrap();
    let source_text = source_price.child(ir::XML_TEXT_FIELD).unwrap();
    assert!(source_text.text);
    assert!(matches!(
        source_text.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Float
        }
    ));
    assert!(source_price.child("currency").unwrap().attribute);

    let source = format_xml::read(&fixture("simple-content.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let entries = target
        .field("Entry")
        .and_then(Instance::as_repeated)
        .unwrap();
    let amount = entries[1].field("Amount").unwrap();
    assert_eq!(scalar(amount, ir::XML_TEXT_FIELD), Value::Float(8.75));
    assert_eq!(scalar(amount, "currency"), Value::String("EUR".into()));

    let dir = TempDir::new("simple_content");
    let xml_out = dir.0.join("prices.xml");
    format_xml::write(&xml_out, &project.target, &target).unwrap();
    let xml = std::fs::read_to_string(&xml_out).unwrap();
    assert!(xml.contains("<Amount currency=\"USD\">12.5</Amount>"));

    let mfd_out = dir.0.join("prices.mfd");
    let warnings = mfd::export(project, &mfd_out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported = std::fs::read_to_string(&mfd_out).unwrap();
    assert!(!exported.contains("name=\"#text\""));
    let reimported = mfd::import(&mfd_out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(reimported.project.source, project.source);
    assert_eq!(reimported.project.target, project.target);
    let rerun = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(rerun, target);
}

#[test]
fn xml_to_json_with_ref_schema_imports_runs_and_roundtrips() {
    let imported = mfd::import(&fixture("stock.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    // The JSON Schema resolves through its root-level and nested $refs.
    assert_eq!(project.target.name, "Stock");
    assert!(project.target.repeating);
    let batches = project.target.child("batches").unwrap();
    assert!(batches.repeating);
    assert!(batches.child("code").is_some());
    assert_eq!(project.target_path.as_deref(), Some("stock-out.json"));

    // Row iteration lands on the root scope; batches nest inside it.
    assert_eq!(project.root.source, Some(vec!["Item".to_string()]));
    let batches_scope = &project.root.children[0];
    assert_eq!(batches_scope.target_field, "batches");
    assert_eq!(batches_scope.source, Some(vec!["Batch".to_string()]));

    let source = format_xml::read(&fixture("stock.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let rows = target.as_repeated().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(scalar(&rows[0], "sku"), Value::String("A1".into()));
    assert_eq!(scalar(&rows[0], "qty"), Value::Int(4));
    let batches = rows[0]
        .field("batches")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(batches.len(), 2);
    assert_eq!(scalar(&batches[1], "code"), Value::String("B2".into()));

    let dir = TempDir::new("stock");
    let out = dir.0.join("stock.mfd");
    let warnings = mfd::export(project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(dir.0.join("stock-target.schema.json").exists());
    let reimported = mfd::import(&out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(project.source, reimported.project.source);
    assert_eq!(project.target, reimported.project.target);
    let out_b = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(target, out_b);
}

#[test]
fn json_source_designs_import_and_run() {
    let imported = mfd::import(&fixture("inventory.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    assert_eq!(project.source.name, "Inventory");
    assert_eq!(project.source_path.as_deref(), Some("inventory.json"));
    assert!(project.source.child("items").unwrap().repeating);

    let line = &project.root.children[0];
    assert_eq!(line.target_field, "Line");
    assert_eq!(line.source, Some(vec!["items".to_string()]));

    let source = format_json::read(&fixture("inventory.json"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    assert_eq!(scalar(&target, "Store"), Value::String("Downtown".into()));
    let lines = target
        .field("Line")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(scalar(&lines[1], "Product"), Value::String("Gadget".into()));
    assert_eq!(scalar(&lines[1], "Count"), Value::Int(3));
}

#[test]
fn json_components_without_schema_fall_back_to_the_entry_tree() {
    let imported = mfd::import(&fixture("noschema-json.mfd")).unwrap();
    assert!(
        imported
            .warnings
            .iter()
            .any(|w| w.contains("no schema reference")),
        "{:?}",
        imported.warnings
    );
    let source = &imported.project.source;
    assert_eq!(source.name, "orders");
    assert!(matches!(
        source.child("customer").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    ));
    assert!(matches!(
        source.child("total").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Float
        }
    ));
}

#[test]
fn csv_source_designs_import_and_run() {
    let imported = mfd::import(&fixture("people-csv.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    assert_eq!(project.source.name, "Staff");
    assert!(!project.source.repeating);
    assert!(matches!(
        project.source.child("Age").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
    assert_eq!(project.source_path.as_deref(), Some("people.csv"));
    assert_eq!(project.source_options.delimiter, Some(','));
    assert_eq!(project.source_options.has_header_row, Some(true));

    // The row block feeds the Person iteration; rows arrive as the
    // enclosing Repeated, so the scope path is empty.
    let person = &project.root.children[0];
    assert_eq!(person.target_field, "Person");
    assert_eq!(person.source, Some(vec![]));

    let rows = format_csv::read(&fixture("people.csv"), &project.source, Some(','), true).unwrap();
    let target = engine::run(project, &Instance::Repeated(rows)).unwrap();
    let people = target
        .field("Person")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(people.len(), 2);
    assert_eq!(
        scalar(&people[0], "Name"),
        Value::String("Alice Carter".into())
    );
    assert_eq!(scalar(&people[1], "Age"), Value::Int(41));
}

#[test]
fn csv_target_designs_import_run_and_roundtrip() {
    let imported = mfd::import(&fixture("people-to-csv.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    assert_eq!(project.target.name, "PeopleRows");
    assert_eq!(project.target_path.as_deref(), Some("people-out.csv"));
    assert_eq!(project.target_options.delimiter, Some(';'));
    assert_eq!(project.target_options.has_header_row, Some(false));

    // Rows iterate on the root scope itself.
    assert_eq!(project.root.source, Some(vec!["Staff".to_string()]));

    let source = format_xml::read(&fixture("people.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let rows = target.as_repeated().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        scalar(&rows[0], "Name"),
        Value::String("Alice Carter".into())
    );
    assert_eq!(scalar(&rows[1], "Age"), Value::Int(41));

    let dir = TempDir::new("people_to_csv");
    let out = dir.0.join("people-to-csv.mfd");
    let warnings = mfd::export(project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = mfd::import(&out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(project.target, reimported.project.target);
    assert_eq!(reimported.project.target_options.delimiter, Some(';'));
    assert_eq!(
        reimported.project.target_options.has_header_row,
        Some(false)
    );
    let out_b = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(target, out_b);
}

#[test]
fn db_target_designs_import_run_and_roundtrip() {
    // Stage the design in a scratch dir with a typed (empty) SQLite table
    // next to it, so the importer's introspection path is exercised
    // without a binary fixture in the repo.
    let dir = TempDir::new("people_to_db");
    for f in ["people-to-db.mfd", "people-source.xsd", "people.xml"] {
        std::fs::copy(fixture(f), dir.0.join(f)).unwrap();
    }
    let table = ir::SchemaNode::group(
        "People",
        vec![
            ir::SchemaNode::scalar("Name", ScalarType::String),
            ir::SchemaNode::scalar("Age", ScalarType::Int),
        ],
    )
    .repeating();
    let db_path = dir.0.join("people-out.sqlite");
    format_db::write(&db_path, &table, &[]).unwrap();

    let imported = mfd::import(&dir.0.join("people-to-db.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    // Schema came from introspecting the SQLite file (typed).
    assert_eq!(project.target, table);
    assert_eq!(project.target_path.as_deref(), Some("people-out.sqlite"));
    // Rows iterate on the root scope, like the other flat-rows formats.
    assert_eq!(project.root.source, Some(vec!["Staff".to_string()]));

    let source = format_xml::read(&fixture("people.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let rows = target.as_repeated().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        scalar(&rows[0], "Name"),
        Value::String("Alice Carter".into())
    );

    // The rows actually land in (and read back from) the database.
    format_db::write(&db_path, &project.target, rows).unwrap();
    let read_back = format_db::read(&db_path, &project.target).unwrap();
    assert_eq!(read_back.len(), 2);
    assert_eq!(scalar(&read_back[1], "Age"), Value::Int(41));

    // Export emits a db component + datasource; reimport is faithful.
    let out = dir.0.join("people-to-db-2.mfd");
    let warnings = mfd::export(project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.contains("library=\"db\""), "{text}");
    assert!(text.contains("database_connection"), "{text}");
    let reimported = mfd::import(&out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(project.target, reimported.project.target);
    assert_eq!(
        reimported.project.target_path.as_deref(),
        Some("people-out.sqlite")
    );
    let out_b = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(target, out_b);
}

#[test]
fn aggregate_designs_import_run_and_roundtrip() {
    let imported = mfd::import(&fixture("orders.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    // count(Item) and sum(Item/Price) evaluate inside the Order scope;
    // string-join(Order, Id, ", ") evaluates at the root, so its
    // collection keeps the Order segment.
    let order_scope = &project.root.children[0];
    assert_eq!(order_scope.target_field, "Order");
    let count_binding = order_scope
        .bindings
        .iter()
        .find(|b| b.target_field == "ItemCount")
        .unwrap();
    assert!(matches!(
        &project.graph.nodes[&count_binding.node],
        Node::Aggregate { function: mapping::AggregateOp::Count, collection, value, .. }
            if collection == &["Item"] && value.is_empty()
    ));
    let total_binding = order_scope
        .bindings
        .iter()
        .find(|b| b.target_field == "Total")
        .unwrap();
    assert!(matches!(
        &project.graph.nodes[&total_binding.node],
        Node::Aggregate { function: mapping::AggregateOp::Sum, collection, value, .. }
            if collection == &["Item"] && value == &["Price"]
    ));
    let doubled_binding = order_scope
        .bindings
        .iter()
        .find(|b| b.target_field == "DoubledTotal")
        .unwrap();
    let doubled_expression = match &project.graph.nodes[&doubled_binding.node] {
        Node::Aggregate {
            function: mapping::AggregateOp::Sum,
            collection,
            value,
            expression: Some(expression),
            ..
        } if collection == &["Item"] && value.is_empty() => *expression,
        other => panic!("expected computed sum aggregate, got {other:?}"),
    };
    assert!(matches!(
        &project.graph.nodes[&doubled_expression],
        Node::Call { function, .. } if function == "multiply"
    ));
    let ids_binding = project
        .root
        .bindings
        .iter()
        .find(|b| b.target_field == "AllIds")
        .unwrap();
    assert!(matches!(
        &project.graph.nodes[&ids_binding.node],
        Node::Aggregate {
            function: mapping::AggregateOp::Join,
            collection,
            value,
            arg: Some(_),
            ..
        }
            if collection == &["Order"] && value == &["Id"]
    ));

    let source = format_xml::read(&fixture("orders.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    assert_eq!(scalar(&target, "AllIds"), Value::String("A-1, B-2".into()));
    let orders = target
        .field("Order")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(scalar(&orders[0], "ItemCount"), Value::Int(2));
    assert_eq!(scalar(&orders[0], "Total"), Value::Float(4.0));
    assert_eq!(scalar(&orders[0], "DoubledTotal"), Value::Float(8.0));
    assert_eq!(scalar(&orders[1], "ItemCount"), Value::Int(1));
    assert_eq!(scalar(&orders[1], "Total"), Value::Float(10.0));
    assert_eq!(scalar(&orders[1], "DoubledTotal"), Value::Float(20.0));

    let dir = TempDir::new("orders");
    let out = dir.0.join("orders.mfd");
    let warnings = mfd::export(project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = mfd::import(&out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let out_b = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(target, out_b);
}

#[test]
fn group_by_designs_import_run_and_roundtrip() {
    let imported = mfd::import(&fixture("temps.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    // The group-by component becomes the scope's grouping key; its key
    // output feeds the Year binding as the key expression itself.
    let stats = &project.root.children[0];
    assert_eq!(stats.target_field, "YearlyStats");
    assert_eq!(stats.source, Some(vec!["Row".to_string()]));
    let group_key = stats.group_by.expect("scope should group");
    assert!(matches!(
        &project.graph.nodes[&group_key],
        Node::Call { function, .. } if function == "substring_before"
    ));
    let year = stats
        .bindings
        .iter()
        .find(|b| b.target_field == "Year")
        .unwrap();
    assert_eq!(year.node, group_key);

    let source = format_xml::read(&fixture("temps.xml"), &project.source).unwrap();
    let target = engine::run(project, &source).unwrap();
    let years = target
        .field("YearlyStats")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(years.len(), 2);
    assert_eq!(scalar(&years[0], "Year"), Value::String("2024".into()));
    assert_eq!(scalar(&years[0], "MinTemp"), Value::Float(2.0));
    assert_eq!(scalar(&years[0], "MaxTemp"), Value::Float(22.0));
    assert_eq!(scalar(&years[0], "AvgTemp"), Value::Float(12.0));
    assert_eq!(scalar(&years[1], "Year"), Value::String("2025".into()));
    assert_eq!(scalar(&years[1], "AvgTemp"), Value::Float(4.0));

    let dir = TempDir::new("temps");
    let out = dir.0.join("temps.mfd");
    let warnings = mfd::export(project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = mfd::import(&out).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(reimported.project.root.children[0].group_by.is_some());
    let out_b = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(target, out_b);
}
