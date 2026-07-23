use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};

use pdf_extract::{
    ColorSpace, Dictionary, Document, MediaBox, Object, OutputDev, OutputError, Path, PathOp,
    Transform, content::Content, output_doc,
};

use crate::{MAX_EVENTS, MAX_INPUT_BYTES, MAX_PAGES, MAX_VALUE_BYTES, PdfError};

const HORIZONTAL_EPSILON: f64 = 0.25;
const MAX_PAGE_INHERITANCE_DEPTH: usize = 64;

/// One decoded PDF page in top-left coordinates.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Page {
    pub number: u32,
    pub bounds: Rect,
    pub glyphs: Vec<Glyph>,
    pub horizontal_edges: Vec<HorizontalEdge>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Glyph {
    pub text: String,
    pub bounds: Rect,
    pub font_face: Option<String>,
    pub cell_height: f64,
    pub baseline_angle: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HorizontalEdge {
    pub left: f64,
    pub right: f64,
    pub y: f64,
}

/// Decode a bounded PDF into layout-friendly visual primitives.
pub(crate) fn extract_pages(bytes: &[u8]) -> Result<Vec<Page>, PdfError> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(PdfError::InputTooLarge);
    }
    let document = catch_unwind(AssertUnwindSafe(|| Document::load_mem(bytes)))
        .map_err(|_| PdfError::InvalidPdf("PDF parser panicked".into()))?
        .map_err(|error| PdfError::InvalidPdf(error.to_string()))?;
    if document.is_encrypted() || document.was_encrypted() {
        return Err(PdfError::InvalidPdf(
            "encrypted PDF documents are not supported".into(),
        ));
    }
    let page_objects = catch_unwind(AssertUnwindSafe(|| document.get_pages()))
        .map_err(|_| PdfError::InvalidPdf("PDF page traversal panicked".into()))?;
    if page_objects.len() > MAX_PAGES {
        return Err(PdfError::TooManyPages);
    }
    let geometries = page_geometries(&document, &page_objects)?;

    let mut collector = Collector::new(geometries);
    let output = catch_unwind(AssertUnwindSafe(|| output_doc(&document, &mut collector)))
        .map_err(|_| PdfError::InvalidPdf("PDF content extraction panicked".into()))?;
    if let Some(abort) = collector.abort.take() {
        return Err(abort.into_error());
    }
    output.map_err(|error| PdfError::InvalidPdf(error.to_string()))?;
    if collector.current.is_some() {
        return Err(PdfError::InvalidPdf(
            "PDF content ended before its page was closed".into(),
        ));
    }
    annotate_font_faces(&document, &mut collector.pages);
    clip_pages_to_visible_bounds(&mut collector.pages);
    Ok(collector.pages)
}

fn clip_pages_to_visible_bounds(pages: &mut [Page]) {
    for page in pages {
        page.glyphs.retain_mut(|glyph| {
            let clipped = Rect {
                left: glyph.bounds.left.max(page.bounds.left),
                top: glyph.bounds.top.max(page.bounds.top),
                right: glyph.bounds.right.min(page.bounds.right),
                bottom: glyph.bounds.bottom.min(page.bounds.bottom),
            };
            if clipped.right <= clipped.left || clipped.bottom <= clipped.top {
                return false;
            }
            glyph.bounds = clipped;
            true
        });
    }
}

#[derive(Debug, Clone, Copy)]
struct UserBox {
    left: f64,
    bottom: f64,
    right: f64,
    top: f64,
}

impl UserBox {
    fn parse(value: &Object, name: &str) -> Result<Self, PdfError> {
        let values = value.as_array().map_err(|_| {
            PdfError::InvalidPdf(format!("PDF page /{name} must be an array of four numbers"))
        })?;
        let [left, bottom, right, top] = values.as_slice() else {
            return Err(PdfError::InvalidPdf(format!(
                "PDF page /{name} must contain exactly four numbers"
            )));
        };
        let values = [
            object_number(left, name)?,
            object_number(bottom, name)?,
            object_number(right, name)?,
            object_number(top, name)?,
        ];
        if !values.iter().all(|value| value.is_finite()) {
            return Err(PdfError::InvalidPdf(format!(
                "PDF page /{name} contains a non-finite coordinate"
            )));
        }
        let bounds = Self {
            left: values[0],
            bottom: values[1],
            right: values[2],
            top: values[3],
        };
        if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
            return Err(PdfError::InvalidPdf(format!(
                "PDF page /{name} has empty or reversed geometry"
            )));
        }
        Ok(bounds)
    }

    fn width(self) -> f64 {
        self.right - self.left
    }

    fn height(self) -> f64 {
        self.top - self.bottom
    }

    fn intersect(self, other: Self) -> Result<Self, PdfError> {
        let intersection = Self {
            left: self.left.max(other.left),
            bottom: self.bottom.max(other.bottom),
            right: self.right.min(other.right),
            top: self.top.min(other.top),
        };
        if intersection.width() <= 0.0 || intersection.height() <= 0.0 {
            return Err(PdfError::InvalidPdf(
                "PDF page /CropBox has no visible intersection with /MediaBox".into(),
            ));
        }
        Ok(intersection)
    }
}

