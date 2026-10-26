use std::collections::{BTreeMap, BTreeSet};

use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};

use super::{
    XlsxLimit, XlsxLimitOptions, add, check, invalid,
    package::{Sheet, Summary},
    unsupported,
};
use crate::XlsxFormatError;

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const DOCUMENT_RELS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";

#[derive(Default)]
struct Budget {
    nodes: usize,
    text: usize,
    cells: usize,
    cell_text: usize,
}

type Attributes = BTreeMap<String, String>;

fn attr<'a>(attrs: &'a Attributes, key: &str) -> Result<&'a str, XlsxFormatError> {
    attrs
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| invalid(format!("missing {key} attribute")))
}

fn number(value: &str) -> Result<usize, XlsxFormatError> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid("invalid unsigned integer"));
    }
    value.parse().map_err(|_| invalid("integer overflow"))
}

fn attributes(
    e: &BytesStart<'_>,
    reader: &Reader<&[u8]>,
    budget: &mut Budget,
    limits: &XlsxLimitOptions,
    operational: bool,
) -> Result<Attributes, XlsxFormatError> {
    let mut result = BTreeMap::new();
    let mut local_names = BTreeSet::new();
    for a in e.attributes() {
        let a = a.map_err(|e| invalid(e.to_string()))?;
        let name =
            std::str::from_utf8(a.key.as_ref()).map_err(|_| invalid("invalid attribute name"))?;
        // Calamine's sheet reader matches the local `id` name and takes the
        // last occurrence. Never let its selection differ from this guard's.
        if operational && name != "xmlns" && !name.starts_with("xmlns:") {
            let local = name.rsplit(':').next().unwrap_or(name);
            if !local_names.insert(local.to_string()) {
                return Err(invalid("competing XML attributes with the same local name"));
            }
            // These attributes are consumed as raw bytes by calamine. XML
            // unescaping must not change their meaning only in the preflight.
            if matches!(
                name,
                "r:id"
                    | "Id"
                    | "Type"
                    | "Target"
                    | "TargetMode"
                    | "r"
                    | "s"
                    | "t"
                    | "ref"
                    | "uniqueCount"
                    | "date1904"
                    | "state"
                    | "numFmtId"
                    | "xml:space"
            ) && a
                .value
                .iter()
                .any(|b| matches!(b, b'&' | b'\r' | b'\n' | b'\t'))
            {
                return Err(unsupported("escaped or normalized structural attribute"));
            }
        }
        // Check raw lengths before the XML unescaper can allocate.
        check(
            XlsxLimit::StringBytes,
            a.value.len(),
            limits.max_string_bytes,
        )?;
        // Match calamine's attribute decoding (without XML whitespace
        // normalization), so names/relationship IDs select the same parts.
        let decoded = reader
            .decoder()
            .decode(&a.value)
            .map_err(|e| invalid(e.to_string()))?;
        let value = quick_xml::escape::unescape(&decoded).map_err(|e| invalid(e.to_string()))?;
        add(
            &mut budget.text,
            value.len(),
            XlsxLimit::XmlTextBytes,
            limits.max_xml_text_bytes,
        )?;
        if result
            .insert(name.to_string(), value.into_owned())
            .is_some()
        {
            return Err(invalid("duplicate XML attribute"));
        }
    }
    Ok(result)
}

trait Visitor {
    fn start(&mut self, _path: &[String], _attrs: &Attributes) -> Result<(), XlsxFormatError> {
        Ok(())
    }
    fn text(&mut self, _path: &[String], _text: &str) -> Result<(), XlsxFormatError> {
        Ok(())
    }
    fn end(&mut self, _path: &[String]) -> Result<(), XlsxFormatError> {
        Ok(())
    }
}

struct Ignore;
impl Visitor for Ignore {}

#[derive(Default)]
struct ContentTypes {
    names: BTreeSet<String>,
    defaults: BTreeSet<String>,
}

