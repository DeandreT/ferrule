use super::*;
use crate::NamedSourceProgram;

fn collection_find(collection: &[&str], predicate: u32, value: u32) -> Expression {
    Expression::CollectionFind {
        collection: collection
            .iter()
            .map(|segment| (*segment).to_string())
            .collect(),
        predicate,
        value,
    }
}

#[test]
fn validates_collection_find_dependencies_and_cycles() {
    let mut program = program();
    program.expressions[1].expression = collection_find(&["Rows"], 99, 1);
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::MissingDependency {
            node: 2,
            dependency: 99,
        })
    );

    program.expressions[1].expression = collection_find(&["Rows"], 1, 99);
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::MissingDependency {
            node: 2,
            dependency: 99,
        })
    );

    program.expressions[1].expression = collection_find(&["Rows"], 1, 2);
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::ExpressionCycle { cycle: vec![2, 2] })
    );
}

#[test]
fn validates_flattened_primary_and_named_source_paths() {
    let mut program = program();
    program.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Departments",
                vec![SchemaNode::group("People", Vec::new()).repeating()],
            )
            .repeating(),
            SchemaNode::group("Singleton", Vec::new()),
        ],
    );
    program.extra_sources.push(NamedSourceProgram {
        name: "Catalog".into(),
        source: SchemaNode::group(
            "CatalogDocument",
            vec![SchemaNode::group("Items", Vec::new()).repeating()],
        ),
    });

    for collection in [
        vec!["Departments", "People"],
        vec!["Singleton"],
        vec!["Catalog", "Items"],
        Vec::new(),
    ] {
        program.expressions[1].expression = collection_find(&collection, 1, 1);
        assert_eq!(validate_program(&program), Ok(()), "{collection:?}");
    }

    program.expressions[1].expression = collection_find(&["Missing"], 1, 1);
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidCollectionFindCollection {
            node: 2,
            collection: vec!["Missing".into()],
        })
    );
}
