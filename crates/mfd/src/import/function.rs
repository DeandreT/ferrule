use ir::{ScalarType, Value};
use mapping::AggregateOp;

use crate::canonical_function;

use super::schema::parse_u32;

/// Typed lookup data declared by a MapForce value-map component.
#[derive(Clone, Default)]
pub(super) struct ValueMapData {
    pub(super) table: Vec<(Value, Value)>,
    pub(super) default: Option<Value>,
    pub(super) input_type: Option<ScalarType>,
}

#[derive(Clone)]
pub(super) enum DbWhereComponent {
    Supported(DbWhere),
    Unsupported(String),
}

#[derive(Clone)]
pub(super) struct DbWhere {
    pub(super) predicate: DbPredicate,
    pub(super) order: Option<DbOrder>,
    pub(super) parameter_type: ScalarType,
}

#[derive(Clone)]
pub(super) struct DbPredicate {
    pub(super) column: Vec<String>,
    pub(super) operator: DbPredicateOperator,
}

#[derive(Clone, Copy)]
pub(super) enum DbPredicateOperator {
    Equal,
    Like,
}

#[derive(Clone)]
pub(super) struct DbOrder {
    pub(super) column: Vec<String>,
    pub(super) descending: bool,
}

pub(super) struct FnComponent {
    pub(super) library: String,
    pub(super) name: String,
    pub(super) kind: u32,
    /// Input pins in `pos` order; `None` for declared-but-keyless pins.
    pub(super) inputs: Vec<Option<u32>>,
    /// Output pin keys in `pos` order.
    pub(super) outputs: Vec<u32>,
    /// Declared output positions, retaining keyless pins so a secondary
    /// output cannot be mistaken for the primary result.
    pub(super) output_pins: Vec<Option<u32>>,
    /// Scalar coercion declared by a transparent kind=6 input parameter.
    pub(super) input_type: Option<ScalarType>,
    pub(super) constant: Option<(String, String)>,
    pub(super) valuemap: Option<ValueMapData>,
    /// Sort directions in key-index order; `Some` also identifies a sort
    /// component whose key declarations are malformed or absent.
    pub(super) sort_directions: Option<Vec<bool>>,
    pub(super) db_where: Option<DbWhereComponent>,
    pub(super) recursive: Option<RecursiveComponent>,
}

#[derive(Clone)]
pub(super) enum RecursiveComponent {
    Invalid,
    Collect {
        collection: Vec<String>,
        children: Vec<String>,
        descent_value: Vec<String>,
        values: Vec<String>,
        value: Vec<String>,
    },
    Filter {
        children: String,
        items: String,
    },
    PathHierarchy {
        collection: Vec<String>,
        separator: String,
        directories: String,
        files: String,
        name: String,
    },
    AdjacencyTree {
        collection: Vec<String>,
        key: Vec<String>,
        parent: Vec<String>,
        target_key: String,
        target_children: String,
        has_root: bool,
    },
}