impl Visitor for ContentTypes {
    fn start(&mut self, path: &[String], attrs: &Attributes) -> Result<(), XlsxFormatError> {
        if path.len() == 1 {
            return Ok(());
        }
        if path.len() != 2 {
            return Err(unsupported("content types shape"));
        }
        let ty = attr(attrs, "ContentType")?;
        match path[1].as_str() {
            "Default" => {
                let extension = attr(attrs, "Extension")?;
                if !matches!(
                    (extension, ty),
                    (
                        "rels",
                        "application/vnd.openxmlformats-package.relationships+xml"
                    ) | ("xml", "application/xml")
                ) {
                    return Err(unsupported("content type outside flat workbook profile"));
                }
                if !self.defaults.insert(extension.to_string()) {
                    return Err(invalid("duplicate default content type"));
                }
            }
            "Override" => {
                let name = attr(attrs, "PartName")?;
                let expected = match name {
                    "/xl/workbook.xml" => {
                        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"
                    }
                    "/xl/styles.xml" => {
                        "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"
                    }
                    "/xl/sharedStrings.xml" => {
                        "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"
                    }
                    "/docProps/core.xml" => {
                        "application/vnd.openxmlformats-package.core-properties+xml"
                    }
                    "/docProps/app.xml" => {
                        "application/vnd.openxmlformats-officedocument.extended-properties+xml"
                    }
                    _ if name.starts_with("/xl/worksheets/sheet") => {
                        "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"
                    }
                    _ if name.starts_with("/xl/theme/theme") => {
                        "application/vnd.openxmlformats-officedocument.theme+xml"
                    }
                    _ => {
                        return Err(unsupported(
                            "content type part outside flat workbook profile",
                        ));
                    }
                };
                if ty != expected {
                    return Err(unsupported("non-XLSX content type"));
                }
                if !self.names.insert(name.to_string()) {
                    return Err(invalid("duplicate content type override"));
                }
            }
            _ => return Err(unsupported("content types shape")),
        }
        Ok(())
    }
}