#[derive(Debug, Clone, Copy)]
struct PageGeometry {
    crop: UserBox,
    rotation: PageRotation,
}

#[derive(Debug, Clone, Copy)]
enum PageRotation {
    None,
    Clockwise90,
    Clockwise180,
    Clockwise270,
}

impl PageGeometry {
    fn bounds(self) -> Rect {
        let (width, height) = match self.rotation {
            PageRotation::None | PageRotation::Clockwise180 => {
                (self.crop.width(), self.crop.height())
            }
            PageRotation::Clockwise90 | PageRotation::Clockwise270 => {
                (self.crop.height(), self.crop.width())
            }
        };
        Rect {
            left: 0.0,
            top: 0.0,
            right: width,
            bottom: height,
        }
    }

    fn point(self, x: f64, y: f64) -> Option<(f64, f64)> {
        let horizontal = x - self.crop.left;
        let vertical = y - self.crop.bottom;
        let point = match self.rotation {
            PageRotation::None => (horizontal, self.crop.height() - vertical),
            PageRotation::Clockwise90 => (vertical, horizontal),
            PageRotation::Clockwise180 => (self.crop.width() - horizontal, vertical),
            PageRotation::Clockwise270 => (
                self.crop.height() - vertical,
                self.crop.width() - horizontal,
            ),
        };
        (point.0.is_finite() && point.1.is_finite()).then_some(point)
    }
}

fn page_geometries(
    document: &Document,
    pages: &BTreeMap<u32, (u32, u16)>,
) -> Result<BTreeMap<u32, PageGeometry>, PdfError> {
    pages
        .iter()
        .map(|(number, page_id)| {
            let media = inherited_page_value(document, *page_id, b"MediaBox")?
                .ok_or_else(|| PdfError::InvalidPdf("PDF page has no /MediaBox".into()))?;
            let media = UserBox::parse(media, "MediaBox")?;
            let crop = inherited_page_value(document, *page_id, b"CropBox")?
                .map(|value| UserBox::parse(value, "CropBox"))
                .transpose()?
                .unwrap_or(media)
                .intersect(media)?;
            let rotation = inherited_page_value(document, *page_id, b"Rotate")?
                .map(parse_rotation)
                .transpose()?
                .unwrap_or(PageRotation::None);
            Ok((*number, PageGeometry { crop, rotation }))
        })
        .collect()
}

fn inherited_page_value<'a>(
    document: &'a Document,
    page_id: (u32, u16),
    key: &[u8],
) -> Result<Option<&'a Object>, PdfError> {
    let mut current = page_id;
    let mut visited = BTreeSet::new();
    for _ in 0..=MAX_PAGE_INHERITANCE_DEPTH {
        if !visited.insert(current) {
            return Err(PdfError::InvalidPdf(
                "PDF page inheritance contains a cycle".into(),
            ));
        }
        let dictionary = document
            .get_object(current)
            .and_then(Object::as_dict)
            .map_err(|error| PdfError::InvalidPdf(error.to_string()))?;
        if let Ok(value) = dictionary.get(key) {
            let (_, value) = document
                .dereference(value)
                .map_err(|error| PdfError::InvalidPdf(error.to_string()))?;
            return Ok(Some(value));
        }
        let Ok(parent) = dictionary.get(b"Parent") else {
            return Ok(None);
        };
        current = parent.as_reference().map_err(|_| {
            PdfError::InvalidPdf("PDF page /Parent must be an indirect reference".into())
        })?;
    }
    Err(PdfError::InvalidPdf(format!(
        "PDF page inheritance exceeds the {MAX_PAGE_INHERITANCE_DEPTH}-level limit"
    )))
}

fn object_number(value: &Object, name: &str) -> Result<f64, PdfError> {
    match value {
        Object::Integer(value) => Ok(*value as f64),
        Object::Real(value) => Ok(f64::from(*value)),
        _ => Err(PdfError::InvalidPdf(format!(
            "PDF page /{name} must contain only numbers"
        ))),
    }
}

fn parse_rotation(value: &Object) -> Result<PageRotation, PdfError> {
    let rotation = value
        .as_i64()
        .map_err(|_| PdfError::InvalidPdf("PDF page /Rotate must be an integer".into()))?;
    match rotation {
        0 => Ok(PageRotation::None),
        90 => Ok(PageRotation::Clockwise90),
        180 => Ok(PageRotation::Clockwise180),
        270 => Ok(PageRotation::Clockwise270),
        _ => Err(PdfError::InvalidPdf(
            "PDF page /Rotate must be one of 0, 90, 180, or 270".into(),
        )),
    }
}