pub(super) fn read(component: &roxmltree::Node) -> FnComponent {
    let library = component
        .attribute("library")
        .unwrap_or_default()
        .to_string();
    let mut name = component.attribute("name").unwrap_or_default().to_string();
    if library == "edifact" && name == "to-datetime" {
        name = "edifact-to-datetime".to_string();
    }
    let kind = parse_u32(component.attribute("kind")).unwrap_or(0);
    let pins = |tag: &str| -> Vec<Option<u32>> {
        component
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == tag)
            .map(|pins| {
                let datapoints: Vec<_> = pins
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "datapoint")
                    .collect();
                let mut ordered = Vec::new();
                for (index, datapoint) in datapoints.into_iter().enumerate() {
                    let position = parse_u32(datapoint.attribute("pos"))
                        .and_then(|position| usize::try_from(position).ok())
                        .filter(|position| *position < 64)
                        .unwrap_or(index);
                    if ordered.len() <= position {
                        ordered.resize(position + 1, None);
                    }
                    if ordered[position].is_none() {
                        ordered[position] = parse_u32(datapoint.attribute("key"));
                    }
                }
                ordered
            })
            .unwrap_or_default()
    };
    let inputs = pins("sources");
    let output_pins = pins("targets");
    let outputs: Vec<u32> = output_pins.iter().flatten().copied().collect();

    let data = component
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "data");
    let input_type = (library == "core" && kind == 6)
        .then(|| {
            data.and_then(|data| data.descendants().find(|node| node.has_tag_name("input")))
                .and_then(|input| {
                    input
                        .attribute("datatype")
                        .or_else(|| input.attribute("type"))
                })
                .map(parse_scalar_type)
        })
        .flatten();
    let constant = data
        .and_then(|d| {
            d.children()
                .find(|n| n.is_element() && n.tag_name().name() == "constant")
        })
        .map(|c| {
            (
                c.attribute("value").unwrap_or_default().to_string(),
                c.attribute("datatype").unwrap_or_default().to_string(),
            )
        });
    let valuemap = data
        .and_then(|d| {
            d.children()
                .find(|n| n.is_element() && n.tag_name().name() == "valuemap")
        })
        .map(|vm| {
            let input_type = vm
                .descendants()
                .find(|node| node.has_tag_name("input"))
                .and_then(|node| node.attribute("type"))
                .map(parse_scalar_type)
                .or(Some(ScalarType::String));
            let result_type = vm
                .descendants()
                .find(|node| node.has_tag_name("result"))
                .and_then(|node| node.attribute("type"))
                .map(parse_scalar_type)
                .unwrap_or(ScalarType::String);
            let table = vm
                .descendants()
                .filter(|n| n.has_tag_name("entry"))
                .map(|e| {
                    (
                        parse_constant(
                            e.attribute("from").unwrap_or_default(),
                            scalar_type_name(input_type.unwrap_or(ScalarType::String)),
                        ),
                        parse_constant(
                            e.attribute("to").unwrap_or_default(),
                            scalar_type_name(result_type),
                        ),
                    )
                })
                .collect();
            let default = vm
                .descendants()
                .find(|n| n.has_tag_name("result"))
                .and_then(|r| r.attribute("defaultValue"))
                .map(|value| parse_constant(value, scalar_type_name(result_type)))
                .filter(|_| {
                    vm.attribute("defaultValueMode") == Some("custom")
                        || vm.attribute("enableDefaultValue") == Some("1")
                });
            ValueMapData {
                table,
                default,
                input_type,
            }
        });
    let sort_directions = data
        .and_then(|data| data.descendants().find(|node| node.has_tag_name("sort")))
        .map(|sort| {
            let mut keys = sort
                .descendants()
                .filter(|node| node.has_tag_name("key"))
                .map(|key| {
                    let index = key
                        .attribute("index")
                        .and_then(|index| index.parse::<usize>().ok())
                        .unwrap_or(0);
                    (index, key.attribute("direction") == Some("descending"))
                })
                .collect::<Vec<_>>();
            keys.sort_by_key(|(index, _)| *index);
            keys.into_iter().map(|(_, descending)| descending).collect()
        });
    let db_where =
        (library == "db" && kind == 21).then(|| parse_db_where(component, &inputs, &outputs));

    FnComponent {
        library,
        name,
        kind,
        inputs,
        outputs,
        output_pins,
        input_type,
        constant,
        valuemap,
        sort_directions,
        db_where,
        recursive: None,
    }
}