fn scan(
    bytes: &[u8],
    root: Option<(&str, &str)>,
    visitor: &mut impl Visitor,
    budget: &mut Budget,
    limits: &XlsxLimitOptions,
) -> Result<(), XlsxFormatError> {
    std::str::from_utf8(bytes).map_err(|_| unsupported("XML must be UTF-8"))?;
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().check_comments = true;
    let mut path = Vec::<String>::new();
    let mut roots = 0;
    loop {
        let event = reader.read_event().map_err(|e| invalid(e.to_string()))?;
        if matches!(event, Event::Eof) {
            break;
        }
        add(
            &mut budget.nodes,
            1,
            XlsxLimit::XmlNodes,
            limits.max_xml_nodes,
        )?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                check(XlsxLimit::XmlDepth, path.len() + 1, limits.max_xml_depth)?;
                let name = std::str::from_utf8(e.name().as_ref())
                    .map_err(|_| invalid("invalid element name"))?
                    .to_string();
                let attrs = attributes(e, &reader, budget, limits, root.is_some())?;
                if path.is_empty() {
                    roots += 1;
                    if roots != 1 {
                        return Err(invalid("multiple XML roots"));
                    }
                    if let Some((expected, namespace)) = root {
                        if name != expected
                            || attrs.get("xmlns").map(String::as_str) != Some(namespace)
                        {
                            return Err(unsupported("non-transitional or namespace-ambiguous XML"));
                        }
                        for (key, value) in &attrs {
                            if key.starts_with("xmlns:")
                                && !(key == "xmlns:r"
                                    && value == DOCUMENT_RELS.trim_end_matches('/'))
                            {
                                return Err(unsupported("unsupported XML namespace"));
                            }
                        }
                    }
                } else if root.is_some()
                    && attrs
                        .keys()
                        .any(|key| key == "xmlns" || key.starts_with("xmlns:"))
                {
                    return Err(unsupported("nested XML namespace declarations"));
                }
                if root.is_some()
                    && (name.contains(':')
                        || attrs.keys().any(|key| {
                            key.contains(':')
                                && !matches!(key.as_str(), "xml:space" | "r:id" | "xmlns:r")
                        }))
                {
                    return Err(unsupported("qualified XML outside the bounded profile"));
                }
                path.push(name);
                visitor.start(&path, &attrs)?;
                if matches!(event, Event::Empty(_)) {
                    visitor.end(&path)?;
                    path.pop();
                }
            }
            Event::End(_) => {
                visitor.end(&path)?;
                path.pop().ok_or_else(|| invalid("unmatched XML end"))?;
            }
            Event::Text(e) => {
                if scalar_value_path(root, &path) && e.contains(&b'\r') {
                    return Err(unsupported("normalized scalar cell text"));
                }
                let text = e.xml10_content().map_err(|e| invalid(e.to_string()))?;
                add(
                    &mut budget.text,
                    text.len(),
                    XlsxLimit::XmlTextBytes,
                    limits.max_xml_text_bytes,
                )?;
                if path.is_empty() && !text.trim().is_empty() {
                    return Err(invalid("text outside XML root"));
                }
                visitor.text(&path, &text)?;
            }
            Event::CData(e) => {
                if scalar_value_path(root, &path) {
                    return Err(unsupported("CDATA scalar cell value"));
                }
                let text = e.xml10_content().map_err(|e| invalid(e.to_string()))?;
                add(
                    &mut budget.text,
                    text.len(),
                    XlsxLimit::XmlTextBytes,
                    limits.max_xml_text_bytes,
                )?;
                if path.is_empty() {
                    return Err(invalid("CDATA outside XML root"));
                }
                visitor.text(&path, &text)?;
            }
            Event::GeneralRef(e) => {
                if scalar_value_path(root, &path) {
                    return Err(unsupported("entity in scalar cell value"));
                }
                let text =
                    if let Some(ch) = e.resolve_char_ref().map_err(|e| invalid(e.to_string()))? {
                        ch.to_string()
                    } else {
                        let name = e.decode().map_err(|e| invalid(e.to_string()))?;
                        quick_xml::escape::resolve_predefined_entity(&name)
                            .ok_or_else(|| unsupported("XML entity reference"))?
                            .to_string()
                    };
                add(
                    &mut budget.text,
                    text.len(),
                    XlsxLimit::XmlTextBytes,
                    limits.max_xml_text_bytes,
                )?;
                if path.is_empty() {
                    return Err(invalid("entity outside XML root"));
                }
                visitor.text(&path, &text)?;
            }
            Event::Decl(e) => {
                if roots != 0 || e.version().map_err(|e| invalid(e.to_string()))?.as_ref() != b"1.0"
                {
                    return Err(unsupported("XML declaration"));
                }
                if let Some(encoding) = e.encoding() {
                    let encoding = encoding.map_err(|e| invalid(e.to_string()))?;
                    if !encoding.eq_ignore_ascii_case(b"utf-8") {
                        return Err(unsupported("XML encoding other than UTF-8"));
                    }
                }
            }
            Event::Comment(_) => {
                if scalar_value_path(root, &path) {
                    return Err(unsupported("comment in scalar cell value"));
                }
            }
            Event::DocType(_) | Event::PI(_) => {
                return Err(unsupported("DTD or processing instruction"));
            }
            Event::Eof => unreachable!(),
        }
    }
    if roots != 1 || !path.is_empty() {
        return Err(invalid("incomplete XML document"));
    }
    Ok(())
}

fn scalar_value_path(root: Option<(&str, &str)>, path: &[String]) -> bool {
    root.is_some_and(|(name, _)| name == "worksheet") && path.last().is_some_and(|name| name == "v")
}

