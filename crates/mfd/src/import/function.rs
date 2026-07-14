use ir::{ScalarType, Value};
use mapping::AggregateOp;

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
    pub(super) constant: Option<(String, String)>,
    pub(super) valuemap: Option<ValueMapData>,
    pub(super) sort_descending: Option<bool>,
    pub(super) db_where: Option<DbWhereComponent>,
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
    let sort_descending = data
        .and_then(|data| data.descendants().find(|node| node.has_tag_name("sort")))
        .map(|sort| {
            sort.descendants()
                .find(|node| node.has_tag_name("key"))
                .is_some_and(|key| key.attribute("direction") == Some("descending"))
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
        constant,
        valuemap,
        sort_descending,
        db_where,
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
    component.attribute("kind") == Some("21")
        || component.attribute("kind") == Some("5")
            && component.attribute("name") == Some("substitute-null")
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
        )
        || component.kind == 5 && aggregate_op(&component.name).is_some()
        || map_name(&component.name).is_some()
}

pub(super) fn is_input(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 6
}

pub(super) fn is_sort(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 30 && component.sort_descending.is_some()
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
    component.library == "core"
        && component.kind == 5
        && matches!(
            component.name.as_str(),
            "tokenize" | "tokenize-by-length" | "generate-sequence"
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
        "get-folder" => "get_folder",
        "remove-folder" => "remove_folder",
        "resolve-filepath" => "resolve_filepath",
        "is-xsi-nil" => "is_xml_nil",
        "exists" => "exists",
        "round" | "round-precision" => "round",
        "date-from-datetime" => "date_from_datetime",
        "month-from-datetime" => "month_from_datetime",
        "time-from-datetime" => "time_from_datetime",
        "datetime-from-date-and-time" => "datetime_from_date_and_time",
        "datetime-from-parts" => "datetime_from_parts",
        "datetime-add" => "datetime_add",
        "parse-date" => "parse_date",
        "parse-dateTime" => "parse_datetime",
        "parse-time" => "parse_time",
        "edifact-to-datetime" => "edifact_to_datetime",
        "substitute-missing" | "substitute-null" => "substitute_missing",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use ir::{ScalarType, Value};

    use super::{map_name, read};

    #[test]
    fn scalar_names_use_canonical_ir_spelling() {
        assert_eq!(map_name("string"), Some("string"));
        assert_eq!(map_name("numeric"), Some("is_numeric"));
        assert_eq!(map_name("format-number"), Some("format_number"));
        assert_eq!(map_name("time-from-datetime"), Some("time_from_datetime"));
        assert_eq!(map_name("month-from-datetime"), Some("month_from_datetime"));
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
