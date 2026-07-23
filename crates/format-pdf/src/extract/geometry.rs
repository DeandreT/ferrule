use std::collections::{BTreeMap, BTreeSet};

use pdf_extract::{Document, Object};

use crate::PdfError;

use super::{Page, Rect};

const MAX_PAGE_INHERITANCE_DEPTH: usize = 64;

pub(super) fn clip_pages_to_visible_bounds(pages: &mut [Page]) {
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
pub(super) struct PageGeometry {
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
    pub(super) fn bounds(self) -> Rect {
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

    pub(super) fn point(self, x: f64, y: f64) -> Option<(f64, f64)> {
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

pub(super) fn page_geometries(
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
