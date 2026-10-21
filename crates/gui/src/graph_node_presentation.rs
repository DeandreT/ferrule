//! Compact canvas summaries; the graph remains the source of operation names.

use std::sync::Arc;

use egui::{Response, Role, TextStyle, Ui, Vec2, WidgetInfo};
use functions::BuiltinCategory;
use lucide_icons::Icon;
use mapping::{AggregateOp, Node, RuntimeValue};

pub(super) const MIN_HEADER_WIDTH: f32 = 32.0;
pub(super) const MAX_HEADER_WIDTH: f32 = 320.0;
const MAX_MANUAL_HEADER_WIDTH: f32 = 480.0;
// A work/allocation bound, not a character-based visible-width policy. The
// actual font galley below decides where the single-line preview is elided.
const MAX_LITERAL_PREVIEW_CHARS: usize = 4096;

#[derive(Clone)]
pub(super) struct Header {
    icon: Icon,
    summary: String,
    accessible_label: String,
}

/// Quoted, escaped presentation only; this never changes a stored literal.
pub(super) fn literal_preview(value: &ir::Value) -> String {
    let ir::Value::String(text) = value else {
        return match value {
            ir::Value::Null => "null".into(),
            _ => crate::value_editor::display_string(value),
        };
    };
    let mut preview = String::from("\"");
    let mut chars = text.chars();
    for ch in chars.by_ref().take(MAX_LITERAL_PREVIEW_CHARS) {
        match ch {
            '\n' => preview.push_str("\\n"),
            '\r' => preview.push_str("\\r"),
            '\t' => preview.push_str("\\t"),
            '\\' => preview.push_str("\\\\"),
            '"' => preview.push_str("\\\""),
            ch if ch.is_control() => preview.extend(ch.escape_debug()),
            ch => preview.push(ch),
        }
    }
    if chars.next().is_some() {
        preview.push('…');
    }
    preview.push('"');
    preview
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

pub(super) fn owned_source_title(node: &Node) -> Option<String> {
    fn owner(owner: &mapping::SourceOwner) -> String {
        match owner {
            mapping::SourceOwner::Primary => "primary".into(),
            mapping::SourceOwner::Named { name } => format!("named {name:?}"),
        }
    }
    fn frame(frame: Option<&[String]>) -> String {
        match frame {
            None => "automatic (unresolved)".into(),
            Some(path) => format!("explicit {path:?}"),
        }
    }
    match node {
        Node::OwnedSourceField(field) => Some(format!(
            "Owned source: {}\nPath: {:?}\nFrame: {}\nRead-only; source ownership execution is unavailable",
            owner(&field.source.owner),
            field.source.path,
            frame(field.frame.as_deref()),
        )),
        Node::OwnedPosition(position) => Some(format!(
            "Owned position: {}\nCollection: {}\nRead-only; source ownership execution is unavailable",
            owner(&position.owner),
            frame(position.collection.as_deref()),
        )),
        Node::OwnedLookup(lookup) => Some(format!(
            "Owned lookup: {}\nCollection: {:?}\nKey: {:?}\nValue: {:?}\nRead-only; owned lookup authoring is unavailable",
            owner(&lookup.collection.owner),
            lookup.collection.path,
            lookup.key,
            lookup.value,
        )),
        Node::OwnedCollectionFind(find) => Some(format!(
            "Owned find: {}\nCollection: {:?}\nRead-only; owned find authoring is unavailable",
            owner(&find.collection.owner),
            find.collection.path,
        )),
        _ => None,
    }
}

pub(super) fn complete_title(node: Option<&Node>, title: String) -> String {
    if let Some(owned) = node.and_then(owned_source_title) {
        return format!(
            "{title}
{owned}"
        );
    }
    match node {
        Some(Node::XmlMixedContent {
            path,
            frame,
            replacements,
        }) => {
            let path = if path.is_empty() {
                "<current> (empty path)".to_owned()
            } else {
                path.join("/")
            };
            let frame = match frame {
                None => "automatic".to_owned(),
                Some(frame) if frame.is_empty() => "explicit (empty path)".to_owned(),
                Some(frame) => format!("explicit {}", frame.join("/")),
            };
            return format!(
                "{title}\nPath: {path}\nFrame: {frame}\nReplacements: {}\nRead-only ordered XML content",
                replacements.len(),
            );
        }
        Some(Node::XmlSerialize {
            path,
            frame,
            schema,
            declaration,
            indent,
            namespace,
        }) => {
            let path = if path.is_empty() {
                "<current> (empty path)".to_owned()
            } else {
                path.join("/")
            };
            let frame = match frame {
                None => "automatic".to_owned(),
                Some(frame) if frame.is_empty() => "explicit (empty path)".to_owned(),
                Some(frame) => format!("explicit {}", frame.join("/")),
            };
            let element_namespace = match &schema.xml_namespace {
                None => "unspecified".to_owned(),
                Some(ir::XmlNamespace::Unqualified) => "explicitly unqualified".to_owned(),
                Some(ir::XmlNamespace::Qualified(uri)) => format!("qualified {:?}", uri.as_str()),
            };
            let default_namespace = match namespace {
                None => "not set".to_owned(),
                Some(uri) => format!("set {uri:?}"),
            };
            return format!(
                "{title}\nPath: {path}\nFrame: {frame}\nElement: {}\nElement namespace: {element_namespace}\nXML declaration: {declaration}\nIndent output: {indent}\nDefault namespace: {default_namespace}",
                schema.name,
            );
        }
        Some(Node::ValueMap {
            input_type,
            table,
            default,
            ..
        }) => {
            let input_type = input_type.map_or_else(
                || "automatic (runtime type)".to_owned(),
                |ty| format!("{ty:?}").to_lowercase(),
            );
            let unmatched = match default {
                None => "null (no default)".to_owned(),
                Some(ir::Value::Null) => "null (stored default)".to_owned(),
                Some(ir::Value::String(value)) if value.is_empty() => {
                    "empty string (stored default)".to_owned()
                }
                Some(value) => format!(
                    "{}: {} (stored default)",
                    value.type_name(),
                    crate::value_editor::display_string(value),
                ),
            };
            return format!(
                "{title}\nEntries: {}\nInput type: {input_type}\nUnmatched value: {unmatched}\nFirst matching entry wins. Edit the table with the pencil.",
                table.len(),
            );
        }
        Some(Node::DynamicSourceField { object, frame, .. }) => {
            let object = if object.is_empty() {
                "<current> (empty path)".to_owned()
            } else {
                object.join("/")
            };
            let frame = match frame {
                None => "automatic".to_owned(),
                Some(frame) if frame.is_empty() => "explicit (empty path)".to_owned(),
                Some(frame) => format!("explicit {}", frame.join("/")),
            };
            return format!(
                "{title}\nOpen source object: {object}\nFrame: {frame}\nThe property name is supplied by the input.\nChoose a supported source object with the pencil."
            );
        }
        Some(Node::SourceRootField { path, required }) => {
            let path = if path.is_empty() {
                "<empty> (empty path)".to_owned()
            } else {
                path.join("/")
            };
            let (required, policy) = if *required {
                (
                    "yes",
                    "A missing value stops execution when this field is read.",
                )
            } else {
                ("no", "Missing values remain null.")
            };
            return format!(
                "{title}\nPrimary source root\nPath: {path}\nRequired read: {required}\n{policy}"
            );
        }
        Some(Node::SourceRootXmlTypeEquals {
            canonical_expanded_type,
        }) => {
            return format!(
                "{title}\nPrimary source root\nXML type: {canonical_expanded_type}\nCompares the actual input XML type annotation."
            );
        }
        _ => {}
    }
    if let Some(Node::JoinField {
        join,
        collection,
        path,
    }) = node
    {
        let collection = if collection.is_empty() {
            "<empty> (empty collection)".to_owned()
        } else {
            collection.join("/")
        };
        let path = if path.is_empty() {
            "<current> (empty path)".to_owned()
        } else {
            path.join("/")
        };
        return format!(
            "{title}\nJoin: #{}\nCollection: {collection}\nPath: {path}\nRead-only joined field",
            join.get(),
        );
    }
    let Some(Node::SourceField { path, frame }) = node else {
        return title;
    };
    let path = if path.is_empty() {
        "<current> (empty path)".to_owned()
    } else {
        path.join("/")
    };
    let frame = match frame {
        None => "automatic".to_owned(),
        Some(frame) if frame.is_empty() => "explicit (empty path)".to_owned(),
        Some(frame) => format!("explicit {}", frame.join("/")),
    };
    format!("{title}\nPath: {path}\nFrame: {frame}")
}

pub(super) fn header(node: &Node, full_title: &str, is_output: bool) -> Option<Header> {
    let title = if is_output {
        full_title.strip_suffix(" (output)").unwrap_or(full_title)
    } else {
        full_title
    };
    let (icon, summary) = match node {
        Node::OwnedSourceField(field) => (
            Icon::ArrowRightFromLine,
            field
                .source
                .path
                .last()
                .map_or("owned root", String::as_str)
                .to_owned(),
        ),
        Node::OwnedPosition(_) => (Icon::ListOrdered, "owned".into()),
        Node::Const { value } => {
            let icon = match value {
                ir::Value::String(_) => Icon::Type,
                ir::Value::Int(_) => Icon::Hash,
                ir::Value::Float(_) | ir::Value::Decimal(_) => Icon::Calculator,
                ir::Value::Bool(_) => Icon::Binary,
                ir::Value::Null | ir::Value::JsonNull(_) | ir::Value::XmlNil(_) => Icon::CircleHelp,
            };
            let summary = match value {
                ir::Value::Null | ir::Value::JsonNull(_) | ir::Value::XmlNil(_) => {
                    value.type_name().to_owned()
                }
                _ => format!("{} {}", value.type_name(), literal_preview(value)),
            };
            (icon, summary)
        }
        Node::Call { function, .. } => {
            let builtin = functions::builtin(function);
            (
                builtin.map_or(Icon::FunctionSquare, |builtin| {
                    builtin_icon(builtin.category)
                }),
                builtin
                    .map_or(function.as_str(), |builtin| builtin.display_name)
                    .to_owned(),
            )
        }
        Node::UserFunctionCall { .. } => (
            Icon::FunctionSquare,
            title.strip_prefix("Call: ").unwrap_or(title).to_owned(),
        ),
        Node::If { .. } => (Icon::GitBranch, "If / else".into()),
        Node::Raise { .. } => (Icon::CircleX, "error".into()),
        Node::Position { .. } | Node::JoinPosition { .. } => (Icon::ListOrdered, String::new()),
        Node::SourceField { path, .. } => {
            if matches!(title, "Stage input" | "Mapped output") {
                (Icon::ListOrdered, title.into())
            } else if let Some(item) = title.strip_prefix("Generated item #") {
                (Icon::ListOrdered, format!("item #{item}"))
            } else {
                (
                    Icon::ArrowRightFromLine,
                    path.last().cloned().unwrap_or_else(|| "current".into()),
                )
            }
        }
        Node::SourceRootField { path, required } => (
            Icon::ArrowRightFromLine,
            format!(
                "{} {}",
                if *required { "required" } else { "primary" },
                path.last()
                    .filter(|field| !field.is_empty())
                    .map_or("field?", String::as_str),
            ),
        ),
        Node::SourceRootXmlTypeEquals {
            canonical_expanded_type,
        } => {
            let local_type = canonical_expanded_type
                .strip_prefix('{')
                .and_then(|expanded| expanded.split_once('}'))
                .map_or(canonical_expanded_type.as_str(), |(_, local)| local);
            (
                Icon::Check,
                format!(
                    "type = {}",
                    if local_type.is_empty() {
                        "?"
                    } else {
                        local_type
                    }
                ),
            )
        }
        Node::JoinField { join, path, .. } => (
            Icon::ArrowRightFromLine,
            format!(
                "#{} {}",
                join.get(),
                path.last().map_or("current", String::as_str),
            ),
        ),
        Node::DynamicSourceField { object, .. } => (
            Icon::Braces,
            format!(
                "dynamic {}",
                object.last().map_or("current", String::as_str),
            ),
        ),
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
                name.clone()
            },
        ),
        Node::FunctionParameter { .. } => (
            Icon::Variable,
            title.strip_prefix("Input: ").unwrap_or(title).to_owned(),
        ),
        Node::ValueMap { table, .. } => (
            Icon::ArrowRightLeft,
            format!(
                "{} {}",
                table.len(),
                if table.len() == 1 { "entry" } else { "entries" }
            ),
        ),
        Node::Lookup { collection, .. } => (
            Icon::Search,
            if collection.is_empty() {
                "lookup".into()
            } else {
                collection.join("/")
            },
        ),
        Node::CollectionFind { .. } => (Icon::Search, "find".into()),
        Node::SequenceExists { .. } => (Icon::ListChecks, "any".into()),
        Node::SequenceItemAt { .. } => (Icon::ListOrdered, "item at".into()),
        Node::Aggregate { function, .. }
        | Node::SequenceAggregate { function, .. }
        | Node::JoinAggregate { function, .. } => (Icon::Sigma, aggregate_label(*function).into()),
        Node::XmlMixedContent { path, .. } => (
            Icon::FileCode,
            format!("mixed {}", path.last().map_or("current", String::as_str),),
        ),
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
    Some(Header {
        icon,
        summary: summary.replace('\r', "\\r").replace('\n', "\\n"),
        accessible_label: hint(node, &complete_title(Some(node), full_title.to_owned())),
    })
}

