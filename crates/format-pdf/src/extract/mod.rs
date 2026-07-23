mod geometry;

use std::collections::BTreeMap;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};

use pdf_extract::{
    ColorSpace, Dictionary, Document, MediaBox, Object, OutputDev, OutputError, Path, PathOp,
    Transform, content::Content, output_doc,
};

use crate::{MAX_EVENTS, MAX_INPUT_BYTES, MAX_PAGES, MAX_VALUE_BYTES, PdfError};
use geometry::{PageGeometry, clip_pages_to_visible_bounds, page_geometries};

const HORIZONTAL_EPSILON: f64 = 0.25;

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
mod tests;
