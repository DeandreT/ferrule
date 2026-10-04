//! Preserves only a caller-supplied default namespace in the new document API.
//! Legacy serializers keep their existing namespace rendering.
use std::borrow::Cow;
use std::ops::Range;

use quick_xml::{Reader, events::Event};

use super::{XmlBoundaryError, XmlBoundaryErrorKind, check_document_size};

const ROOT_HEADER_BYTES: usize = 1024 * 1024;

struct Patches {
    spans: Vec<Range<usize>>,
    growth: usize,
    root_header_bytes: Option<usize>,
}

fn refusal(reason: &str) -> XmlBoundaryError {
    XmlBoundaryError::detail(XmlBoundaryErrorKind::Output, reason)
}

fn scan(xml: &str, uri: &str, root_only: bool) -> Result<Patches, XmlBoundaryError> {
    let mut reader = Reader::from_str(xml);
    let mut patches = Patches {
        spans: Vec::new(),
        growth: 0,
        root_header_bytes: None,
    };
    let base = xml.as_ptr() as usize;
    loop {
        let event = reader
            .read_event()
            .map_err(|error| XmlBoundaryError::with_source(XmlBoundaryErrorKind::Output, error))?;
        let start = match event {
            Event::Start(start) | Event::Empty(start) => start,
            Event::Eof => break,
            _ => continue,
        };
        let is_root = patches.root_header_bytes.is_none();
        let mut root_growth = 0;
        for attribute in start.attributes() {
            let attribute = attribute.map_err(|error| {
                XmlBoundaryError::with_source(XmlBoundaryErrorKind::Output, error)
            })?;
            if attribute.key.as_ref() != b"xmlns" {
                continue;
            }
            // Entity decoding deliberately precedes XML attribute normalization.
            let raw_value = std::str::from_utf8(attribute.value.as_ref()).map_err(|error| {
                XmlBoundaryError::with_source(XmlBoundaryErrorKind::Output, error)
            })?;
            let decoded = quick_xml::escape::unescape(raw_value).map_err(|error| {
                XmlBoundaryError::with_source(XmlBoundaryErrorKind::Output, error)
            })?;
            if decoded != uri {
                continue;
            }
            let Cow::Borrowed(raw) = attribute.value else {
                return Err(refusal(
                    "namespace attribute must borrow the rendered document",
                ));
            };
            let offset = (raw.as_ptr() as usize).checked_sub(base).ok_or_else(|| {
                refusal("namespace attribute span is outside the rendered document")
            })?;
            let end = offset
                .checked_add(raw.len())
                .ok_or_else(|| refusal("namespace attribute span overflow"))?;
            if xml.as_bytes().get(offset..end) != Some(raw)
                || !xml.is_char_boundary(offset)
                || !xml.is_char_boundary(end)
            {
                return Err(refusal(
                    "namespace attribute span is outside the rendered document",
                ));
            }
            let extra = raw
                .iter()
                .filter(|byte| matches!(byte, b'\t' | b'\n' | b'\r'))
                .count()
                .checked_mul(4)
                .ok_or_else(|| refusal("namespace reference size overflow"))?;
            if extra == 0 {
                continue;
            }
            if patches
                .spans
                .last()
                .is_some_and(|previous| previous.end > offset)
            {
                return Err(refusal("namespace attribute spans overlap"));
            }
            patches.growth = patches
                .growth
                .checked_add(extra)
                .ok_or_else(|| refusal("namespace reference size overflow"))?;
            if is_root {
                root_growth += extra;
            }
            patches.spans.push(offset..end);
        }
        if is_root {
            patches.root_header_bytes = Some(
                start
                    .len()
                    .checked_add(root_growth)
                    .ok_or_else(|| refusal("namespace root header size overflow"))?,
            );
            if root_only {
                break;
            }
        }
    }
    Ok(patches)
}

pub(super) fn preserve(
    xml: String,
    schema: &ir::SchemaNode,
    instance: &ir::Instance,
    options: &format_xml::XmlWriteOptions,
) -> Result<String, XmlBoundaryError> {
    let effective_namespace = match &schema.xml_namespace {
        Some(ir::XmlNamespace::Qualified(uri)) => Some(uri.as_str()),
        Some(ir::XmlNamespace::Unqualified) => None,
        None => options.default_namespace.as_deref(),
    };
    let Some(uri) = effective_namespace.filter(|uri| uri.contains(['\t', '\n', '\r'])) else {
        return Ok(xml);
    };
    let patches = scan(&xml, uri, false)?;
    if patches.spans.is_empty() {
        return Ok(xml);
    }
    // Hints are appended after the native header cap. Only an uncommon near-cap
    // header needs a second serialization without those injected attributes.
    if options.schema_hints.is_some()
        && patches
            .root_header_bytes
            .is_some_and(|size| size > ROOT_HEADER_BYTES)
    {
        let mut no_hints = options.clone();
        no_hints.schema_hints = None;
        let preflight = format_xml::to_string_with_options(schema, instance, &no_hints)
            .map_err(|error| XmlBoundaryError::with_source(XmlBoundaryErrorKind::Output, error))?;
        if scan(&preflight, uri, true)?
            .root_header_bytes
            .is_some_and(|size| size > ROOT_HEADER_BYTES)
        {
            return Err(refusal(
                "XML schema hints conflict with the root attributes: root header exceeds 1 MiB",
            ));
        }
    }
    let expanded = xml
        .len()
        .checked_add(patches.growth)
        .ok_or_else(|| refusal("namespace reference size overflow"))?;
    check_document_size(expanded)?;
    // The exact complete size is checked before allocating the expanded buffer.
    let mut output = String::with_capacity(expanded);
    let mut cursor = 0;
    for span in patches.spans {
        output.push_str(&xml[cursor..span.start]);
        for character in xml[span.clone()].chars() {
            match character {
                '\t' => output.push_str("&#x9;"),
                '\n' => output.push_str("&#xA;"),
                '\r' => output.push_str("&#xD;"),
                _ => output.push(character),
            }
        }
        cursor = span.end;
    }
    output.push_str(&xml[cursor..]);
    debug_assert_eq!(output.len(), expanded);
    Ok(output)
}
