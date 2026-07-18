//! Bounded import of MapForce-style EDI configuration files.
//!
//! The configuration is a library of positional data, composite, and
//! segment definitions plus a message/envelope tree. Import expands that
//! library into ferrule's ordinary EDI [`SchemaNode`] representation so the
//! original configuration is not needed at execution time.

mod definitions;
mod files;
pub mod idoc;
mod parser;
pub mod swift;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use definitions::{Definitions, compiled_config, load_definitions};
use files::{Files, parse_document, resolve_message_config, resolve_sibling};
use ir::SchemaNode;
use mapping::{EdiImpliedDecimal, EdiLexicalFormat};
use parser::{MessageName, build_envelope, build_message};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not read EDI configuration `{path}`: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse EDI configuration `{path}`: {source}")]
    Xml {
        path: PathBuf,
        #[source]
        source: roxmltree::Error,
    },
    #[error("invalid EDI configuration: {0}")]
    Invalid(String),
    #[error("EDI configuration exceeds the {0} limit")]
    Limit(&'static str),
}

/// A compiled EDI configuration whose schema and formatting metadata no
/// longer depend on the source configuration files.
pub struct CompiledConfig {
    pub schema: SchemaNode,
    pub implied_decimals: Vec<EdiImpliedDecimal>,
    pub lexical_formats: Vec<EdiLexicalFormat>,
}

/// Imports one complete EDI configuration.
///
/// A message configuration is wrapped in its sibling `Envelope.Config`.
/// An envelope configuration uses `selected_messages` to resolve and embed
/// each concrete message selected by the `.mfd` component.
pub fn import_config(
    path: &Path,
    selected_messages: &[String],
) -> Result<CompiledConfig, ConfigError> {
    let mut files = Files::default();
    let mut definitions = Definitions::default();
    load_definitions(path, &mut files, &mut definitions)?;

    let main_text = files.read(path)?;
    let main_doc = parse_document(path, &main_text)?;
    let root = main_doc.root_element();
    let standard = root
        .children()
        .find(|node| node.has_tag_name("Format"))
        .and_then(|node| node.attribute("standard"))
        .ok_or_else(|| ConfigError::Invalid("configuration has no Format/@standard".into()))?;
    let mut schema_nodes = 0usize;

    if let Some(message_layout) = root.children().find(|node| node.has_tag_name("Message")) {
        let mut message = build_message(
            message_layout,
            &definitions,
            MessageName::Canonical,
            None,
            &mut schema_nodes,
        )?;
        if standard.eq_ignore_ascii_case("HL7") {
            let message_type = message_layout
                .children()
                .find(|node| node.has_tag_name("MessageType"))
                .and_then(|node| node.text())
                .ok_or_else(|| ConfigError::Invalid("HL7 Message has no MessageType".into()))?;
            message.name = message_type.to_string();
            return Ok(compiled_config(message, &definitions));
        }
        let envelope_path = resolve_sibling(path, "Envelope.Config")?;
        load_definitions(&envelope_path, &mut files, &mut definitions)?;
        let envelope_text = files.read(&envelope_path)?;
        let envelope_doc = parse_document(&envelope_path, &envelope_text)?;
        let schema = build_envelope(
            envelope_doc.root_element(),
            standard,
            vec![message],
            &definitions,
            &mut schema_nodes,
        )?;
        return Ok(compiled_config(schema, &definitions));
    }

    let envelope = root
        .children()
        .find(|node| node.has_tag_name("Group"))
        .ok_or_else(|| {
            ConfigError::Invalid("configuration has no Message or Group layout".into())
        })?;
    if selected_messages.is_empty() {
        return Err(ConfigError::Invalid(
            "envelope configuration has no selected message types".into(),
        ));
    }

    let select = envelope
        .descendants()
        .find(|node| node.has_tag_name("Select"))
        .ok_or_else(|| ConfigError::Invalid("envelope has no message Select".into()))?;
    let discriminator = select.attribute("field").map(str::to_string);
    let mut messages = Vec::with_capacity(selected_messages.len());
    for selected in selected_messages {
        let message_path = resolve_message_config(path, selected)?;
        load_definitions(&message_path, &mut files, &mut definitions)?;
        let message_text = files.read(&message_path)?;
        let message_doc = parse_document(&message_path, &message_text)?;
        let message = message_doc
            .root_element()
            .children()
            .find(|node| node.has_tag_name("Message"))
            .ok_or_else(|| {
                ConfigError::Invalid(format!(
                    "selected message `{selected}` has no Message layout"
                ))
            })?;
        messages.push(build_message(
            message,
            &definitions,
            MessageName::Declared,
            discriminator
                .as_deref()
                .map(|path| (path, selected.as_str())),
            &mut schema_nodes,
        )?);
    }
    let schema = build_envelope(root, standard, messages, &definitions, &mut schema_nodes)?;
    Ok(compiled_config(schema, &definitions))
}