pub(super) fn show_endpoint_header(ui: &mut Ui, source: bool, full_title: &str) -> Response {
    let (icon, prefix) = if source {
        (Icon::ArrowRightFromLine, "Source: ")
    } else {
        (Icon::ArrowRightToLine, "Target: ")
    };
    let title = full_title.strip_prefix(prefix).unwrap_or(full_title);
    let response = ui
        .horizontal(|ui| {
            let icon = ui.label(crate::icons::text(icon, icon_font_size(ui)));
            let title = ui.add(
                egui::Label::new(title)
                    .truncate()
                    .show_tooltip_when_elided(false),
            );
            icon.union(title)
        })
        .inner;
    response.widget_info(|| WidgetInfo::labeled(Role::Label, ui.is_enabled(), full_title));
    response
}

fn icon_font_size(ui: &Ui) -> f32 {
    egui::FontSelection::Default.resolve(ui.style()).size + 2.0
}

fn icon_galley(ui: &Ui, icon: Icon) -> Arc<egui::Galley> {
    egui::WidgetText::from(crate::icons::text(icon, icon_font_size(ui))).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        TextStyle::Body,
    )
}

fn text_galley(ui: &Ui, text: &str, max_width: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(
        text.to_owned(),
        egui::FontSelection::Default.resolve(ui.style()),
        ui.visuals().text_color(),
        max_width.max(0.0),
    );
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.break_on_newline = false;
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

/// Measured header content size, excluding Snarl's node/header frames and
/// any edit/open-function control. Uses the active font, zoom and DPI.
pub(super) fn header_size(ui: &Ui, header: &Header) -> Vec2 {
    let icon = icon_galley(ui, header.icon).size();
    if header.summary.is_empty() {
        return Vec2::new(icon.x.max(MIN_HEADER_WIDTH), icon.y);
    }
    let gap = ui.spacing().item_spacing.x;
    let text = text_galley(
        ui,
        &header.summary,
        (MAX_HEADER_WIDTH - icon.x - gap).max(0.0),
    );
    Vec2::new(
        (icon.x + gap + text.size().x).clamp(MIN_HEADER_WIDTH, MAX_HEADER_WIDTH),
        icon.y.max(text.size().y),
    )
}

/// Preferred content width, not an instruction to resize or move saved nodes.
/// The caller must keep an explicit manual width until Reset to auto size.
pub(super) fn node_width(ui: &Ui, node: &Node, full_title: &str, is_output: bool) -> f32 {
    let preferred = header(node, full_title, is_output).map_or_else(
        || text_galley(ui, full_title, MAX_HEADER_WIDTH).size().x,
        |header| header_size(ui, &header).x,
    );
    // Unlike elided general labels, the three conditional meanings must
    // always remain readable. This is only geometry; their order is unchanged.
    let input_width = if matches!(node, Node::If { .. }) {
        text_galley(ui, "condition", f32::INFINITY).size().x
    } else {
        0.0
    };
    preferred
        .max(input_width)
        .clamp(MIN_HEADER_WIDTH, MAX_HEADER_WIDTH)
}

fn resolved_header_width(ui: &Ui, header: &Header, manual_width: Option<f32>) -> f32 {
    manual_width
        .filter(|width| width.is_finite())
        .unwrap_or_else(|| header_size(ui, header).x)
        .clamp(MIN_HEADER_WIDTH, MAX_MANUAL_HEADER_WIDTH)
}

fn show_header_contents(ui: &mut Ui, header: &Header, width: f32) -> Response {
    let icon = icon_galley(ui, header.icon);
    let text_width = (width - icon.size().x - ui.spacing().item_spacing.x).max(0.0);
    let icon = ui.add(egui::Label::new(icon).selectable(false));
    if header.summary.is_empty() {
        icon
    } else {
        icon.union(
            ui.add(
                egui::Label::new(text_galley(ui, &header.summary, text_width))
                    .selectable(false)
                    .show_tooltip_when_elided(false),
            ),
        )
    }
}

/// Lays out a header row exactly `width` wide, starting at the cursor.
/// egui-snarl has already placed its collapse button and drag space on the
/// row; bounding the row itself with set_max_width would move its cursor
/// back over them, so a fresh child is bounded instead.
fn header_row<R>(ui: &mut Ui, width: f32, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let min = ui.cursor().min;
    let row = egui::Rect::from_min_max(min, egui::pos2(min.x + width, ui.max_rect().bottom()));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(row)
            .layout(egui::Layout::left_to_right(egui::Align::Min)),
        |ui| {
            ui.set_min_width(width);
            ui.set_max_width(width);
            ui.horizontal(add_contents).inner
        },
    )
    .inner
}