struct Shared<'a> {
    lengths: Vec<usize>,
    current: usize,
    limits: &'a XlsxLimitOptions,
}
impl Visitor for Shared<'_> {
    fn start(&mut self, path: &[String], attrs: &Attributes) -> Result<(), XlsxFormatError> {
        if path[..path.len() - 1].iter().any(|s| s == "t") {
            return Err(unsupported("nested string text element"));
        }
        match path.last().map(String::as_str) {
            Some("sst") if path.len() == 1 => {
                if let Some(count) = attrs.get("uniqueCount") {
                    check(
                        XlsxLimit::SharedStrings,
                        number(count)?,
                        self.limits.max_shared_strings,
                    )?;
                }
            }
            Some("si") => {
                if path.len() != 2 {
                    return Err(invalid("nested shared string"));
                }
                check(
                    XlsxLimit::SharedStrings,
                    self.lengths.len() + 1,
                    self.limits.max_shared_strings,
                )?;
                self.current = 0;
            }
            Some("t" | "r" | "rPr" | "rPh" | "phoneticPr") => {}
            // Formatting children don't affect values; all nodes/text still have budgets.
            _ if path.iter().any(|s| s == "rPr") => {}
            _ => return Err(unsupported("shared string shape")),
        }
        Ok(())
    }
    fn text(&mut self, path: &[String], text: &str) -> Result<(), XlsxFormatError> {
        if path.last().is_some_and(|s| s == "t") {
            add(
                &mut self.current,
                text.len(),
                XlsxLimit::StringBytes,
                self.limits.max_string_bytes,
            )?;
        }
        Ok(())
    }
    fn end(&mut self, path: &[String]) -> Result<(), XlsxFormatError> {
        if path.last().is_some_and(|s| s == "si") {
            self.lengths.push(self.current);
        }
        Ok(())
    }
}

#[derive(Default)]
struct Relations {
    targets: BTreeMap<String, (String, String)>,
    package: bool,
}
impl Visitor for Relations {
    fn start(&mut self, path: &[String], attrs: &Attributes) -> Result<(), XlsxFormatError> {
        if path.len() == 1 {
            return Ok(());
        }
        if path.len() != 2 || path[1] != "Relationship" {
            return Err(unsupported("relationship XML shape"));
        }
        if attrs.get("TargetMode").is_some_and(|s| s != "Internal") {
            return Err(unsupported("external relationship"));
        }
        let ty = attr(attrs, "Type")?;
        let target = attr(attrs, "Target")?;
        let allowed = if self.package {
            (ty == format!("{DOCUMENT_RELS}officeDocument")
                && matches!(target, "xl/workbook.xml" | "/xl/workbook.xml"))
                || (ty
                    == "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties"
                    && target == "docProps/core.xml")
                || (ty == format!("{DOCUMENT_RELS}extended-properties")
                    && target == "docProps/app.xml")
        } else {
            let part = target.strip_prefix("/xl/").unwrap_or(target);
            (ty == format!("{DOCUMENT_RELS}worksheet")
                && part.starts_with("worksheets/sheet")
                && part.ends_with(".xml"))
                || (ty == format!("{DOCUMENT_RELS}styles") && part == "styles.xml")
                || (ty == format!("{DOCUMENT_RELS}sharedStrings") && part == "sharedStrings.xml")
                || (ty == format!("{DOCUMENT_RELS}theme")
                    && part.starts_with("theme/theme")
                    && part.ends_with(".xml"))
        };
        if !allowed {
            return Err(unsupported("relationship outside flat workbook profile"));
        }
        if self
            .targets
            .insert(
                attr(attrs, "Id")?.to_string(),
                (target.to_string(), ty.to_string()),
            )
            .is_some()
        {
            return Err(invalid("duplicate relationship ID"));
        }
        Ok(())
    }
}

