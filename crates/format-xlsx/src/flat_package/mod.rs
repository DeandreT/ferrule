//! Portable, stored ZIP profile shared with the generated flat workbook adapter.
use crate::{XlsxFormatError as Error, validate_row};
use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use std::fmt::Write;
const MAX: usize = 256 * 1024;
fn refused() -> Error {
    Error::UnsupportedBoundedProfile("portable flat workbook")
}

/// Read only packages which reproduce their entire original byte sequence.
/// Bound expansion and coordinates before asking the native reader for rows.
pub fn from_flat_package(
    bytes: &[u8],
    schema: &SchemaNode,
    sheet: &str,
) -> Result<Vec<Instance>, Error> {
    let limits = crate::XlsxLimits::new(crate::XlsxLimitOptions {
        max_input_bytes: MAX,
        max_entries: 5,
        max_entry_bytes: MAX,
        max_expanded_bytes: MAX,
        max_rows: 257,
        max_columns: 26,
        max_cells: 257 * 26,
        max_dense_cells: 257 * 26,
        max_xml_depth: 8,
        ..Default::default()
    })?;
    let rows = crate::from_bytes_bounded(bytes, schema, Some(sheet), 1, &[], true, &limits)?;
    if to_flat_package(schema, &rows, sheet)? != bytes {
        return Err(refused());
    }
    Ok(rows)
}
/// Write 0..=256 complete flat rows, preserving declaration order and original strings.
/// This versioned writer is separate from the general-purpose XLSX package renderer.
pub fn to_flat_package(
    schema: &SchemaNode,
    rows: &[Instance],
    sheet: &str,
) -> Result<Vec<u8>, Error> {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Err(refused());
    };
    let safe = |s: &str, max| {
        !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    };
    if !safe(sheet, 31)
        || sheet.eq_ignore_ascii_case("history")
        || children.is_empty()
        || children.len() > 26
        || rows.len() > 256
        || *schema != SchemaNode::group(&schema.name, children.clone())
    {
        return Err(refused());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut fields = Vec::new();
    for c in children {
        let SchemaKind::Scalar { ty } = c.kind else {
            return Err(refused());
        };
        if !matches!(ty, ScalarType::String | ScalarType::Int | ScalarType::Bool)
            || !safe(&c.name, 64)
            || !seen.insert(&c.name)
            || *c != SchemaNode::scalar(&c.name, ty)
        {
            return Err(refused());
        }
        fields.push((c.name.as_str(), ty));
    }
    let mut xml = String::from(
        "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?><worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>",
    );
    for i in 0..=rows.len() {
        let row = i + 1;
        write!(xml, "<row r=\"{row}\">").map_err(|_| refused())?;
        let values = if i > 0 {
            if !matches!(&rows[i-1], Instance::Group(values) if values.len() == fields.len()) {
                return Err(refused());
            }
            Some(validate_row(i - 1, &rows[i - 1], &fields)?)
        } else {
            None
        };
        for (j, (name, ty)) in fields.iter().enumerate() {
            let col = (b'A' + j as u8) as char;
            let value = values.as_ref().map(|v| v[j]);
            let text = match (value, ty) {
                (None, _) => Some(*name),
                (Some(Value::String(s)), ScalarType::String)
                    if !s.is_empty()
                        && s.len() <= 4096
                        && s.encode_utf16().count() <= 1024
                        && !s.contains('_')
                        && !s
                            .chars()
                            .any(|c| c.is_control() || matches!(c, '\u{fffe}' | '\u{ffff}')) =>
                {
                    Some(s.as_str())
                }
                (Some(Value::Int(n)), ScalarType::Int) if n.unsigned_abs() <= 999999999999999 => {
                    write!(xml, "<c r=\"{col}{row}\" t=\"n\"><v>{n}</v></c>")
                        .map_err(|_| refused())?;
                    None
                }
                (Some(Value::Bool(b)), ScalarType::Bool) => {
                    write!(
                        xml,
                        "<c r=\"{col}{row}\" t=\"b\"><v>{}</v></c>",
                        u8::from(*b)
                    )
                    .map_err(|_| refused())?;
                    None
                }
                _ => return Err(refused()),
            };
            if let Some(s) = text {
                write!(xml,"<c r=\"{col}{row}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",s.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;")).map_err(|_|refused())?;
            }
            if xml.len() > MAX {
                return Err(refused());
            }
        }
        xml.push_str("</row>");
    }
    xml.push_str("</sheetData></worksheet>");
    let book = include_str!("part2.xml").replace("SHEET_NAME", sheet);
    let parts = [
        (
            "[Content_Types].xml",
            include_bytes!("part0.xml").as_slice(),
        ),
        ("_rels/.rels", include_bytes!("part1.xml").as_slice()),
        ("xl/workbook.xml", book.as_bytes()),
        (
            "xl/_rels/workbook.xml.rels",
            include_bytes!("part3.xml").as_slice(),
        ),
        ("xl/worksheets/sheet1.xml", xml.as_bytes()),
    ];
    package(&parts)
}
// Fixed classic ZIP, stored entries, DOS epoch, Linux regular-file mode. No descriptors, extensions or comments.
fn package(parts: &[(&str, &[u8])]) -> Result<Vec<u8>, Error> {
    let mut b = Vec::new();
    let mut dir = Vec::new();
    for &(name, data) in parts {
        let offset = b.len() as u32;
        let len = data.len() as u32;
        let crc = crc32fast::hash(data);
        b.extend(0x04034b50u32.to_le_bytes());
        dir.extend(0x02014b50u32.to_le_bytes());
        dir.extend(0x0314u16.to_le_bytes());
        for buf in [&mut b, &mut dir] {
            for v in [20u16, 0, 0, 0, 33] {
                buf.extend(v.to_le_bytes())
            }
            for v in [crc, len, len] {
                buf.extend(v.to_le_bytes())
            }
            buf.extend((name.len() as u16).to_le_bytes());
            buf.extend([0; 2]);
        }
        dir.extend([0; 6]);
        dir.extend(0x81a40000u32.to_le_bytes());
        dir.extend(offset.to_le_bytes());
        dir.extend(name.bytes());
        b.extend(name.bytes());
        b.extend(data);
    }
    let start = b.len() as u32;
    let size = dir.len() as u32;
    if b.len() + dir.len() + 22 > MAX {
        return Err(refused());
    }
    b.extend(dir);
    b.extend(0x06054b50u32.to_le_bytes());
    b.extend([0; 4]);
    for _ in 0..2 {
        b.extend((parts.len() as u16).to_le_bytes())
    }
    b.extend(size.to_le_bytes());
    b.extend(start.to_le_bytes());
    b.extend([0; 2]);
    Ok(b)
}