pub(super) fn show_header(
    ui: &mut Ui,
    header: Header,
    full_title: &str,
    manual_width: Option<f32>,
) -> Response {
    let preferred = resolved_header_width(ui, &header, manual_width);
    let response = header_row(ui, preferred, |ui| {
        show_header_contents(ui, &header, preferred)
    });
    let accessible_label = if header.accessible_label.is_empty() {
        full_title
    } else {
        &header.accessible_label
    };
    response.widget_info(|| WidgetInfo::labeled(Role::Label, ui.is_enabled(), accessible_label));
    response
}

/// Single-line alternative for callers that host editing in the header.
/// The second response is the keyboard-focusable edit trigger/popup anchor;
/// `.clicked()` includes native Enter/Space activation. No popup or canvas
/// focus policy is implemented here. Hidden controls retain their space and
/// keyboard target, and reveal themselves when focused or hovered.
pub(super) fn show_header_with_edit(
    ui: &mut Ui,
    header: Header,
    full_title: &str,
    manual_width: Option<f32>,
    reveal_edit: bool,
    edit_enabled: bool,
) -> (Response, Response) {
    let edit_size = ui.spacing().interact_size.y.max(18.0);
    let edit_space = edit_size + ui.spacing().item_spacing.x;
    let preferred = manual_width
        .filter(|width| width.is_finite())
        .unwrap_or_else(|| header_size(ui, &header).x + edit_space)
        .clamp(MIN_HEADER_WIDTH + edit_space, MAX_MANUAL_HEADER_WIDTH);
    header_row(ui, preferred, |ui| {
        let response = show_header_contents(ui, &header, preferred - edit_space);
        response.widget_info(|| {
            WidgetInfo::labeled(Role::Label, ui.is_enabled(), &header.accessible_label)
        });
        let edit = ui
            .add_enabled_ui(edit_enabled, |ui| {
                let (rect, edit) =
                    ui.allocate_exact_size(Vec2::splat(edit_size), egui::Sense::click());
                let label = format!("Edit {full_title} · F2");
                edit.widget_info(|| WidgetInfo::labeled(Role::Button, ui.is_enabled(), &label));
                if reveal_edit || edit.hovered() || edit.has_focus() {
                    let visuals = ui.style().interact(&edit);
                    ui.painter().rect(
                        rect,
                        visuals.corner_radius,
                        visuals.weak_bg_fill,
                        visuals.bg_stroke,
                        egui::StrokeKind::Inside,
                    );
                    let icon = icon_galley(ui, Icon::Pencil);
                    ui.painter().galley(
                        rect.center() - icon.size() * 0.5,
                        icon,
                        visuals.text_color(),
                    );
                }
                edit.on_hover_text(label)
            })
            .inner;
        (response, edit)
    })
}

