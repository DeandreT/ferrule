//! Owned XML mixed-content checks against the exact owner's schema.
use std::collections::BTreeSet;

use ir::{SchemaKind, XML_TEXT_FIELD};
use mapping::{OwnedXmlMixedContent, OwnedXmlMixedContentContext, Project};

use super::ValidationIssue;
use super::schema::{follow_schema, follow_schema_resolved, owned_source_root};

/// The group must be a mixed group of its declared owner; an explicit frame
/// prefixes it and is the endpoint root or a repeating owner path. Each
/// occurrence-context replacement names a repeating direct child element.
pub(super) fn validate_owned_mixed_content(
    project: &Project,
    mixed: &OwnedXmlMixedContent,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut elements = BTreeSet::new();
    for replacement in &mixed.replacements {
        if replacement.element.is_empty() {
            issues.push(ValidationIssue::new(
                location,
                "XML mixed-content replacement element cannot be empty",
            ));
        } else if !elements.insert(replacement.element.as_str()) {
            issues.push(ValidationIssue::new(
                location,
                format!(
                    "XML mixed-content element `{}` has more than one replacement",
                    replacement.element
                ),
            ));
        }
    }
    let Some(root) = owned_source_root(project, &mixed.source.owner, location, issues) else {
        return;
    };
    let path = &mixed.source.path;
    let group = follow_schema_resolved(root, path).filter(|node| {
        matches!(node.kind, SchemaKind::Group { .. })
            && node.child(XML_TEXT_FIELD).is_some_and(|text| text.text)
    });
    let Some(group) = group else {
        issues.push(ValidationIssue::new(
            location,
            format!(
                "owned XML mixed-content source `{}` matches no mixed group in its owner",
                path.join("/")
            ),
        ));
        return;
    };
    if let Some(frame) = &mixed.frame {
        if !path.starts_with(frame) {
            issues.push(ValidationIssue::new(
                location,
                format!(
                    "owned frame `{}` does not prefix mixed group `{}`",
                    frame.join("/"),
                    path.join("/")
                ),
            ));
        } else if !frame.is_empty() && follow_schema(root, frame).is_none_or(|node| !node.repeating)
        {
            issues.push(ValidationIssue::new(
                location,
                format!(
                    "owned collection `{}` is not a repeating path in its owner",
                    frame.join("/")
                ),
            ));
        }
    }
    for replacement in &mixed.replacements {
        if replacement.context == OwnedXmlMixedContentContext::Occurrence
            && !replacement.element.is_empty()
            && group
                .child(&replacement.element)
                .is_none_or(|child| !child.repeating || child.attribute || child.text)
        {
            issues.push(ValidationIssue::new(
                location,
                format!(
                    "owned XML mixed-content occurrence `{}` is not a repeating child element in its owner",
                    mixed.occurrence_collection(&replacement.element).join("/")
                ),
            ));
        }
    }
}
