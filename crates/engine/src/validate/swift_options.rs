use std::collections::BTreeSet;

use ir::SchemaNode;
use mapping::{FormatOptions, Scope, SwiftMtDirection};

use super::ValidationIssue;

const MESSAGE: &str = "Message";
const APPLICATION_HEADER: &str = "Application Header";

/// The side a boundary's SWIFT MT options are validated for. A target
/// carries the scope tree that constructs its messages.
#[derive(Clone, Copy)]
pub(super) enum SwiftSide<'a> {
    Source,
    Target(&'a Scope),
}

/// A SWIFT MT target needs its layout plus the direction and per-message
/// headers its mapped instance does not carry; sources take neither.
pub(super) fn validate_swift_options(
    location: &str,
    options: &FormatOptions,
    schema: &SchemaNode,
    side: SwiftSide<'_>,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut refuse = |message: String| issues.push(ValidationIssue::new(location, message));
    let root = match side {
        SwiftSide::Source => {
            if options.swift_mt_output.is_some() {
                refuse("`swift_mt_output` is valid only for mapping targets".into());
            }
            return;
        }
        SwiftSide::Target(root) => root,
    };
    if options.swift_mt_output.is_some() && options.swift_mt.is_none() {
        refuse("`swift_mt_output` requires an embedded `swift_mt` layout".into());
        return;
    }
    if options.swift_mt.is_none() {
        return;
    }
    if let Err(error) = format_edi::swift::validate_output(schema, options) {
        refuse(error.to_string());
    }
    if let Some(layout) = &options.swift_mt {
        let filled = FilledPaths::of(root);
        if let Err(error) =
            format_edi::swift::validate_filled_fields(layout, |path| filled.fills(path))
        {
            refuse(error.to_string());
        }
    }
    let Some(output) = &options.swift_mt_output else {
        return;
    };
    // Every document would repeat the same session, sequence and input
    // reference values, which FIN never repeats.
    if root.produces_documents() {
        refuse(
            "`swift_mt_output` headers cannot be shared by the separate documents this target produces"
                .into(),
        );
    }
    if let Some(mapped) = contradicting_direction(root, output.direction()) {
        refuse(format!(
            "the mapping constructs only the {mapped} application header, but `swift_mt_output` writes {} messages",
            output.direction().group_name()
        ));
    }
}

/// The other direction when some `Application Header` scope under `Message`
/// constructs only that direction's group. Unmapped headers, and headers
/// mapping both groups, take the configured direction when written.
fn contradicting_direction(root: &Scope, configured: SwiftMtDirection) -> Option<&'static str> {
    let other = match configured {
        SwiftMtDirection::Input => SwiftMtDirection::Output,
        SwiftMtDirection::Output => SwiftMtDirection::Input,
    }
    .group_name();
    let configured = configured.group_name();
    root.children
        .iter()
        .filter(|scope| scope.target_field == MESSAGE)
        .flat_map(|message| &message.children)
        .filter(|scope| scope.target_field == APPLICATION_HEADER)
        .any(|header| {
            let mapped = header
                .children
                .iter()
                .map(|group| group.target_field.as_str())
                .filter(|name| [configured, other].contains(name))
                .collect::<BTreeSet<_>>();
            mapped.contains(other) && !mapped.contains(configured)
        })
        .then_some(other)
}

/// Target paths below `Message` that bindings and child scopes construct,
/// used to decide which layout fields the mapping fills. Copies and computed
/// field names are not spelled out here; the writer still refuses a shadowed
/// field they fill when the output is written.
struct FilledPaths(BTreeSet<Vec<String>>);

impl FilledPaths {
    fn of(root: &Scope) -> Self {
        let mut filled = Self(BTreeSet::new());
        for message in root
            .children
            .iter()
            .filter(|scope| scope.target_field == MESSAGE)
        {
            filled.visit(message, &mut Vec::new());
        }
        filled
    }

    fn visit(&mut self, scope: &Scope, path: &mut Vec<String>) {
        for binding in &scope.bindings {
            path.push(binding.target_field.clone());
            self.0.insert(path.clone());
            path.pop();
        }
        for child in &scope.children {
            path.push(child.target_field.clone());
            self.0.insert(path.clone());
            self.visit(child, path);
            path.pop();
        }
    }

    /// Whether anything is constructed at or below the field.
    fn fills(&self, field: &[String]) -> bool {
        self.0.iter().any(|path| path.starts_with(field))
    }
}