/// Header-aligned native pins are suitable for these individual cards only.
/// This neither merges operations nor changes their input/output identities.
pub(super) fn is_inline(node: &Node) -> bool {
    matches!(node, Node::Const { .. })
        || matches!(node, Node::Call { function, args } if args.len() == 1 && matches!(function.as_str(), "exists" | "not"))
}

pub(super) fn has_properties(node: &Node) -> bool {
    matches!(
        node,
        Node::OwnedSourceField(_)
            | Node::OwnedPosition(_)
            | Node::Const { .. }
            | Node::Call { .. }
            | Node::SourceField { .. }
            | Node::SourceRootField { .. }
            | Node::SourceRootXmlTypeEquals { .. }
            | Node::DynamicSourceField { .. }
            | Node::ValueMap { .. }
            | Node::Raise { .. }
            | Node::JoinField { .. }
            | Node::Lookup { .. }
            | Node::Position { .. }
            | Node::RuntimeParameter { .. }
            | Node::RuntimeParameterDefault { .. }
            | Node::Aggregate { .. }
            | Node::CollectionFind { .. }
            | Node::SequenceExists { .. }
            | Node::SequenceItemAt { .. }
            | Node::SequenceAggregate { .. }
            | Node::XmlMixedContent { .. }
            | Node::XmlSerialize { .. }
    )
}

