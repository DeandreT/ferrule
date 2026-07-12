use ir::Value;
use mapping::AggregateOp;

use super::schema::parse_u32;

/// One function component's extracted facts.
pub(super) type ValueMapData = (Vec<(String, String)>, Option<String>);

pub(super) struct FnComponent {
    pub(super) library: String,
    pub(super) name: String,
    pub(super) kind: u32,
    /// Input pins in `pos` order; `None` for declared-but-keyless pins.
    pub(super) inputs: Vec<Option<u32>>,
    /// Output pin keys in `pos` order.
    pub(super) outputs: Vec<u32>,
    pub(super) constant: Option<(String, String)>,
    pub(super) valuemap: Option<ValueMapData>,
    pub(super) sort_descending: Option<bool>,
}

pub(super) fn read(component: &roxmltree::Node) -> FnComponent {
    let library = component
        .attribute("library")
        .unwrap_or_default()
        .to_string();
    let name = component.attribute("name").unwrap_or_default().to_string();
    let kind = parse_u32(component.attribute("kind")).unwrap_or(0);
    let pins = |tag: &str| -> Vec<Option<u32>> {
        component
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == tag)
            .map(|pins| {
                pins.children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "datapoint")
                    .map(|d| parse_u32(d.attribute("key")))
                    .collect()
            })
            .unwrap_or_default()
    };
    let inputs = pins("sources");
    let outputs = pins("targets").into_iter().flatten().collect();

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
            let table = vm
                .descendants()
                .filter(|n| n.has_tag_name("entry"))
                .map(|e| {
                    (
                        e.attribute("from").unwrap_or_default().to_string(),
                        e.attribute("to").unwrap_or_default().to_string(),
                    )
                })
                .collect();
            let default = vm
                .descendants()
                .find(|n| n.has_tag_name("result"))
                .and_then(|r| r.attribute("defaultValue"))
                .map(str::to_string)
                .filter(|_| vm.attribute("defaultValueMode") == Some("custom"));
            (table, default)
        });
    let sort_descending = data
        .and_then(|data| data.descendants().find(|node| node.has_tag_name("sort")))
        .map(|sort| {
            sort.descendants()
                .find(|node| node.has_tag_name("key"))
                .is_some_and(|key| key.attribute("direction") == Some("descending"))
        });

    FnComponent {
        library,
        name,
        kind,
        inputs,
        outputs,
        constant,
        valuemap,
        sort_descending,
    }
}

pub(super) fn is_filter(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 3
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
        "boolean" => value.parse().map(Value::Bool).unwrap_or(Value::Null),
        _ => Value::String(value.to_string()),
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
        "format-number" => "format_number",
        "trim" => "trim",
        "left-trim" => "left_trim",
        "right-trim" => "right_trim",
        "pad-string-left" => "pad_string_left",
        "pad-string-right" => "pad_string_right",
        "substring" => "substring",
        "substring-before" => "substring_before",
        "substring-after" => "substring_after",
        "exists" => "exists",
        "round" | "round-precision" => "round",
        "date-from-datetime" => "date_from_datetime",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::map_name;

    #[test]
    fn scalar_names_use_canonical_ir_spelling() {
        assert_eq!(map_name("string"), Some("string"));
        assert_eq!(map_name("format-number"), Some("format_number"));
    }
}