fn parse_db_where(
    component: &roxmltree::Node,
    inputs: &[Option<u32>],
    outputs: &[u32],
) -> DbWhereComponent {
    let parse = || -> Result<DbWhere, &'static str> {
        if inputs.len() != 2 || inputs.iter().any(Option::is_none) || outputs.len() != 1 {
            return Err("expected one collection input, one parameter input, and one output");
        }
        let where_node = component
            .descendants()
            .find(|node| node.has_tag_name("where"))
            .ok_or("missing where metadata")?;
        let parameters = where_node
            .descendants()
            .filter(|node| node.has_tag_name("parameter"))
            .collect::<Vec<_>>();
        let [parameter] = parameters.as_slice() else {
            return Err("expected exactly one declared parameter");
        };
        let parameter_name = parameter
            .attribute("name")
            .filter(|name| valid_identifier_segment(name))
            .ok_or("parameter name is not a safe SQL identifier")?;
        let parameter_type = match parameter.attribute("type").unwrap_or_default() {
            "string" | "text" => ScalarType::String,
            _ => return Err("only string parameters are supported in database where controls"),
        };

        let condition = where_node
            .attribute("condition")
            .ok_or("missing where condition")?;
        let tokens = condition.split_ascii_whitespace().collect::<Vec<_>>();
        let [column, operator, bound] = tokens.as_slice() else {
            return Err("condition must be `Identifier (=|LIKE) :Parameter`");
        };
        let column =
            parse_identifier(column).ok_or("condition column is not a safe SQL identifier")?;
        let operator = if *operator == "=" {
            DbPredicateOperator::Equal
        } else if operator.eq_ignore_ascii_case("LIKE") {
            DbPredicateOperator::Like
        } else {
            return Err("condition operator must be `=` or `LIKE`");
        };
        if bound.strip_prefix(':') != Some(parameter_name) {
            return Err("condition parameter does not match its declaration");
        }

        let order = where_node
            .attribute("order")
            .filter(|order| !order.trim().is_empty())
            .map(parse_db_order)
            .transpose()?;
        Ok(DbWhere {
            predicate: DbPredicate { column, operator },
            order,
            parameter_type,
        })
    };
    match parse() {
        Ok(where_control) => DbWhereComponent::Supported(where_control),
        Err(reason) => DbWhereComponent::Unsupported(reason.to_string()),
    }
}

fn parse_db_order(order: &str) -> Result<DbOrder, &'static str> {
    let tokens = order.split_ascii_whitespace().collect::<Vec<_>>();
    let (column, descending) = match tokens.as_slice() {
        [column] => (*column, false),
        [column, direction] if direction.eq_ignore_ascii_case("ASC") => (*column, false),
        [column, direction] if direction.eq_ignore_ascii_case("DESC") => (*column, true),
        _ => return Err("order must contain one identifier with optional ASC or DESC"),
    };
    let column = parse_identifier(column).ok_or("order column is not a safe SQL identifier")?;
    Ok(DbOrder { column, descending })
}

fn parse_identifier(identifier: &str) -> Option<Vec<String>> {
    let segments = identifier.split('.').collect::<Vec<_>>();
    (!segments.is_empty()
        && segments
            .iter()
            .all(|segment| valid_identifier_segment(segment)))
    .then(|| segments.into_iter().map(str::to_string).collect())
}