struct Workbook<'a> {
    relations: &'a Relations,
    sheets: Vec<(String, String)>,
    names: BTreeSet<String>,
    parts: BTreeSet<String>,
}
impl Visitor for Workbook<'_> {
    fn start(&mut self, path: &[String], attrs: &Attributes) -> Result<(), XlsxFormatError> {
        if path.last().is_some_and(|s| s == "sheet") {
            if path.len() != 3 || path[1] != "sheets" {
                return Err(invalid("sheet outside sheets"));
            }
            let name = attr(attrs, "name")?;
            if name.is_empty() || !self.names.insert(name.to_lowercase()) {
                return Err(invalid("duplicate or empty sheet name"));
            }
            let (target, ty) = self
                .relations
                .targets
                .get(attr(attrs, "r:id")?)
                .ok_or_else(|| invalid("missing sheet relationship"))?;
            if ty != &format!("{DOCUMENT_RELS}worksheet") {
                return Err(unsupported("non-worksheet sheet"));
            }
            let part = target
                .strip_prefix('/')
                .map(str::to_string)
                .unwrap_or_else(|| format!("xl/{target}"));
            if !self.parts.insert(part.clone()) {
                return Err(invalid("multiple sheet names target one part"));
            }
            self.sheets.push((name.to_string(), part));
        }
        Ok(())
    }
}

fn coordinate(text: &str, limits: &XlsxLimitOptions) -> Result<(usize, usize), XlsxFormatError> {
    let split = text
        .bytes()
        .position(|b| !b.is_ascii_uppercase())
        .ok_or_else(|| invalid("cell reference without row"))?;
    if split == 0 || split > 3 {
        return Err(invalid("invalid column reference"));
    }
    let col = text.as_bytes()[..split]
        .iter()
        .fold(0usize, |n, b| n * 26 + usize::from(*b - b'A' + 1));
    let row = number(&text[split..])?;
    if row == 0 {
        return Err(invalid("zero row"));
    }
    check(XlsxLimit::Rows, row, limits.max_rows)?;
    check(XlsxLimit::Columns, col, limits.max_columns)?;
    Ok((row, col))
}

struct Worksheet<'a> {
    limits: &'a XlsxLimitOptions,
    strings: &'a [usize],
    cells: usize,
    text: usize,
    min_row: usize,
    min_col: usize,
    max_row: usize,
    max_col: usize,
    row: usize,
    col: usize,
    kind: String,
    value: String,
    inline_bytes: usize,
    sheet_data: bool,
    cell_value: bool,
    value_text_seen: bool,
    integer_risks: Vec<(usize, usize, String)>,
}

