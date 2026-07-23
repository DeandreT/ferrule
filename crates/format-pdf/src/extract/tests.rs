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
fn rejects_invalid_inherited_rotation_and_crop_geometry() -> Result<(), Box<dyn std::error::Error>>
{
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

fn make_pdf(page_count: usize, with_content: bool) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
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
