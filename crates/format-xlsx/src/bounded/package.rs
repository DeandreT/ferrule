use std::collections::BTreeSet;
use std::io::{Cursor, Read};

use crate::XlsxFormatError;

use super::{XlsxLimit, XlsxLimitOptions, add, check, invalid, unsupported, xml};

pub(super) struct Sheet {
    pub name: String,
    pub last_row: usize,
    pub integer_risks: Vec<(usize, usize, String)>,
}

pub(super) struct Summary {
    pub sheets: Vec<Sheet>,
    pub cell_text_bytes: usize,
}

struct Entry<'a> {
    name: &'a str,
    expanded: usize,
}

fn u16_at(bytes: &[u8], pos: usize) -> Result<usize, XlsxFormatError> {
    let b = bytes
        .get(pos..pos + 2)
        .ok_or_else(|| invalid("truncated ZIP"))?;
    Ok(u16::from_le_bytes([b[0], b[1]]) as usize)
}

fn u32_at(bytes: &[u8], pos: usize) -> Result<usize, XlsxFormatError> {
    let b = bytes
        .get(pos..pos + 4)
        .ok_or_else(|| invalid("truncated ZIP"))?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
}

fn signature(bytes: &[u8], pos: usize, expected: &[u8]) -> Result<(), XlsxFormatError> {
    if bytes.get(pos..pos + expected.len()) != Some(expected) {
        return Err(invalid("invalid ZIP signature or non-contiguous archive"));
    }
    Ok(())
}

fn part_name(name: &str) -> bool {
    matches!(
        name,
        "[Content_Types].xml"
            | "_rels/.rels"
            | "docProps/app.xml"
            | "docProps/core.xml"
            | "xl/workbook.xml"
            | "xl/_rels/workbook.xml.rels"
            | "xl/styles.xml"
            | "xl/sharedStrings.xml"
    ) || numbered_part(name, "xl/worksheets/sheet")
        || numbered_part(name, "xl/theme/theme")
}

fn numbered_part(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .and_then(|s| s.strip_suffix(".xml"))
        .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}