pub(super) fn hint(node: &Node, full_title: &str) -> String {
    match node {
        Node::Const { value } => {
            let literal = match value {
                ir::Value::String(text) => format!("{text:?}"),
                ir::Value::Null => "null".into(),
                _ => crate::value_editor::display_string(value),
            };
            format!(
                "{full_title}\nType: {}\nLiteral: {literal}",
                value.type_name()
            )
        }
        Node::Call { function, .. } => functions::builtin(function).map_or_else(
            || full_title.to_owned(),
            |builtin| {
                format!(
                    "{full_title}\n{}\n{}",
                    builtin.native_name, builtin.documentation
                )
            },
        ),
        Node::Raise { message } => format!(
            "{full_title}\nStops execution when this expression is reached.\n{}",
            if message.is_some() {
                "Reads the message input only when reached."
            } else {
                "No message is set. Connect the optional message input to supply one."
            }
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

pub(super) fn input_pin_hint(node: Option<&Node>, index: usize) -> Option<String> {
    let Node::XmlMixedContent { replacements, .. } = node? else {
        return None;
    };
    let replacement = replacements.get(index)?;
    let collection = if replacement.collection.is_empty() {
        "parent context (empty path)".to_owned()
    } else {
        replacement.collection.join("/")
    };
    Some(format!(
        "Input {index}: {}\nCollection: {collection}\nExpression: #{}",
        replacement.element, replacement.expression,
    ))
}

pub(super) fn conditional_input_label(node: &Node, index: usize) -> Option<&'static str> {
    matches!(node, Node::If { .. })
        .then(|| ["condition", "then", "else"].get(index).copied())
        .flatten()
}

pub(super) fn show_input(ui: &mut Ui, label: &str, node: Option<&Node>, index: usize) {
    let conditional = node.and_then(|node| conditional_input_label(node, index));
    let label = conditional.unwrap_or(label);
    let hint = if let Some(Node::Call { function, .. }) = node {
        super::builtin_parameter(function, index).map_or_else(
            || label.to_owned(),
            |parameter| format!("{}\nType: {:?}", parameter.name, parameter.domain),
        )
    } else {
        label.to_owned()
    };
    let widget = egui::Label::new(label).show_tooltip_when_elided(false);
    let response = if conditional.is_some() {
        ui.add(widget.extend())
    } else {
        ui.add(widget.truncate())
    };
    response.widget_info(|| WidgetInfo::labeled(Role::Label, ui.is_enabled(), label));
    response.on_hover_text(hint);
}

#[cfg(test)]
#[path = "graph_node_presentation_tests.rs"]
mod tests;