fn valid_identifier_segment(segment: &str) -> bool {
    let mut bytes = segment.bytes();
    bytes
        .next()
        .is_some_and(|first| first == b'_' || first.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

pub(super) fn is_filter(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 3
}

pub(super) fn is_db_where(component: &FnComponent) -> bool {
    component.library == "db" && component.kind == 21 && component.db_where.is_some()
}

pub(super) fn is_db_function_component(component: &roxmltree::Node<'_, '_>) -> bool {
    component.attribute("library") == Some("db")
        && (component.attribute("kind") == Some("21")
            || component.attribute("kind") == Some("5")
                && matches!(
                    component.attribute("name"),
                    Some("substitute-null" | "is-null" | "is-not-null")
                ))
}

pub(super) fn is_xbrl_measure_component(component: &roxmltree::Node<'_, '_>) -> bool {
    component.attribute("library") == Some("xbrl")
        && component.attribute("kind") == Some("5")
        && matches!(
            component.attribute("name"),
            Some("xbrl-measure-currency" | "xbrl-measure-shares")
        )
}

pub(super) fn is_isbn_converter_component(component: &roxmltree::Node<'_, '_>) -> bool {
    component.attribute("library") == Some("IsbnConverterService")
        && component.attribute("kind") == Some("20")
        && matches!(
            component.attribute("name"),
            Some("convertToISBN13" | "convertToEAN")
        )
}

pub(super) fn read_isbn_converter_component(
    component: &roxmltree::Node<'_, '_>,
) -> Result<FnComponent, &'static str> {
    let mut function = read(component);
    if !function.inputs.is_empty() || !function.outputs.is_empty() {
        return (matches!(function.inputs.as_slice(), [Some(_)])
            && matches!(function.output_pins.as_slice(), [Some(_)]))
        .then_some(function)
        .ok_or("expected one scalar request input and one scalar response output");
    }
    let data = component
        .children()
        .find(|node| node.has_tag_name("data"))
        .ok_or("missing request and response entry trees")?;
    let inputs = data
        .descendants()
        .filter_map(|node| parse_u32(node.attribute("inpkey")))
        .collect::<Vec<_>>();
    let outputs = data
        .descendants()
        .filter_map(|node| parse_u32(node.attribute("outkey")))
        .collect::<Vec<_>>();
    let ([input], [output]) = (inputs.as_slice(), outputs.as_slice()) else {
        return Err("expected one scalar request input and one scalar response output");
    };
    function.inputs = vec![Some(*input)];
    function.outputs = vec![*output];
    function.output_pins = vec![Some(*output)];
    Ok(function)
}

pub(super) fn produces_scalar(component: &FnComponent) -> bool {
    component.name == "constant"
        || matches!(
            component.name.as_str(),
            "if-else"
                | "value-map"
                | "position"
                | "mfd-filepath"
                | "main-mfd-filepath"
                | "now"
                | "set-xsi-nil"
                | "auto-number"
        )
        || component.kind == 5
            && component.library == "db"
            && matches!(component.name.as_str(), "is-null" | "is-not-null")
        || component.kind == 5
            && component.library == "xbrl"
            && matches!(
                component.name.as_str(),
                "xbrl-measure-currency" | "xbrl-measure-shares"
            )
        || component.kind == 5 && aggregate_op(&component.name).is_some()
        || map_component_name(component).is_some()
}

pub(super) fn map_component_name(component: &FnComponent) -> Option<&str> {
    if component.library == "ferrule"
        && component.kind == 5
        && canonical_function::is_internal(&component.name)
    {
        Some(component.name.as_str())
    } else {
        map_name(&component.name)
    }
}

pub(super) fn is_input(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 6
}

pub(super) fn is_sort(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 30 && component.sort_directions.is_some()
}

pub(super) fn is_first_items(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 5 && component.name == "first-items"
}

pub(super) fn is_group_into_blocks(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 5 && component.name == "group-into-blocks"
}

pub(super) fn is_group_starting_with(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 5 && component.name == "group-starting-with"
}

pub(super) fn is_distinct_values(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 5 && component.name == "distinct-values"
}

pub(super) fn is_sequence_producer(component: &FnComponent) -> bool {
    component.kind == 5
        && ((component.library == "core"
            && matches!(
                component.name.as_str(),
                "tokenize" | "tokenize-by-length" | "generate-sequence"
            ))
            || (component.library == "ferrule"
                && matches!(
                    component.recursive,
                    Some(RecursiveComponent::Collect { .. })
                )))
}

pub(super) fn is_recursive_construction(component: &FnComponent) -> bool {
    component.library == "ferrule"
        && component.kind == 5
        && matches!(
            component.recursive,
            Some(
                RecursiveComponent::Filter { .. }
                    | RecursiveComponent::PathHierarchy { .. }
                    | RecursiveComponent::AdjacencyTree { .. }
                    | RecursiveComponent::Invalid
            )
        )
}

pub(super) fn parse_constant(value: &str, datatype: &str) -> Value {
    match datatype {
        "integer" | "int" | "long" => value.parse().map(Value::Int).unwrap_or(Value::Null),
        "decimal" | "double" | "float" => value.parse().map(Value::Float).unwrap_or(Value::Null),
        "boolean" => match value {
            "true" | "1" => Value::Bool(true),
            "false" | "0" => Value::Bool(false),
            _ => Value::Null,
        },
        _ => Value::String(value.to_string()),
    }
}

fn parse_scalar_type(datatype: &str) -> ScalarType {
    match datatype {
        "integer" | "int" | "long" => ScalarType::Int,
        "decimal" | "double" | "float" | "number" => ScalarType::Float,
        "boolean" => ScalarType::Bool,
        _ => ScalarType::String,
    }
}

fn scalar_type_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "decimal",
        ScalarType::Bool => "boolean",
    }
}