/// Inspect the complete classic ZIP directory *before* ZipArchive allocates its
/// file table. Requiring one canonical representation also prevents differences
/// between zip's and calamine's duplicate/case-insensitive path lookup.
fn directory<'a>(
    bytes: &'a [u8],
    limits: &XlsxLimitOptions,
) -> Result<Vec<Entry<'a>>, XlsxFormatError> {
    let end = bytes
        .len()
        .checked_sub(22)
        .ok_or_else(|| invalid("truncated ZIP"))?;
    signature(bytes, end, b"PK\x05\x06")?;
    if u16_at(bytes, end + 4)? != 0 || u16_at(bytes, end + 6)? != 0 {
        return Err(unsupported("multi-disk ZIP"));
    }
    let count = u16_at(bytes, end + 10)?;
    if count == 65_535
        || u32_at(bytes, end + 12)? == u32::MAX as usize
        || u32_at(bytes, end + 16)? == u32::MAX as usize
    {
        return Err(unsupported("ZIP64"));
    }
    check(XlsxLimit::Entries, count, limits.max_entries)?;
    if count != u16_at(bytes, end + 8)? || u16_at(bytes, end + 20)? != 0 {
        return Err(unsupported("ZIP comments or inconsistent disk directory"));
    }
    let start = u32_at(bytes, end + 16)?;
    if start.checked_add(u32_at(bytes, end + 12)?) != Some(end) {
        return Err(invalid("invalid ZIP directory extent"));
    }
    let mut offset = start;
    let mut entries = Vec::new();
    let mut names = BTreeSet::new();
    let mut extents = Vec::new();
    for _ in 0..count {
        if offset
            .checked_add(46)
            .is_none_or(|end_of_header| end_of_header > end)
        {
            return Err(invalid("truncated central ZIP header"));
        }
        signature(bytes, offset, b"PK\x01\x02")?;
        let flags = u16_at(bytes, offset + 8)?;
        let compression = u16_at(bytes, offset + 10)?;
        if flags & !0x0808 != 0 || !matches!(compression, 0 | 8) {
            return Err(unsupported("ZIP encryption, flags or compression method"));
        }
        let compressed = u32_at(bytes, offset + 20)?;
        let expanded = u32_at(bytes, offset + 24)?;
        let name_len = u16_at(bytes, offset + 28)?;
        let extra_len = u16_at(bytes, offset + 30)?;
        let comment_len = u16_at(bytes, offset + 32)?;
        if extra_len != 0 || comment_len != 0 || u16_at(bytes, offset + 34)? != 0 {
            return Err(unsupported("ZIP extra fields, comments or multiple disks"));
        }
        let name = bytes
            .get(offset + 46..offset + 46 + name_len)
            .ok_or_else(|| invalid("truncated ZIP name"))?;
        let name = std::str::from_utf8(name).map_err(|_| unsupported("non-UTF-8 ZIP name"))?;
        if !name.is_ascii() || !names.insert(name.to_ascii_lowercase()) {
            return Err(invalid("duplicate or ambiguous ZIP part name"));
        }
        if !part_name(name) {
            return Err(unsupported("part outside the flat workbook profile"));
        }
        let mode = u32_at(bytes, offset + 38)? >> 16;
        if mode & 0xf000 != 0 && mode & 0xf000 != 0x8000 {
            return Err(unsupported("non-regular ZIP entry"));
        }
        let local = u32_at(bytes, offset + 42)?;
        if local >= start {
            return Err(invalid("local ZIP header outside entry area"));
        }
        signature(bytes, local, b"PK\x03\x04")?;
        if flags != u16_at(bytes, local + 6)?
            || compression != u16_at(bytes, local + 8)?
            || name_len != u16_at(bytes, local + 26)?
            || u16_at(bytes, local + 28)? != 0
            || bytes.get(local + 30..local + 30 + name_len) != Some(name.as_bytes())
        {
            return Err(invalid("local and central ZIP headers disagree"));
        }
        if flags & 8 == 0 {
            if bytes.get(local + 14..local + 26) != bytes.get(offset + 16..offset + 28) {
                return Err(invalid("local and central ZIP sizes/CRC disagree"));
            }
        } else if [14, 18, 22]
            .into_iter()
            .any(|i| u32_at(bytes, local + i).is_ok_and(|n| n != 0))
        {
            return Err(unsupported("nonzero local sizes with a ZIP descriptor"));
        }
        let mut data_end = local
            .checked_add(30 + name_len)
            .and_then(|p| p.checked_add(compressed))
            .ok_or_else(|| invalid("ZIP offset overflow"))?;
        if data_end > start {
            return Err(invalid("ZIP data extends into its directory"));
        }
        if flags & 8 != 0 {
            if bytes.get(data_end..data_end + 4) == Some(b"PK\x07\x08") {
                data_end += 4;
            }
            if bytes.get(data_end..data_end + 12) != bytes.get(offset + 16..offset + 28) {
                return Err(invalid("ZIP descriptor disagrees with directory"));
            }
            data_end += 12;
        }
        if data_end > start {
            return Err(invalid("overlapping ZIP entries or directory"));
        }
        extents.push((local, data_end));
        entries.push(Entry { name, expanded });
        offset += 46 + name_len;
    }
    if offset != end {
        return Err(invalid("ZIP entry count disagrees with directory"));
    }
    extents.sort_unstable();
    let mut next = 0;
    for (start, end) in extents {
        if start != next {
            return Err(invalid("overlapping, hidden or non-contiguous ZIP entries"));
        }
        next = end;
    }
    if next != start {
        return Err(invalid("ZIP prefix or hidden local entries"));
    }
    for required in [
        "[content_types].xml",
        "_rels/.rels",
        "xl/workbook.xml",
        "xl/_rels/workbook.xml.rels",
    ] {
        if !names.contains(required) {
            return Err(invalid("missing required workbook part"));
        }
    }
    Ok(entries)
}

pub(super) fn validate(
    bytes: &[u8],
    limits: &XlsxLimitOptions,
) -> Result<Summary, XlsxFormatError> {
    let entries = directory(bytes, limits)?;
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| invalid(e.to_string()))?;
    if archive.len() != entries.len() {
        return Err(invalid("ambiguous ZIP directory"));
    }
    let mut total = 0;
    let mut parts = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let mut file = archive
            .by_index(index)
            .map_err(|e| invalid(e.to_string()))?;
        let mut content = Vec::new();
        let mut chunk = [0; 8192];
        loop {
            // Probe only one byte beyond either budget; never use declared
            // uncompressed lengths for reservation or decompression limits.
            let remaining = limits
                .max_entry_bytes
                .saturating_sub(content.len())
                .min(limits.max_expanded_bytes.saturating_sub(total));
            let size = chunk.len().min(remaining.saturating_add(1));
            let read = file
                .read(&mut chunk[..size])
                .map_err(|e| invalid(e.to_string()))?;
            if read == 0 {
                break;
            }
            check(
                XlsxLimit::EntryBytes,
                content.len() + read,
                limits.max_entry_bytes,
            )?;
            add(
                &mut total,
                read,
                XlsxLimit::ExpandedBytes,
                limits.max_expanded_bytes,
            )?;
            content.extend_from_slice(&chunk[..read]);
        }
        if content.len() != entry.expanded {
            return Err(invalid("actual expanded size disagrees with ZIP directory"));
        }
        parts.push((entry.name, content));
    }
    xml::validate(&parts, limits)
}
