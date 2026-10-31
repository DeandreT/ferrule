//! A compact byte program that builds a candidate package.
//!
//! The first byte selects a [`templates`](crate::templates) starting package;
//! the remaining bytes are operations that add, rename, drop, alias and
//! rewrite entries. Every byte string decodes: reads past the end yield zero,
//! entry indexes wrap, and an operation on an empty package does nothing.
//! [`encode`] writes the same format for the seed generator.

use mfd::{MAX_PACKAGE_BYTES, MAX_PACKAGE_ENTRIES, MAX_PACKAGE_FILE_BYTES, MAX_PACKAGE_PATH_BYTES};

use crate::templates;

/// Most entries a program can build: enough to cross the entry limit.
pub const MAX_PROGRAM_ENTRIES: usize = MAX_PACKAGE_ENTRIES + 8;

/// Largest entry a program can build: one byte past the per-entry limit.
pub const MAX_PROGRAM_ENTRY_BYTES: usize = MAX_PACKAGE_FILE_BYTES + 1;

/// Largest total a program can build: one entry past the package limit.
pub const MAX_PROGRAM_BYTES: usize = MAX_PACKAGE_BYTES + MAX_PACKAGE_FILE_BYTES;

/// Most byte comparisons all `Substitute` operations of one program may do.
/// Each entry position compares up to `from.len()` bytes, so the cost of one
/// substitution is bounded by `entry.len() * from.len()`; a short program
/// substituting a long needle in 4 MiB entries over and over would otherwise
/// spend libFuzzer's whole timeout decoding. A `Substitute` whose bound would
/// cross this budget does nothing.
pub const MAX_SUBSTITUTE_SCAN: usize = 16 * MAX_PROGRAM_ENTRY_BYTES;

/// Longest include or reference chain one operation adds.
pub const MAX_CHAIN: usize = 64;

/// Filler entry sizes: empty, tiny, small, at and one past the entry limit.
pub const FILLER_SIZES: [usize; 5] = [
    0,
    1,
    4096,
    MAX_PACKAGE_FILE_BYTES,
    MAX_PACKAGE_FILE_BYTES + 1,
];

/// A decoded candidate package, before admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub template: &'static str,
    pub design: String,
    pub entries: Vec<(String, Vec<u8>)>,
}

impl Candidate {
    pub fn total_bytes(&self) -> usize {
        self.entries.iter().map(|(_, bytes)| bytes.len()).sum()
    }
}