#[derive(Debug)]
enum Abort {
    TooManyEvents,
    TooMuchText,
    InvalidCoordinate,
    InvalidStructure(&'static str),
}

impl Abort {
    fn into_error(self) -> PdfError {
        match self {
            Self::TooManyEvents => PdfError::TooManyEvents,
            Self::TooMuchText => PdfError::DecodedTextTooLarge,
            Self::InvalidCoordinate => {
                PdfError::InvalidPdf("PDF contains a non-finite visual coordinate".into())
            }
            Self::InvalidStructure(message) => PdfError::InvalidPdf(message.into()),
        }
    }
}

struct Collector {
    pages: Vec<Page>,
    current: Option<PageCollector>,
    geometries: BTreeMap<u32, PageGeometry>,
    events: usize,
    text_bytes: usize,
    abort: Option<Abort>,
}

struct PageCollector {
    page: Page,
    geometry: PageGeometry,
}

impl Default for Collector {
    fn default() -> Self {
        Self::new(BTreeMap::new())
    }
}

impl Collector {
    fn new(geometries: BTreeMap<u32, PageGeometry>) -> Self {
        Self {
            pages: Vec::new(),
            current: None,
            geometries,
            events: 0,
            text_bytes: 0,
            abort: None,
        }
    }

    fn event(&mut self) -> Result<(), OutputError> {
        self.events(1)
    }

    fn events(&mut self, count: usize) -> Result<(), OutputError> {
        self.events = self.events.checked_add(count).ok_or_else(|| {
            self.abort = Some(Abort::TooManyEvents);
            abort_output()
        })?;
        if self.events > MAX_EVENTS {
            self.abort = Some(Abort::TooManyEvents);
            return Err(abort_output());
        }
        Ok(())
    }

    fn reject(&mut self, reason: Abort) -> OutputError {
        self.abort = Some(reason);
        abort_output()
    }

    fn text(&mut self, count: usize) -> Result<(), OutputError> {
        self.text_bytes = self.text_bytes.checked_add(count).ok_or_else(|| {
            self.abort = Some(Abort::TooMuchText);
            abort_output()
        })?;
        if self.text_bytes > MAX_VALUE_BYTES {
            self.abort = Some(Abort::TooMuchText);
            return Err(abort_output());
        }
        Ok(())
    }
}

impl OutputDev for Collector {
    fn begin_page(
        &mut self,
        page_num: u32,
        _media_box: &MediaBox,
        _art_box: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), OutputError> {
        self.event()?;
        if self.current.is_some() {
            return Err(self.reject(Abort::InvalidStructure(
                "PDF started a page before closing the previous page",
            )));
        }
        let Some(geometry) = self.geometries.get(&page_num).copied() else {
            return Err(self.reject(Abort::InvalidStructure(
                "PDF extractor opened a page without validated geometry",
            )));
        };
        self.current = Some(PageCollector {
            page: Page {
                number: page_num,
                bounds: geometry.bounds(),
                glyphs: Vec::new(),
                horizontal_edges: Vec::new(),
            },
            geometry,
        });
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), OutputError> {
        self.event()?;
        let Some(page) = self.current.take() else {
            return Err(self.reject(Abort::InvalidStructure(
                "PDF closed a page that was not open",
            )));
        };
        self.pages.push(page.page);
        Ok(())
    }