pub(super) fn aggregate_op(name: &str) -> Option<AggregateOp> {
    Some(match name {
        "count" => AggregateOp::Count,
        "sum" => AggregateOp::Sum,
        "avg" => AggregateOp::Avg,
        "min" => AggregateOp::Min,
        "max" => AggregateOp::Max,
        "string-join" => AggregateOp::Join,
        "item-at" => AggregateOp::ItemAt,
        _ => return None,
    })
}

pub(super) fn map_name(name: &str) -> Option<&'static str> {
    Some(match name {
        "concat" => "concat",
        "add" => "add",
        "subtract" => "subtract",
        "multiply" => "multiply",
        "divide" => "divide",
        "equal" => "equal",
        "not-equal" => "not_equal",
        "greater" => "greater_than",
        "less" => "less_than",
        "greater-equal" | "greater-or-equal" | "equal-or-greater" => "greater_or_equal",
        "less-equal" | "less-or-equal" | "equal-or-less" => "less_or_equal",
        "logical-and" => "and",
        "logical-or" => "or",
        "logical-not" => "not",
        "string-length" => "length",
        "contains" => "contains",
        "starts-with" => "starts_with",
        "upper-case" => "upper",
        "lower-case" => "lower",
        "string" => "string",
        "numeric" => "is_numeric",
        "format-number" => "format_number",
        "trim" => "trim",
        "left-trim" => "left_trim",
        "right-trim" => "right_trim",
        "pad-string-left" => "pad_string_left",
        "pad-string-right" => "pad_string_right",
        "substring" => "substring",
        "substring-before" => "substring_before",
        "substring-after" => "substring_after",
        "normalize-space" => "normalize_space",
        "empty" => "is_empty",
        "get-folder" => "get_folder",
        "remove-folder" => "remove_folder",
        "resolve-filepath" => "resolve_filepath",
        "is-xsi-nil" => "is_xml_nil",
        "exists" => "exists",
        "round" | "round-precision" => "round",
        "date-from-datetime" => "date_from_datetime",
        "year-from-datetime" => "year_from_datetime",
        "month-from-datetime" => "month_from_datetime",
        "day-from-datetime" => "day_from_datetime",
        "hour-from-datetime" | "hours-from-datetime" => "hours_from_datetime",
        "minute-from-datetime" | "minutes-from-datetime" => "minutes_from_datetime",
        "time-from-datetime" => "time_from_datetime",
        "datetime-from-date-and-time" => "datetime_from_date_and_time",
        "datetime-from-parts" => "datetime_from_parts",
        "datetime-add" => "datetime_add",
        "parse-date" => "parse_date",
        "parse-dateTime" => "parse_datetime",
        "parse-time" => "parse_time",
        "edifact-to-datetime" => "edifact_to_datetime",
        "substitute-missing" | "substitute-null" => "substitute_missing",
        "convertToISBN13" | "convertToEAN" => "isbn10_to_isbn13",
        "sleep" => "delay_passthrough",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use ir::{ScalarType, Value};

    use super::{
        is_db_function_component, is_isbn_converter_component, is_xbrl_measure_component,
        map_component_name, map_name, produces_scalar, read, read_isbn_converter_component,
    };

    #[test]
    fn scalar_names_use_canonical_ir_spelling() {
        assert_eq!(map_name("string"), Some("string"));
        assert_eq!(map_name("numeric"), Some("is_numeric"));
        assert_eq!(map_name("format-number"), Some("format_number"));
        assert_eq!(map_name("normalize-space"), Some("normalize_space"));
        assert_eq!(map_name("empty"), Some("is_empty"));
        assert_eq!(map_name("time-from-datetime"), Some("time_from_datetime"));
        assert_eq!(map_name("year-from-datetime"), Some("year_from_datetime"));
        assert_eq!(map_name("month-from-datetime"), Some("month_from_datetime"));
        assert_eq!(map_name("day-from-datetime"), Some("day_from_datetime"));
        assert_eq!(map_name("hours-from-datetime"), Some("hours_from_datetime"));
        assert_eq!(map_name("hour-from-datetime"), Some("hours_from_datetime"));
        assert_eq!(
            map_name("minutes-from-datetime"),
            Some("minutes_from_datetime")
        );
        assert_eq!(
            map_name("minute-from-datetime"),
            Some("minutes_from_datetime")
        );
        assert_eq!(
            map_name("datetime-from-date-and-time"),
            Some("datetime_from_date_and_time")
        );
        assert_eq!(map_name("datetime-from-parts"), Some("datetime_from_parts"));
        assert_eq!(map_name("datetime-add"), Some("datetime_add"));
        assert_eq!(map_name("parse-date"), Some("parse_date"));
        assert_eq!(map_name("parse-dateTime"), Some("parse_datetime"));
        assert_eq!(map_name("edifact-to-datetime"), Some("edifact_to_datetime"));
        assert_eq!(map_name("substitute-missing"), Some("substitute_missing"));
        assert_eq!(map_name("substitute-null"), Some("substitute_missing"));
        assert_eq!(map_name("convertToISBN13"), Some("isbn10_to_isbn13"));
        assert_eq!(map_name("convertToEAN"), Some("isbn10_to_isbn13"));
        assert_eq!(map_name("sleep"), Some("delay_passthrough"));
    }

    #[test]
    fn only_explicit_ferrule_components_map_internal_names() {
        let ferrule = roxmltree::Document::parse(
            r#"<component library="ferrule" name="to_number" kind="5"/>"#,
        )
        .unwrap();
        let ferrule = read(&ferrule.root_element());
        assert_eq!(map_component_name(&ferrule), Some("to_number"));
        assert!(produces_scalar(&ferrule));

        let vendor =
            roxmltree::Document::parse(r#"<component library="core" name="to_number" kind="5"/>"#)
                .unwrap();
        let vendor = read(&vendor.root_element());
        assert_eq!(map_component_name(&vendor), None);
        assert!(!produces_scalar(&vendor));
    }

    #[test]
    fn specialized_scalar_component_classifiers_are_exact() -> Result<(), Box<dyn std::error::Error>>
    {
        for (library, name, accepted) in [
            ("db", "is-null", true),
            ("db", "is-not-null", true),
            ("db", "substitute-null", true),
            ("db", "other", false),
            ("xbrl", "xbrl-measure-currency", true),
            ("xbrl", "xbrl-measure-shares", true),
            ("xbrl", "other", false),
        ] {
            let xml = format!(
                r#"<component library="{library}" name="{name}" kind="5"><targets><datapoint pos="0" key="1"/></targets></component>"#
            );
            let document = roxmltree::Document::parse(&xml)?;
            let node = document.root_element();
            let classified = if library == "db" {
                is_db_function_component(&node)
            } else {
                is_xbrl_measure_component(&node)
            };
            assert_eq!(classified, accepted, "{library}/{name}");
            if accepted && name != "substitute-null" {
                assert!(produces_scalar(&read(&node)), "{library}/{name}");
            }
        }

        let wrong_db_library =
            roxmltree::Document::parse(r#"<component library="core" name="is-null" kind="5"/>"#)?;
        assert!(!is_db_function_component(&wrong_db_library.root_element()));
        let wrong_xbrl_library = roxmltree::Document::parse(
            r#"<component library="core" name="xbrl-measure-shares" kind="5"/>"#,
        )?;
        assert!(!is_xbrl_measure_component(
            &wrong_xbrl_library.root_element()
        ));

        for (name, kind, accepted) in [
            ("convertToISBN13", "20", true),
            ("convertToEAN", "20", true),
            ("convertToISBN10", "20", false),
            ("convertToISBN13", "5", false),
        ] {
            let xml = format!(
                r#"<component library="IsbnConverterService" name="{name}" kind="{kind}"/>"#
            );
            let document = roxmltree::Document::parse(&xml)?;
            assert_eq!(
                is_isbn_converter_component(&document.root_element()),
                accepted,
                "IsbnConverterService/{name}/{kind}"
            );
        }
        let wrong_isbn_library = roxmltree::Document::parse(
            r#"<component library="other" name="convertToISBN13" kind="20"/>"#,
        )?;
        assert!(!is_isbn_converter_component(
            &wrong_isbn_library.root_element()
        ));
        let entry_pins = roxmltree::Document::parse(
            r#"<component library="IsbnConverterService" name="convertToEAN" kind="20"><data><root><entry inpkey="4"/></root><root><entry outkey="7"/></root></data></component>"#,
        )?;
        let converted = read_isbn_converter_component(&entry_pins.root_element())?;
        assert_eq!(converted.inputs, [Some(4)]);
        assert_eq!(converted.outputs, [7]);
        Ok(())
    }

    #[test]
    fn datapoint_positions_preserve_keyless_primary_outputs() {
        let document = roxmltree::Document::parse(
            r#"<component name="filter" library="core" kind="3">
                <sources><datapoint pos="1" key="2"/><datapoint pos="0" key="1"/></sources>
                <targets><datapoint pos="1" key="9"/><datapoint pos="0"/></targets>
            </component>"#,
        )
        .unwrap();
        let component = read(&document.root_element());
        assert_eq!(component.inputs, vec![Some(1), Some(2)]);
        assert_eq!(component.output_pins, vec![None, Some(9)]);
        assert_eq!(component.outputs, vec![9]);
    }

    #[test]
    fn value_map_data_preserves_declared_scalar_types() {
        let document = roxmltree::Document::parse(
            r#"<component name="value-map" library="core" kind="23">
                <sources><datapoint pos="0" key="1"/></sources>
                <targets><datapoint pos="0" key="2"/></targets>
                <data><valuemap defaultValueMode="custom">
                    <valuemapTable><entry from="7" to="1"/></valuemapTable>
                    <input name="input" type="integer"/>
                    <result name="result" type="boolean" defaultValue="0"/>
                </valuemap></data>
            </component>"#,
        )
        .unwrap();

        let value_map = read(&document.root_element()).valuemap.unwrap();
        assert_eq!(value_map.input_type, Some(ScalarType::Int));
        assert_eq!(value_map.table, vec![(Value::Int(7), Value::Bool(true))]);
        assert_eq!(value_map.default, Some(Value::Bool(false)));
    }

    #[test]
    fn value_map_accepts_enabled_default_value_flag() {
        let document = roxmltree::Document::parse(
            r#"<component name="value-map" library="core" kind="23">
                <data><valuemap enableDefaultValue="1">
                    <valuemapTable><entry from="Admin" to="true"/></valuemapTable>
                    <input name="input" type="string"/>
                    <result name="result" type="boolean" defaultValue="false"/>
                </valuemap></data>
            </component>"#,
        )
        .unwrap();

        let value_map = read(&document.root_element()).valuemap.unwrap();
        assert_eq!(value_map.default, Some(Value::Bool(false)));
    }
}