impl Visitor for Worksheet<'_> {
    fn start(&mut self, path: &[String], attrs: &Attributes) -> Result<(), XlsxFormatError> {
        if path[..path.len() - 1].iter().any(|s| s == "v" || s == "t") {
            return Err(unsupported("nested cell text element"));
        }
        let name = path.last().map(String::as_str).unwrap_or("");
        match name {
            "dimension" => {
                if path.len() != 2 {
                    return Err(invalid("nested dimension"));
                }
                let refs = attr(attrs, "ref")?.split(':').collect::<Vec<_>>();
                if refs.len() > 2 {
                    return Err(invalid("invalid dimension"));
                }
                let (r1, c1) = coordinate(refs[0], self.limits)?;
                let (r2, c2) = coordinate(refs[refs.len() - 1], self.limits)?;
                if r2 < r1 || c2 < c1 {
                    return Err(invalid("inverted dimension"));
                }
                check(
                    XlsxLimit::DenseCells,
                    (r2 - r1 + 1).saturating_mul(c2 - c1 + 1),
                    self.limits.max_dense_cells,
                )?;
            }
            "sheetData" => {
                if path.len() != 2 || self.sheet_data {
                    return Err(invalid("duplicate or nested sheetData"));
                }
                self.sheet_data = true;
            }
            "row" => {
                if path.len() != 3 || path[1] != "sheetData" {
                    return Err(invalid("row outside sheetData"));
                }
                let row = number(attr(attrs, "r")?)?;
                if row <= self.row {
                    return Err(invalid("duplicate or unordered row"));
                }
                check(XlsxLimit::Rows, row, self.limits.max_rows)?;
                self.row = row;
                self.col = 0;
            }
            "c" => {
                if path.len() != 4 || path[2] != "row" {
                    return Err(invalid("cell outside row"));
                }
                let (row, col) = coordinate(attr(attrs, "r")?, self.limits)?;
                if row != self.row || col <= self.col {
                    return Err(invalid(
                        "duplicate, unordered or mismatched cell coordinate",
                    ));
                }
                self.col = col;
                add(&mut self.cells, 1, XlsxLimit::Cells, self.limits.max_cells)?;
                self.min_row = self.min_row.min(row);
                self.min_col = self.min_col.min(col);
                self.max_row = self.max_row.max(row);
                self.max_col = self.max_col.max(col);
                check(
                    XlsxLimit::DenseCells,
                    (self.max_row - self.min_row + 1)
                        .saturating_mul(self.max_col - self.min_col + 1),
                    self.limits.max_dense_cells,
                )?;
                self.kind = attrs.get("t").cloned().unwrap_or_else(|| "n".to_string());
                if !matches!(
                    self.kind.as_str(),
                    "n" | "s" | "inlineStr" | "str" | "b" | "d" | "e"
                ) {
                    return Err(unsupported("cell type"));
                }
                self.value.clear();
                self.inline_bytes = 0;
                self.cell_value = false;
                self.value_text_seen = false;
            }
            "v" | "is" => {
                if path.len() != 5 || path[3] != "c" || self.cell_value {
                    return Err(invalid("duplicate or misplaced cell value"));
                }
                self.cell_value = true;
            }
            "f" | "extLst" | "AlternateContent" => {
                return Err(unsupported("formula or worksheet extension"));
            }
            _ => {}
        }
        Ok(())
    }
    fn text(&mut self, path: &[String], text: &str) -> Result<(), XlsxFormatError> {
        if path.last().is_some_and(|s| s == "v") {
            if self.value_text_seen {
                return Err(unsupported("fragmented scalar cell value"));
            }
            self.value_text_seen = true;
            check(
                XlsxLimit::StringBytes,
                self.value.len().saturating_add(text.len()),
                self.limits.max_string_bytes,
            )?;
            self.value.push_str(text);
        } else if path.iter().any(|s| s == "c") && path.last().is_some_and(|s| s == "t") {
            add(
                &mut self.inline_bytes,
                text.len(),
                XlsxLimit::StringBytes,
                self.limits.max_string_bytes,
            )?;
        }
        Ok(())
    }
    fn end(&mut self, path: &[String]) -> Result<(), XlsxFormatError> {
        if path.last().is_some_and(|s| s == "c") {
            let bytes = if self.kind == "s" && !self.value.is_empty() {
                *self
                    .strings
                    .get(number(&self.value)?)
                    .ok_or_else(|| invalid("invalid shared string index"))?
            } else {
                self.value.len().saturating_add(self.inline_bytes)
            };
            if self.kind == "n" && !self.value.is_empty() {
                let n: f64 = self
                    .value
                    .parse()
                    .map_err(|_| invalid("invalid numeric cell"))?;
                if !n.is_finite() {
                    return Err(unsupported("non-finite numeric cell"));
                }
                // Compare the exact original decimal, not its rounded float.
                // Float columns still retain the full finite f64 domain.
                if !super::numeric::exact_integer(&self.value).is_some_and(|v| n == v as f64) {
                    self.integer_risks
                        .push((self.row, self.col, self.value.clone()));
                }
            }
            add(
                &mut self.text,
                bytes,
                XlsxLimit::MaterializedTextBytes,
                self.limits.max_materialized_text_bytes,
            )?;
        }
        Ok(())
    }
}

