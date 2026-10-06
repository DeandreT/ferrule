//! Compact canvas summaries; the graph remains the source of operation names.

use egui::{Response, Ui, WidgetInfo, WidgetType};
use functions::BuiltinCategory;
use lucide_icons::Icon;
use mapping::{AggregateOp, Node, RuntimeValue};

const SUMMARY_CHAR_LIMIT: usize = 16;
const PIN_CHAR_LIMIT: usize = 18;

pub(super) struct Header {
    icon: Icon,
    summary: String,
}

fn compact(text: &str, limit: usize) -> String {
    let text = text
        .chars()
        .map(|ch| if matches!(ch, '\r' | '\n') { ' ' } else { ch })
        .collect::<String>();
    if text.chars().count() <= limit {
        text
    } else {
        let mut short = text.chars().take(limit - 1).collect::<String>();
        short.push('…');
        short
    }
}

fn builtin_icon(category: BuiltinCategory) -> Icon {
    match category {
        BuiltinCategory::Boolean => Icon::GitBranch,
        BuiltinCategory::String => Icon::Type,
        BuiltinCategory::Numeric => Icon::Calculator,
        BuiltinCategory::DateTime => Icon::Calendar,
        BuiltinCategory::Path => Icon::Folder,
        BuiltinCategory::Json => Icon::Braces,
        BuiltinCategory::FlexText => Icon::FileText,
        BuiltinCategory::Generator => Icon::ListOrdered,
        BuiltinCategory::Conversion => Icon::ArrowRightLeft,
        BuiltinCategory::Validation => Icon::Check,
        BuiltinCategory::Internal => Icon::FunctionSquare,
    }
}

fn aggregate_label(function: AggregateOp) -> &'static str {
    match function {
        AggregateOp::Count => "count",
        AggregateOp::Sum => "sum",
        AggregateOp::Avg => "average",
        AggregateOp::Min => "min",
        AggregateOp::Max => "max",
        AggregateOp::Join => "join",
        AggregateOp::ItemAt => "item at",
    }
}

pub(super) fn header(node: &Node, full_title: &str, is_output: bool) -> Option<Header> {
    let title = if is_output {
        full_title.strip_suffix(" (output)").unwrap_or(full_title)
    } else {
        full_title
    };
    let (icon, summary) = match node {
        Node::Const { value } => {
            let icon = match value {
                ir::Value::String(_) => Icon::Type,
                ir::Value::Int(_) => Icon::Hash,
                ir::Value::Float(_) => Icon::Calculator,
                ir::Value::Bool(_) => Icon::Binary,
                ir::Value::Null | ir::Value::JsonNull(_) | ir::Value::XmlNil(_) => Icon::CircleHelp,
            };
            let display = crate::value_editor::display_string(value);
            let summary = if display.is_empty() {
                if matches!(value, ir::Value::Null) {
                    "null".to_string()
                } else {
                    "empty".to_string()
                }
            } else {
                compact(&display, SUMMARY_CHAR_LIMIT)
            };
            (icon, summary)
        }
        Node::Call { function, .. } => {
            let builtin = functions::builtin(function);
            (
                builtin.map_or(Icon::FunctionSquare, |builtin| {
                    builtin_icon(builtin.category)
                }),
                compact(
                    builtin.map_or(function.as_str(), |builtin| builtin.display_name),
                    SUMMARY_CHAR_LIMIT,
                ),
            )
        }
        Node::UserFunctionCall { .. } => (
            Icon::FunctionSquare,
            compact(
                title.strip_prefix("Call: ").unwrap_or(title),
                SUMMARY_CHAR_LIMIT,
            ),
        ),
        Node::If { .. } => (Icon::GitBranch, String::new()),
        Node::Position { .. } | Node::JoinPosition { .. } => (Icon::ListOrdered, String::new()),
        Node::SourceDocumentPath => (Icon::FileText, "path".into()),
        Node::RuntimeValue { value } => match value {
            RuntimeValue::MappingFilePath => (Icon::Folder, "mapping path".into()),
            RuntimeValue::MainMappingFilePath => (Icon::Folder, "main path".into()),
            RuntimeValue::CurrentDateTime => (Icon::Clock, "now".into()),
        },
        Node::RuntimeParameter { name, .. } | Node::RuntimeParameterDefault { name, .. } => (
            Icon::Variable,
            if name.is_empty() {
                "input?".into()
            } else {
                compact(name, SUMMARY_CHAR_LIMIT)
            },
        ),
        Node::FunctionParameter { .. } => (
            Icon::Variable,
            compact(
                title.strip_prefix("Input: ").unwrap_or(title),
                SUMMARY_CHAR_LIMIT,
            ),
        ),
        Node::Lookup { collection, .. } => (
            Icon::Search,
            if collection.is_empty() {
                "lookup".into()
            } else {
                compact(&collection.join("/"), SUMMARY_CHAR_LIMIT)
            },
        ),
        Node::CollectionFind { .. } => (Icon::Search, "find".into()),
        Node::SequenceExists { .. } => (Icon::ListChecks, "any".into()),
        Node::SequenceItemAt { .. } => (Icon::ListOrdered, "item at".into()),
        Node::Aggregate { function, .. }
        | Node::SequenceAggregate { function, .. }
        | Node::JoinAggregate { function, .. } => (Icon::Sigma, aggregate_label(*function).into()),
        Node::XmlSerialize { .. } => (Icon::FileCode, "serialize".into()),
        _ => return None,
    };
    let summary = if is_output {
        if summary.is_empty() {
            "output".into()
        } else {
            format!("{summary} · output")
        }
    } else {
        summary
    };
    Some(Header { icon, summary })
}