    fn output_character(
        &mut self,
        transform: &Transform,
        width: f64,
        spacing: f64,
        font_size: f64,
        text: &str,
    ) -> Result<(), OutputError> {
        self.event()?;
        self.text(text.len())?;
        if self.current.is_none() {
            return Err(self.reject(Abort::InvalidStructure("PDF emitted text outside a page")));
        }
        let advance = width * font_size + spacing;
        let origin_x = transform.m31;
        let origin_y = transform.m32;
        let end_x = transform.m11.mul_add(advance, origin_x);
        let end_y = transform.m12.mul_add(advance, origin_y);
        let cap_x = transform.m21.mul_add(font_size, origin_x);
        let cap_y = transform.m22.mul_add(font_size, origin_y);
        let values = [origin_x, origin_y, end_x, end_y, cap_x, cap_y];
        if !values.iter().all(|value| value.is_finite()) {
            return Err(self.reject(Abort::InvalidCoordinate));
        }
        if text.is_empty() {
            return Ok(());
        }
        let geometry = self
            .current
            .as_ref()
            .map(|current| current.geometry)
            .ok_or_else(abort_output)?;
        let raw_points = [
            (origin_x, origin_y),
            (end_x, end_y),
            (cap_x, cap_y),
            (end_x + cap_x - origin_x, end_y + cap_y - origin_y),
        ];
        let [Some(origin), Some(end), Some(cap), Some(far)] =
            raw_points.map(|(x, y)| geometry.point(x, y))
        else {
            return Err(self.reject(Abort::InvalidCoordinate));
        };
        let points = [origin, end, cap, far];
        let current = self.current.as_mut().ok_or_else(abort_output)?;
        current.page.glyphs.push(Glyph {
            text: text.to_owned(),
            bounds: Rect {
                left: points
                    .iter()
                    .map(|point| point.0)
                    .fold(f64::INFINITY, f64::min),
                top: points
                    .iter()
                    .map(|point| point.1)
                    .fold(f64::INFINITY, f64::min),
                right: points
                    .iter()
                    .map(|point| point.0)
                    .fold(f64::NEG_INFINITY, f64::max),
                bottom: points
                    .iter()
                    .map(|point| point.1)
                    .fold(f64::NEG_INFINITY, f64::max),
            },
            font_face: None,
            cell_height: (cap_x - origin_x).hypot(cap_y - origin_y),
            baseline_angle: (end.1 - origin.1).atan2(end.0 - origin.0).to_degrees(),
        });
        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), OutputError> {
        self.event()
    }

    fn end_word(&mut self) -> Result<(), OutputError> {
        self.event()
    }

    fn end_line(&mut self) -> Result<(), OutputError> {
        self.event()
    }

    fn stroke(
        &mut self,
        transform: &Transform,
        _colorspace: &ColorSpace,
        _color: &[f64],
        path: &Path,
    ) -> Result<(), OutputError> {
        self.event()?;
        self.collect_edges(transform, path, false)
    }