/// Path strings an operation can name by index. Indexes below 0x80 select
/// from this pool (wrapping); others read a raw, lossily decoded string.
pub fn name_pool() -> Vec<String> {
    let mut pool: Vec<String> = [
        "design.mfd",
        "design-source.xsd",
        "design-target.schema.json",
        "design-source.schema.json",
        "design-target.xsd",
        "types.xsd",
        "types.schema.json",
        "source.dtd",
        "maps/design.mfd",
        "Design.mfd",
        "DESIGN-SOURCE.XSD",
        "sub/types.xsd",
        "../design-source.xsd",
        "./design-source.xsd",
        "sub/../design-source.xsd",
        "/etc/passwd",
        "C:/design-source.xsd",
        "https://example.invalid/types.xsd",
        "sub\\types.xsd",
        "",
        "a//b",
        "trailing.",
        "trailing ",
        "nul\0name",
        "tab\tname",
        ".",
        "..",
        "λ/ü.xsd",
        "fill-0.bin",
        "#/$defs/Missing",
        "types.schema.json#/$defs/Missing",
        "/__ferrule_package__/design-source.xsd",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    pool.push("p".repeat(MAX_PACKAGE_PATH_BYTES));
    pool.push("p".repeat(MAX_PACKAGE_PATH_BYTES + 1));
    pool.push(format!("{}/x.xsd", ["d"; 127].join("/")));
    pool
}

/// One operation of a program, as written by [`encode`].
#[derive(Debug, Clone, Copy)]
pub enum Op<'a> {
    /// Append an entry.
    Add(Name<'a>, &'a [u8]),
    /// Set the design path.
    Design(Name<'a>),
    /// Remove an entry.
    Drop(u8),
    /// Rename an entry.
    Rename(u8, Name<'a>),
    /// Replace an entry's contents.
    Replace(u8, &'a [u8]),
    /// Insert bytes at an offset (wrapping to the entry length plus one).
    Splice(u8, u16, &'a [u8]),
    /// Keep a prefix of an entry (wrapping to the entry length plus one).
    Truncate(u8, u16),
    /// Replace every occurrence of one string in an entry with another.
    Substitute(u8, Name<'a>, Name<'a>),
    /// Copy an entry under an alias of its name.
    Alias(u8, Alias),
    /// Move an XSD or JSON Schema entry behind a chain of includes or
    /// references in the same directory.
    Chain(u8, u8, Chain),
    /// Append filler entries of one of [`FILLER_SIZES`].
    Filler(u8, u8),
    /// Surround an entry with a repeated prefix and suffix.
    Wrap(u8, u16, &'a [u8], &'a [u8]),
}

#[derive(Debug, Clone, Copy)]
pub enum Name<'a> {
    Pool(&'a str),
    Raw(&'a [u8]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alias {
    Exact,
    Upper,
    Lower,
    FirstLetterCase,
}

/// Chain shape: complete, a cycle back to the first link, or a dangling
/// final reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chain {
    Complete,
    Cycle,
    Dangling,
}

const OPS: u8 = 12;

/// Writes a program for `template` (by name) and `ops`.
pub fn encode(template: &str, ops: &[Op<'_>]) -> Vec<u8> {
    let templates = templates::all();
    let index = templates
        .iter()
        .position(|candidate| candidate.name == template)
        .unwrap_or_else(|| panic!("unknown template {template}"));
    let pool = name_pool();
    let mut out = vec![index as u8];
    for op in ops {
        let mut operands = Vec::new();
        let code = match *op {
            Op::Add(name, bytes) => {
                put_name(&mut operands, &pool, name);
                put_blob(&mut operands, bytes);
                0
            }
            Op::Design(name) => {
                put_name(&mut operands, &pool, name);
                1
            }
            Op::Drop(index) => {
                operands.push(index);
                2
            }
            Op::Rename(index, name) => {
                operands.push(index);
                put_name(&mut operands, &pool, name);
                3
            }
            Op::Replace(index, bytes) => {
                operands.push(index);
                put_blob(&mut operands, bytes);
                4
            }
            Op::Splice(index, offset, bytes) => {
                operands.push(index);
                operands.extend(offset.to_le_bytes());
                put_blob(&mut operands, bytes);
                5
            }
            Op::Truncate(index, length) => {
                operands.push(index);
                operands.extend(length.to_le_bytes());
                6
            }
            Op::Substitute(index, from, to) => {
                operands.push(index);
                put_name(&mut operands, &pool, from);
                put_name(&mut operands, &pool, to);
                7
            }
            Op::Alias(index, alias) => {
                operands.extend([index, alias as u8]);
                8
            }
            Op::Chain(index, depth, shape) => {
                operands.extend([index, depth, shape as u8]);
                9
            }
            Op::Filler(count, size) => {
                operands.extend([count, size]);
                10
            }
            Op::Wrap(index, count, prefix, suffix) => {
                operands.push(index);
                operands.extend(count.to_le_bytes());
                put_blob(&mut operands, prefix);
                put_blob(&mut operands, suffix);
                11
            }
        };
        out.push(code);
        out.extend(operands);
    }
    out
}

fn put_name(out: &mut Vec<u8>, pool: &[String], name: Name<'_>) {
    match name {
        Name::Pool(text) => {
            let index = pool
                .iter()
                .position(|candidate| candidate == text)
                .unwrap_or_else(|| panic!("{text:?} is not in the name pool"));
            out.push(index as u8);
        }
        Name::Raw(bytes) => {
            out.push(0x80);
            put_blob(out, bytes);
        }
    }
}

fn put_blob(out: &mut Vec<u8>, bytes: &[u8]) {
    let length = u16::try_from(bytes.len()).unwrap_or_else(|_| panic!("blob over 64 KiB"));
    out.extend(length.to_le_bytes());
    out.extend(bytes);
}

/// Decodes a program into a candidate package within the program limits.
pub fn decode(bytes: &[u8]) -> Candidate {
    let mut templates = templates::all();
    let mut reader = Reader { bytes, at: 0 };
    let template = templates.swap_remove(usize::from(reader.byte()) % templates.len());
    let mut candidate = Candidate {
        template: template.name,
        design: template.design.to_owned(),
        entries: template.entries,
    };
    let pool = name_pool();
    let mut scan_budget = MAX_SUBSTITUTE_SCAN;
    while !reader.done() {
        let code = reader.byte() % OPS;
        apply(&mut candidate, &mut reader, &pool, code, &mut scan_budget);
        enforce_limits(&mut candidate);
    }
    candidate
}

fn apply(
    candidate: &mut Candidate,
    reader: &mut Reader<'_>,
    pool: &[String],
    code: u8,
    scan_budget: &mut usize,
) {
    let entries = &mut candidate.entries;
    match code {
        0 => {
            let name = reader.name(pool);
            let contents = reader.blob().to_vec();
            if entries.len() < MAX_PROGRAM_ENTRIES {
                entries.push((name, contents));
            }
        }
        1 => candidate.design = reader.name(pool),
        2 => {
            let index = reader.byte();
            if let Some(index) = wrap(index, entries.len()) {
                entries.remove(index);
            }
        }
        3 => {
            let index = reader.byte();
            let name = reader.name(pool);
            if let Some(index) = wrap(index, entries.len()) {
                entries[index].0 = name;
            }
        }
        4 => {
            let index = reader.byte();
            let contents = reader.blob();
            if let Some(index) = wrap(index, entries.len()) {
                entries[index].1 = contents.to_vec();
            }
        }
        5 => {
            let index = reader.byte();
            let offset = usize::from(reader.u16());
            let insert = reader.blob();
            if let Some(index) = wrap(index, entries.len()) {
                let contents = &mut entries[index].1;
                let at = offset % (contents.len() + 1);
                contents.splice(at..at, insert.iter().copied());
            }
        }
        6 => {
            let index = reader.byte();
            let length = usize::from(reader.u16());
            if let Some(index) = wrap(index, entries.len()) {
                let contents = &mut entries[index].1;
                contents.truncate(length % (contents.len() + 1));
            }
        }
        7 => {
            let index = reader.byte();
            let from = reader.name(pool);
            let to = reader.name(pool);
            if let Some(index) = wrap(index, entries.len()) {
                // Charge the worst-case comparison count, entry length times
                // the needle length, so a long needle cannot make one op scan
                // for the whole timeout (an entry byte is compared against up
                // to the whole needle before a mismatch).
                let cost = entries[index].1.len().saturating_mul(from.len().max(1));
                if let Some(left) = scan_budget.checked_sub(cost) {
                    *scan_budget = left;
                    let cap = entry_cap(entries, index);
                    let contents = &mut entries[index].1;
                    *contents = substitute(contents, from.as_bytes(), to.as_bytes(), cap);
                }
            }
        }
        8 => {
            let index = reader.byte();
            let alias = reader.byte();
            if let Some(index) = wrap(index, entries.len())
                && entries.len() < MAX_PROGRAM_ENTRIES
            {
                let cap = entry_cap(entries, entries.len());
                let (name, contents) = &entries[index];
                let contents = contents[..contents.len().min(cap)].to_vec();
                entries.push((alias_name(name, alias), contents));
            }
        }
        9 => {
            let index = reader.byte();
            let depth = usize::from(reader.byte()) % (MAX_CHAIN + 1);
            let shape = match reader.byte() % 3 {
                0 => Chain::Complete,
                1 => Chain::Cycle,
                _ => Chain::Dangling,
            };
            if let Some(index) = wrap(index, entries.len()) {
                chain(entries, index, depth, shape);
            }
        }
        10 => {
            let count = usize::from(reader.byte());
            let size = FILLER_SIZES[usize::from(reader.byte()) % FILLER_SIZES.len()];
            for _ in 0..count {
                if entries.len() >= MAX_PROGRAM_ENTRIES || total(entries) + size > MAX_PROGRAM_BYTES
                {
                    break;
                }
                let name = format!("fill-{}.bin", entries.len());
                entries.push((name, vec![b'#'; size]));
            }
        }
        _ => {
            let index = reader.byte();
            let count = usize::from(reader.u16());
            let prefix = reader.blob();
            let suffix = reader.blob();
            if let Some(index) = wrap(index, entries.len()) {
                let cap = entry_cap(entries, index);
                let contents = std::mem::take(&mut entries[index].1);
                let pieces = std::iter::repeat_n(prefix, count)
                    .chain([contents.as_slice()])
                    .chain(std::iter::repeat_n(suffix, count));
                let mut wrapped = Vec::new();
                for piece in pieces {
                    let room = cap - wrapped.len();
                    wrapped.extend_from_slice(&piece[..piece.len().min(room)]);
                    if wrapped.len() == cap {
                        break;
                    }
                }
                entries[index].1 = wrapped;
            }
        }
    }
}

/// Clamps a candidate to the program limits after every operation, so no
/// input makes the harness allocate more than [`MAX_PROGRAM_BYTES`] of
/// entry contents.
///
/// Operations that can multiply an entry (`Substitute`, `Wrap`) or copy one
/// (`Alias`, `Chain`) stop building at [`entry_cap`], which yields the same
/// candidate this clamp would. The rest add at most one blob of 64 KiB.
fn enforce_limits(candidate: &mut Candidate) {
    candidate.entries.truncate(MAX_PROGRAM_ENTRIES);
    let mut remaining = MAX_PROGRAM_BYTES;
    for (_, contents) in &mut candidate.entries {
        contents.truncate(MAX_PROGRAM_ENTRY_BYTES.min(remaining));
        remaining -= contents.len();
    }
}

/// Most bytes entry `index` keeps after [`enforce_limits`]: the entry limit,
/// or what the entries before it leave of the total.
fn entry_cap(entries: &[(String, Vec<u8>)], index: usize) -> usize {
    MAX_PROGRAM_ENTRY_BYTES.min(MAX_PROGRAM_BYTES.saturating_sub(total(&entries[..index])))
}

fn total(entries: &[(String, Vec<u8>)]) -> usize {
    entries.iter().map(|(_, contents)| contents.len()).sum()
}

fn wrap(index: u8, len: usize) -> Option<usize> {
    (len > 0).then(|| usize::from(index) % len)
}

/// `contents` with every `from` replaced by `to`, cut at `cap` bytes.
/// Building stops at the cap, so a short `from` with a long `to` cannot
/// expand a 4 MiB entry into gigabytes first.
fn substitute(contents: &[u8], from: &[u8], to: &[u8], cap: usize) -> Vec<u8> {
    if from.is_empty() {
        return contents[..contents.len().min(cap)].to_vec();
    }
    let mut out = Vec::with_capacity(contents.len().min(cap));
    let mut at = 0;
    while at < contents.len() && out.len() < cap {
        if contents[at..].starts_with(from) {
            out.extend_from_slice(to);
            at += from.len();
        } else {
            out.push(contents[at]);
            at += 1;
        }
    }
    out.truncate(cap);
    out
}

fn alias_name(name: &str, alias: u8) -> String {
    match alias % 4 {
        0 => name.to_owned(),
        1 => name.to_ascii_uppercase(),
        2 => name.to_ascii_lowercase(),
        _ => {
            let mut toggled = false;
            name.chars()
                .map(|character| {
                    if !toggled && character.is_ascii_alphabetic() {
                        toggled = true;
                        if character.is_ascii_uppercase() {
                            character.to_ascii_lowercase()
                        } else {
                            character.to_ascii_uppercase()
                        }
                    } else {
                        character
                    }
                })
                .collect()
        }
    }
}

/// Replaces entry `index` with the first of `depth` links whose last link
/// holds the original contents (or, for a cycle, refers back to the first
/// link; for a dangling chain, the last link is omitted).
fn chain(entries: &mut Vec<(String, Vec<u8>)>, index: usize, depth: usize, shape: Chain) {
    let (name, original) = entries[index].clone();
    let json = match original.iter().find(|byte| !byte.is_ascii_whitespace()) {
        Some(b'<') => false,
        Some(b'{') => true,
        _ => return,
    };
    if depth == 0 {
        return;
    }
    let directory = name.rsplit_once('/').map_or("", |(directory, _)| directory);
    let extension = if json { "schema.json" } else { "xsd" };
    let tag = entries.len();
    let link_name = |link: usize| format!("c{tag}-{link}.{extension}");
    let full = |link: usize| {
        if directory.is_empty() {
            link_name(link)
        } else {
            format!("{directory}/{}", link_name(link))
        }
    };
    let link = |target: &str| -> Vec<u8> {
        if json {
            format!(r#"{{"$ref":"{target}"}}"#).into_bytes()
        } else {
            format!(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:include schemaLocation="{target}"/></xs:schema>"#
            )
            .into_bytes()
        }
    };
    entries[index].1 = link(&link_name(0));
    for position in 0..depth {
        if entries.len() >= MAX_PROGRAM_ENTRIES {
            return;
        }
        let last = position + 1 == depth;
        let contents = match (last, shape) {
            (false, _) => link(&link_name(position + 1)),
            (true, Chain::Complete) => {
                let cap = entry_cap(entries, entries.len());
                original[..original.len().min(cap)].to_vec()
            }
            (true, Chain::Cycle) => link(&link_name(0)),
            (true, Chain::Dangling) => return,
        };
        entries.push((full(position), contents));
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn done(&self) -> bool {
        self.at >= self.bytes.len()
    }

    fn byte(&mut self) -> u8 {
        let byte = self.bytes.get(self.at).copied().unwrap_or(0);
        self.at += 1;
        byte
    }

    fn u16(&mut self) -> u16 {
        u16::from_le_bytes([self.byte(), self.byte()])
    }

    fn blob(&mut self) -> &'a [u8] {
        let length = usize::from(self.u16());
        let start = self.at.min(self.bytes.len());
        let end = start.saturating_add(length).min(self.bytes.len());
        self.at = self.at.saturating_add(length);
        &self.bytes[start..end]
    }

    fn name(&mut self, pool: &[String]) -> String {
        let selector = self.byte();
        if selector < 0x80 {
            pool[usize::from(selector) % pool.len()].clone()
        } else {
            String::from_utf8_lossy(self.blob()).into_owned()
        }
    }
}