pub(super) fn show_header(ui: &mut Ui, header: Header, full_title: &str) -> Response {
    let response = ui
        .horizontal(|ui| {
            let icon = ui.label(crate::icons::text(header.icon, 16.0));
            if header.summary.is_empty() {
                icon
            } else {
                icon.union(ui.label(header.summary))
            }
        })
        .inner;
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, full_title));
    response
}

pub(super) fn has_properties(node: &Node) -> bool {
    matches!(
        node,
        Node::Const { .. }
            | Node::Call { .. }
            | Node::Position { .. }
            | Node::RuntimeParameter { .. }
            | Node::RuntimeParameterDefault { .. }
            | Node::Aggregate { .. }
            | Node::CollectionFind { .. }
            | Node::XmlSerialize { .. }
    )
}

pub(super) fn hint(node: &Node, full_title: &str) -> String {
    match node {
        Node::Const { value } => format!(
            "{full_title}\nType: {}\n{}",
            value.type_name(),
            crate::value_editor::display_string(value)
        ),
        Node::Call { function, .. } => functions::builtin(function).map_or_else(
            || full_title.to_owned(),
            |builtin| {
                format!(
                    "{full_title}\n{}\n{}",
                    builtin.native_name, builtin.documentation
                )
            },
        ),
        Node::If { .. } => format!(
            "{full_title}\nSelects then or else using the condition input. Only the selected branch is evaluated."
        ),
        Node::UserFunctionCall { args, .. } => format!(
            "{full_title}\n{} input{}",
            args.len(),
            if args.len() == 1 { "" } else { "s" }
        ),
        Node::Position { .. } => format!("{full_title}\nOne-based collection position."),
        Node::JoinPosition { .. } => format!("{full_title}\nFlattened inner-join position."),
        Node::SequenceExists { .. } => {
            format!("{full_title}\nTests whether any sequence item matches the predicate.")
        }
        Node::SequenceItemAt { .. } => {
            format!("{full_title}\nSelects one sequence item using its one-based index.")
        }
        Node::SequenceAggregate {
            predicate,
            expression,
            ..
        } => format!(
            "{full_title}\n{}",
            match (predicate.is_some(), expression.is_some()) {
                (true, true) => "Reduces filtered computed values.",
                (true, false) => "Reduces filtered items.",
                (false, true) => "Reduces computed values.",
                (false, false) => "Reduces sequence items.",
            }
        ),
        Node::JoinAggregate { expression, .. } => format!(
            "{full_title}\n{}",
            if expression.is_some() {
                "Computed expression evaluated once per joined tuple."
            } else {
                "Aggregate evaluated over joined tuples."
            }
        ),
        _ => full_title.to_owned(),
    }
}

pub(super) fn show_input(ui: &mut Ui, label: &str, node: Option<&Node>, index: usize) {
    let hint = if let Some(Node::Call { function, .. }) = node {
        super::builtin_parameter(function, index).map_or_else(
            || label.to_owned(),
            |parameter| format!("{}\nType: {:?}", parameter.name, parameter.domain),
        )
    } else {
        label.to_owned()
    };
    let response = ui.label(compact(label, PIN_CHAR_LIMIT));
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, label));
    response.on_hover_text(hint);
}