    fn fill(
        &mut self,
        transform: &Transform,
        _colorspace: &ColorSpace,
        _color: &[f64],
        path: &Path,
    ) -> Result<(), OutputError> {
        self.event()?;
        self.collect_edges(transform, path, true)
    }
}

struct FontRun {
    text: String,
    face: String,
}

fn annotate_font_faces(document: &Document, pages: &mut [Page]) {
    for (number, page_id) in document.get_pages() {
        let Some(page) = pages.iter_mut().find(|page| page.number == number) else {
            continue;
        };
        let Some(runs) = page_font_runs(document, page_id) else {
            continue;
        };
        apply_font_runs(&mut page.glyphs, &runs);
    }
}

fn page_font_runs(document: &Document, page_id: (u32, u16)) -> Option<Vec<FontRun>> {
    let fonts = document.get_page_fonts(page_id).ok()?;
    let content = document.get_page_content(page_id).ok()?;
    let content = Content::decode(&content).ok()?;
    if content.operations.len() > MAX_EVENTS {
        return None;
    }
    let mut current_font = None;
    let mut runs = Vec::new();
    for operation in content.operations {
        if operation.operator == "Tf" {
            current_font = operation
                .operands
                .first()
                .and_then(|operand| operand.as_name().ok())
                .map(<[u8]>::to_vec);
            continue;
        }
        let Some(font) = current_font.as_ref().and_then(|name| fonts.get(name)) else {
            continue;
        };
        let face = font
            .get(b"BaseFont")
            .ok()
            .and_then(|value| value.as_name().ok())
            .map(|value| String::from_utf8_lossy(value).into_owned());
        let Some(face) = face else {
            continue;
        };
        match operation.operator.as_str() {
            "Tj" | "'" | "\"" => {
                if let Some(value) = operation.operands.last()
                    && let Some(text) = decode_font_text(document, font, value)
                {
                    runs.push(FontRun {
                        text,
                        face: face.clone(),
                    });
                }
            }
            "TJ" => {
                let Some(values) = operation
                    .operands
                    .first()
                    .and_then(|value| value.as_array().ok())
                else {
                    continue;
                };
                for value in values {
                    if let Some(text) = decode_font_text(document, font, value) {
                        runs.push(FontRun {
                            text,
                            face: face.clone(),
                        });
                    }
                }
            }
            _ => {}
        }
        if runs.len() > MAX_EVENTS {
            return None;
        }
    }
    Some(runs)
}

fn decode_font_text(document: &Document, font: &Dictionary, value: &Object) -> Option<String> {
    let bytes = value.as_str().ok()?;
    let encoding = font.get_font_encoding(document).ok()?;
    Document::decode_text(&encoding, bytes).ok()
}

fn apply_font_runs(glyphs: &mut [Glyph], runs: &[FontRun]) {
    let mut stream = String::new();
    let mut ranges = Vec::with_capacity(glyphs.len());
    for glyph in glyphs.iter() {
        let start = stream.len();
        stream.push_str(&glyph.text);
        ranges.push(start..stream.len());
    }
    let mut cursor = 0;
    let mut glyph_cursor = 0;
    for run in runs {
        if run.text.is_empty() || cursor > stream.len() {
            continue;
        }
        let Some(relative) = stream[cursor..].find(&run.text) else {
            continue;
        };
        let start = cursor + relative;
        let end = start + run.text.len();
        while ranges
            .get(glyph_cursor)
            .is_some_and(|range| range.end <= start)
        {
            glyph_cursor += 1;
        }
        for (glyph, range) in glyphs[glyph_cursor..]
            .iter_mut()
            .zip(&ranges[glyph_cursor..])
            .take_while(|(_, range)| range.start < end)
        {
            if range.start < end && range.end > start {
                glyph.font_face = Some(run.face.clone());
            }
        }
        cursor = end;
    }
}

impl Collector {
    fn collect_edges(
        &mut self,
        transform: &Transform,
        path: &Path,
        _filled: bool,
    ) -> Result<(), OutputError> {
        self.events(path.ops.len())?;
        if self.current.is_none() {
            return Err(self.reject(Abort::InvalidStructure(
                "PDF emitted a painted path outside a page",
            )));
        }
        if !transform_is_finite(transform) || !path.ops.iter().all(path_op_is_finite) {
            return Err(self.reject(Abort::InvalidCoordinate));
        }
        let mut cursor = None;
        let mut subpath_start = None;
        for operation in &path.ops {
            match *operation {
                PathOp::MoveTo(x, y) => {
                    let point = transform_point(transform, x, y)
                        .ok_or_else(|| self.reject(Abort::InvalidCoordinate))?;
                    cursor = Some(point);
                    subpath_start = Some(point);
                }
                PathOp::LineTo(x, y) => {
                    let point = transform_point(transform, x, y)
                        .ok_or_else(|| self.reject(Abort::InvalidCoordinate))?;
                    if let Some(start) = cursor {
                        let valid = {
                            let current = self.current.as_mut().ok_or_else(abort_output)?;
                            push_horizontal(current, start, point)
                        };
                        if !valid {
                            return Err(self.reject(Abort::InvalidCoordinate));
                        }
                    }
                    cursor = Some(point);
                }
                PathOp::Rect(x, y, width, height) => {
                    let corners = [
                        transform_point(transform, x, y)
                            .ok_or_else(|| self.reject(Abort::InvalidCoordinate))?,
                        transform_point(transform, x + width, y)
                            .ok_or_else(|| self.reject(Abort::InvalidCoordinate))?,
                        transform_point(transform, x + width, y + height)
                            .ok_or_else(|| self.reject(Abort::InvalidCoordinate))?,
                        transform_point(transform, x, y + height)
                            .ok_or_else(|| self.reject(Abort::InvalidCoordinate))?,
                    ];
                    let valid = {
                        let current = self.current.as_mut().ok_or_else(abort_output)?;
                        push_horizontal(current, corners[0], corners[1])
                            && push_horizontal(current, corners[1], corners[2])
                            && push_horizontal(current, corners[3], corners[2])
                            && push_horizontal(current, corners[0], corners[3])
                    };
                    if !valid {
                        return Err(self.reject(Abort::InvalidCoordinate));
                    }
                    cursor = Some(corners[0]);
                    subpath_start = Some(corners[0]);
                }
                PathOp::Close => {
                    if let (Some(start), Some(end)) = (cursor, subpath_start) {
                        let valid = {
                            let current = self.current.as_mut().ok_or_else(abort_output)?;
                            push_horizontal(current, start, end)
                        };
                        if !valid {
                            return Err(self.reject(Abort::InvalidCoordinate));
                        }
                    }
                    cursor = subpath_start;
                }
                PathOp::CurveTo(_, _, _, _, x, y) => {
                    cursor = Some(
                        transform_point(transform, x, y)
                            .ok_or_else(|| self.reject(Abort::InvalidCoordinate))?,
                    );
                }
            }
        }
        Ok(())
    }
}

fn transform_point(transform: &Transform, x: f64, y: f64) -> Option<(f64, f64)> {
    let transformed_x = transform
        .m11
        .mul_add(x, transform.m21.mul_add(y, transform.m31));
    let transformed_y = transform
        .m12
        .mul_add(x, transform.m22.mul_add(y, transform.m32));
    if transformed_x.is_finite() && transformed_y.is_finite() {
        Some((transformed_x, transformed_y))
    } else {
        None
    }
}

fn transform_is_finite(transform: &Transform) -> bool {
    [
        transform.m11,
        transform.m12,
        transform.m21,
        transform.m22,
        transform.m31,
        transform.m32,
    ]
    .iter()
    .all(|value| value.is_finite())
}

fn path_op_is_finite(operation: &PathOp) -> bool {
    match *operation {
        PathOp::MoveTo(x, y) | PathOp::LineTo(x, y) => [x, y].iter().all(|value| value.is_finite()),
        PathOp::CurveTo(x1, y1, x2, y2, x, y) => {
            [x1, y1, x2, y2, x, y].iter().all(|value| value.is_finite())
        }
        PathOp::Rect(x, y, width, height) => {
            [x, y, width, height].iter().all(|value| value.is_finite())
        }
        PathOp::Close => true,
    }
}

fn push_horizontal(current: &mut PageCollector, start: (f64, f64), end: (f64, f64)) -> bool {
    let Some(start) = current.geometry.point(start.0, start.1) else {
        return false;
    };
    let Some(end) = current.geometry.point(end.0, end.1) else {
        return false;
    };
    let y_delta = start.1 - end.1;
    if !y_delta.is_finite() {
        return false;
    }
    if y_delta.abs() > HORIZONTAL_EPSILON {
        return true;
    }
    let left = start.0.min(end.0).max(current.page.bounds.left);
    let right = start.0.max(end.0).min(current.page.bounds.right);
    let y = start.1 / 2.0 + end.1 / 2.0;
    if !left.is_finite() || !right.is_finite() || !y.is_finite() {
        return false;
    }
    if y < current.page.bounds.top - HORIZONTAL_EPSILON
        || y > current.page.bounds.bottom + HORIZONTAL_EPSILON
        || right - left <= HORIZONTAL_EPSILON
    {
        return true;
    }
    current
        .page
        .horizontal_edges
        .push(HorizontalEdge { left, right, y });
    true
}

fn abort_output() -> OutputError {
    OutputError::IoError(io::Error::other("bounded PDF extraction aborted"))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use pdf_extract::content::{Content, Operation};
    use pdf_extract::{
        Document, EncryptionState, EncryptionVersion, Object, Permissions, Stream, dictionary,
    };

    use super::*;

    #[test]
    fn extracts_positioned_text_and_horizontal_edges() -> Result<(), Box<dyn std::error::Error>> {
        let bytes = make_pdf(1, true)?;

        let pages = extract_pages(&bytes)?;

        assert_eq!(pages.len(), 1);
        let page = &pages[0];
        assert_eq!(page.number, 1);
        assert_eq!(
            page.bounds,
            Rect {
                left: 0.0,
                top: 0.0,
                right: 200.0,
                bottom: 200.0,
            }
        );
        assert_eq!(
            page.glyphs
                .iter()
                .map(|glyph| glyph.text.as_str())
                .collect::<String>(),
            "Table"
        );
        assert!(page.glyphs.iter().all(|glyph| {
            glyph.bounds.left.is_finite()
                && glyph.bounds.top.is_finite()
                && glyph.bounds.right.is_finite()
                && glyph.bounds.bottom.is_finite()
                && glyph.font_face.as_deref() == Some("Courier")
                && (glyph.cell_height - 20.0).abs() < 0.01
                && glyph.baseline_angle.abs() < 0.01
        }));
        assert!(
            page.horizontal_edges
                .iter()
                .any(|edge| (edge.left - 10.0).abs() < 0.01
                    && (edge.right - 170.0).abs() < 0.01
                    && (edge.y - 140.0).abs() < 0.01)
        );
        Ok(())
    }

    #[test]
    fn inherited_crop_and_all_page_rotations_use_visible_coordinates()
    -> Result<(), Box<dyn std::error::Error>> {
        let cases = [
            (
                0,
                Rect {
                    left: 0.0,
                    top: 0.0,
                    right: 170.0,
                    bottom: 150.0,
                },
                Rect {
                    left: 30.0,
                    top: 10.0,
                    right: 42.0,
                    bottom: 30.0,
                },
                0.0,
            ),
            (
                90,
                Rect {
                    left: 0.0,
                    top: 0.0,
                    right: 150.0,
                    bottom: 170.0,
                },
                Rect {
                    left: 120.0,
                    top: 30.0,
                    right: 140.0,
                    bottom: 42.0,
                },
                90.0,
            ),
            (
                180,
                Rect {
                    left: 0.0,
                    top: 0.0,
                    right: 170.0,
                    bottom: 150.0,
                },
                Rect {
                    left: 128.0,
                    top: 120.0,
                    right: 140.0,
                    bottom: 140.0,
                },
                180.0,
            ),
            (
                270,
                Rect {
                    left: 0.0,
                    top: 0.0,
                    right: 150.0,
                    bottom: 170.0,
                },
                Rect {
                    left: 10.0,
                    top: 128.0,
                    right: 30.0,
                    bottom: 140.0,
                },
                -90.0,
            ),
        ];

        for (rotation, expected_page, expected_glyph, expected_angle) in cases {
            let bytes = make_geometry_pdf(Object::Integer(rotation), crop_box())?;
            let pages = extract_pages(&bytes)?;
            let [page] = pages.as_slice() else {
                panic!("geometry fixture should contain exactly one page");
            };
            assert_rect_close(page.bounds, expected_page);
            assert_eq!(
                page.glyphs
                    .iter()
                    .map(|glyph| glyph.text.as_str())
                    .collect::<String>(),
                "TableP",
                "rotation {rotation} must exclude text outside /CropBox"
            );
            let Some(first) = page.glyphs.first() else {
                panic!("rotation {rotation} should retain visible text");
            };
            assert_rect_close(first.bounds, expected_glyph);
            assert!((first.baseline_angle - expected_angle).abs() < 0.01);
            assert_eq!(first.font_face.as_deref(), Some("Courier"));
            assert!(page.glyphs.iter().all(|glyph| {
                glyph.bounds.left >= page.bounds.left
                    && glyph.bounds.top >= page.bounds.top
                    && glyph.bounds.right <= page.bounds.right
                    && glyph.bounds.bottom <= page.bounds.bottom
            }));
            if rotation == 0 {
                let Some(partial) = page.glyphs.iter().find(|glyph| glyph.text == "P") else {
                    panic!("partially visible glyph should be retained");
                };
                assert!((partial.bounds.left - 165.0).abs() < 0.01);
                assert!((partial.bounds.right - page.bounds.right).abs() < 0.01);
            }
            if rotation == 90 {
                assert!(page.horizontal_edges.iter().any(|edge| {
                    (edge.left - 10.0).abs() < 0.01
                        && (edge.right - 140.0).abs() < 0.01
                        && (edge.y - 40.0).abs() < 0.01
                }));
            }
        }
        Ok(())
    }

    #[test]
    fn rejects_invalid_inherited_rotation_and_crop_geometry()
    -> Result<(), Box<dyn std::error::Error>> {
        let invalid_rotation = make_geometry_pdf(Object::Integer(45), crop_box())?;
        let error = extract_pages(&invalid_rotation)
            .expect_err("non-right-angle page rotation should be rejected");
        assert!(error.to_string().contains("/Rotate must be one of"));

        let reversed_crop = make_geometry_pdf(
            Object::Integer(0),
            vec![180.into(), 30.into(), 20.into(), 180.into()],
        )?;
        let error =
            extract_pages(&reversed_crop).expect_err("reversed crop geometry should be rejected");
        assert!(error.to_string().contains("/CropBox has empty or reversed"));

        let outside_crop = make_geometry_pdf(
            Object::Integer(0),
            vec![300.into(), 300.into(), 400.into(), 400.into()],
        )?;
        let error = extract_pages(&outside_crop)
            .expect_err("crop geometry outside MediaBox should be rejected");
        assert!(error.to_string().contains("no visible intersection"));
        Ok(())
    }

    #[test]
    fn rejects_documents_over_the_page_limit() -> Result<(), Box<dyn std::error::Error>> {
        let bytes = make_pdf(MAX_PAGES + 1, false)?;

        let error = extract_pages(&bytes).expect_err("page limit should reject the document");

        assert!(matches!(error, PdfError::TooManyPages));
        Ok(())
    }

    #[test]
    fn rejects_input_over_the_byte_limit() {
        let bytes = vec![0; MAX_INPUT_BYTES + 1];

        let error = extract_pages(&bytes).expect_err("input limit should reject the document");

        assert!(matches!(error, PdfError::InputTooLarge));
    }

    #[test]
    fn rejects_decoded_text_over_the_byte_limit() {
        let mut collector = Collector::default();
        assert!(collector.text(MAX_VALUE_BYTES + 1).is_err());
        assert!(matches!(collector.abort, Some(Abort::TooMuchText)));
    }

    #[test]
    fn rejects_encrypted_documents_explicitly() -> Result<(), Box<dyn std::error::Error>> {
        let mut document = make_document(1, false)?;
        document.trailer.set(
            "ID",
            vec![
                Object::string_literal("0123456789abcdef"),
                Object::string_literal("fedcba9876543210"),
            ],
        );
        let state = EncryptionState::try_from(EncryptionVersion::V1 {
            document: &document,
            owner_password: "owner-secret",
            user_password: "user-secret",
            permissions: Permissions::PRINTABLE,
        })?;
        document.encrypt(&state)?;
        let mut bytes = Vec::new();
        document.save_to(&mut bytes)?;

        let error = extract_pages(&bytes).expect_err("encryption should reject the document");

        assert!(matches!(error, PdfError::InvalidPdf(_)));
        assert!(error.to_string().contains("encrypted PDF"));
        Ok(())
    }

    #[test]
    #[ignore = "needs the local ReferenceSamples corpus; informational only"]
    fn survey_annual_temperature_primitives() -> Result<(), Box<dyn std::error::Error>> {
        let path = std::env::var_os("FERRULE_PDF_SURVEY")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../samples/ReferenceSamples/Annual Average Temperature By Year.pdf")
            });
        if !path.is_file() {
            return Ok(());
        }
        let pages = extract_pages(&std::fs::read(path)?)?;
        for page in pages {
            eprintln!(
                "page {}: bounds={:?}, glyphs={}, horizontal_edges={}",
                page.number,
                page.bounds,
                page.glyphs.len(),
                page.horizontal_edges.len()
            );
            for glyph in page.glyphs.iter().take(20) {
                eprintln!("  {:?} {:?}", glyph.text, glyph.bounds);
            }
            for edge in &page.horizontal_edges {
                eprintln!("  edge {:?}", edge);
            }
        }
        Ok(())
    }

    fn make_pdf(
        page_count: usize,
        with_content: bool,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut document = make_document(page_count, with_content)?;
        let mut bytes = Vec::new();
        document.save_to(&mut bytes)?;
        Ok(bytes)
    }

    fn crop_box() -> Vec<Object> {
        vec![20.into(), 30.into(), 190.into(), 180.into()]
    }

    fn make_geometry_pdf(
        rotation: Object,
        crop: Vec<Object>,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut document = make_document(1, false)?;
        let pages = document.get_pages();
        let Some(page_id) = pages.get(&1).copied() else {
            return Err("geometry fixture has no first page".into());
        };
        let parent_id = document
            .get_object(page_id)?
            .as_dict()?
            .get(b"Parent")?
            .as_reference()?;
        let operations = vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 20.into()]),
            Operation::new("Td", vec![50.into(), 150.into()]),
            Operation::new("Tj", vec![Object::string_literal("Table")]),
            Operation::new("ET", vec![]),
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 20.into()]),
            Operation::new("Td", vec![185.into(), 100.into()]),
            Operation::new("Tj", vec![Object::string_literal("P")]),
            Operation::new("ET", vec![]),
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 20.into()]),
            Operation::new("Td", vec![195.into(), 100.into()]),
            Operation::new("Tj", vec![Object::string_literal("Hidden")]),
            Operation::new("ET", vec![]),
            Operation::new("m", vec![20.into(), 80.into()]),
            Operation::new("l", vec![180.into(), 80.into()]),
            Operation::new("S", vec![]),
            Operation::new("m", vec![60.into(), 40.into()]),
            Operation::new("l", vec![60.into(), 170.into()]),
            Operation::new("S", vec![]),
        ];
        let content = Content { operations }.encode()?;
        let content_id = document.add_object(Stream::new(dictionary! {}, content));
        document
            .get_object_mut(page_id)?
            .as_dict_mut()?
            .set("Contents", content_id);
        let parent = document.get_object_mut(parent_id)?.as_dict_mut()?;
        parent.set("CropBox", crop);
        parent.set("Rotate", rotation);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes)?;
        Ok(bytes)
    }

    fn assert_rect_close(actual: Rect, expected: Rect) {
        assert!(
            (actual.left - expected.left).abs() < 0.01
                && (actual.top - expected.top).abs() < 0.01
                && (actual.right - expected.right).abs() < 0.01
                && (actual.bottom - expected.bottom).abs() < 0.01,
            "actual {actual:?}, expected {expected:?}"
        );
    }

    fn make_document(
        page_count: usize,
        with_content: bool,
    ) -> Result<Document, Box<dyn std::error::Error>> {
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Courier",
        });
        let resources_id = document.add_object(dictionary! {
            "Font" => dictionary! {
                "F1" => font_id,
            },
        });
        let operations = if with_content {
            vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 20.into()]),
                Operation::new("Td", vec![50.into(), 150.into()]),
                Operation::new("Tj", vec![Object::string_literal("Table")]),
                Operation::new("ET", vec![]),
                Operation::new("m", vec![20.into(), 80.into()]),
                Operation::new("l", vec![180.into(), 80.into()]),
                Operation::new("S", vec![]),
            ]
        } else {
            Vec::new()
        };
        let content = Content { operations }.encode()?;
        let content_id = document.add_object(Stream::new(dictionary! {}, content));
        let mut page_ids = Vec::with_capacity(page_count);
        for _ in 0..page_count {
            page_ids.push(document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
            }));
        }
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => page_ids.into_iter().map(Object::from).collect::<Vec<_>>(),
                "Count" => page_count as i64,
                "Resources" => resources_id,
                "MediaBox" => vec![10.into(), 20.into(), 210.into(), 220.into()],
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        Ok(document)
    }
}