pub(super) fn validate(
    parts: &[(&str, Vec<u8>)],
    limits: &XlsxLimitOptions,
) -> Result<Summary, XlsxFormatError> {
    let mut budget = Budget::default();
    let find = |name: &str| {
        parts
            .iter()
            .find(|(path, _)| *path == name)
            .map(|(_, b)| b.as_slice())
            .ok_or_else(|| invalid(format!("missing part {name}")))
    };
    let mut types = ContentTypes::default();
    scan(
        find("[Content_Types].xml")?,
        Some((
            "Types",
            "http://schemas.openxmlformats.org/package/2006/content-types",
        )),
        &mut types,
        &mut budget,
        limits,
    )?;
    if !types.names.contains("/xl/workbook.xml") {
        return Err(invalid("missing workbook content type"));
    }
    for name in &types.names {
        find(&name[1..])?;
    }
    let mut package = Relations {
        package: true,
        ..Default::default()
    };
    scan(
        find("_rels/.rels")?,
        Some(("Relationships", RELS)),
        &mut package,
        &mut budget,
        limits,
    )?;
    if package
        .targets
        .values()
        .filter(|(_, ty)| ty == &format!("{DOCUMENT_RELS}officeDocument"))
        .count()
        != 1
    {
        return Err(invalid("expected one workbook relationship"));
    }
    let mut relations = Relations::default();
    scan(
        find("xl/_rels/workbook.xml.rels")?,
        Some(("Relationships", RELS)),
        &mut relations,
        &mut budget,
        limits,
    )?;
    for (target, _) in relations.targets.values() {
        let path = target
            .strip_prefix('/')
            .map(str::to_string)
            .unwrap_or_else(|| format!("xl/{target}"));
        find(&path)?;
    }
    let mut workbook = Workbook {
        relations: &relations,
        sheets: Vec::new(),
        names: BTreeSet::new(),
        parts: BTreeSet::new(),
    };
    scan(
        find("xl/workbook.xml")?,
        Some(("workbook", MAIN)),
        &mut workbook,
        &mut budget,
        limits,
    )?;
    let mut shared = Shared {
        lengths: Vec::new(),
        current: 0,
        limits,
    };
    if let Ok(bytes) = find("xl/sharedStrings.xml") {
        scan(bytes, Some(("sst", MAIN)), &mut shared, &mut budget, limits)?;
    }
    let mut extents = BTreeMap::new();
    for (name, bytes) in parts {
        if name.starts_with("xl/worksheets/") {
            let mut sheet = Worksheet {
                limits,
                strings: &shared.lengths,
                cells: 0,
                text: 0,
                min_row: usize::MAX,
                min_col: usize::MAX,
                max_row: 0,
                max_col: 0,
                row: 0,
                col: 0,
                kind: String::new(),
                value: String::new(),
                inline_bytes: 0,
                sheet_data: false,
                cell_value: false,
                value_text_seen: false,
                integer_risks: Vec::new(),
            };
            scan(
                bytes,
                Some(("worksheet", MAIN)),
                &mut sheet,
                &mut budget,
                limits,
            )?;
            if !sheet.sheet_data {
                return Err(invalid("missing sheetData"));
            }
            add(
                &mut budget.cells,
                sheet.cells,
                XlsxLimit::Cells,
                limits.max_cells,
            )?;
            add(
                &mut budget.cell_text,
                sheet.text,
                XlsxLimit::MaterializedTextBytes,
                limits.max_materialized_text_bytes,
            )?;
            extents.insert(*name, (sheet.max_row, sheet.integer_risks));
        } else if !matches!(
            *name,
            "_rels/.rels"
                | "xl/_rels/workbook.xml.rels"
                | "xl/workbook.xml"
                | "xl/sharedStrings.xml"
                | "[Content_Types].xml"
        ) {
            let root = (*name == "xl/styles.xml").then_some(("styleSheet", MAIN));
            scan(bytes, root, &mut Ignore, &mut budget, limits)?;
        }
    }
    let sheets = workbook
        .sheets
        .into_iter()
        .map(|(name, part)| {
            let (last_row, integer_risks) = extents
                .remove(part.as_str())
                .ok_or_else(|| invalid("worksheet relationship targets a non-worksheet part"))?;
            Ok(Sheet {
                name,
                last_row,
                integer_risks,
            })
        })
        .collect::<Result<Vec<_>, XlsxFormatError>>()?;
    Ok(Summary {
        sheets,
        cell_text_bytes: budget.cell_text,
    })
}
