//! Self-authored PDF fixtures, layouts and their independently written
//! expected outcomes. Bytes are generated deterministically at run time.
use std::io::Write;

use anyhow::Result;
use flate2::Compression;
use flate2::write::ZlibEncoder;
use ir::{Instance, Value};
use mapping::{PdfCapture, PdfCommand, PdfGroup, PdfLayout, PdfPageSelection, PdfRegion};

const EMPTY_PAGE: &[u8] = include_bytes!("../../../pdf-worker/tests/fixtures/empty-page.pdf");
const HELLO_PAGE: &[u8] = include_bytes!("../../../pdf-worker/tests/fixtures/hello-page.pdf");
const MIB: usize = 1024 * 1024;
const WIDE_LINES: usize = 256;
const WIDE_WORDS: usize = 32;

#[derive(Clone, Copy)]
pub enum LayoutKind {
    /// One `Value` capture over the whole first page.
    FirstPage,
    /// A `Page` group per selected page, each with one `Value` capture.
    EveryPage,
}

impl LayoutKind {
    pub fn layout(self) -> Result<PdfLayout> {
        let capture = PdfCommand::Capture(PdfCapture {
            name: "Value".into(),
            region: PdfRegion::full(),
            algorithm: Default::default(),
        });
        Ok(match self {
            Self::FirstPage => PdfLayout::new("Document", PdfPageSelection::First, vec![capture])?,
            Self::EveryPage => PdfLayout::new(
                "Document",
                PdfPageSelection::All,
                vec![PdfCommand::GroupPerPage(PdfGroup {
                    name: "Page".into(),
                    region: PdfRegion::full(),
                    children: vec![capture],
                })],
            )?,
        })
    }
}

/// The outcome written down from the fixture's construction, before any run.
pub enum Expected {
    Instance(Instance),
    /// The `format_pdf::PdfError` variant name.
    PdfError(&'static str),
    /// No independent expectation; the outcome is recorded as observed.
    Observed,
}

pub struct Fixture {
    pub name: &'static str,
    pub description: &'static str,
    pub layout: LayoutKind,
    pub build: fn() -> Result<Vec<u8>>,
    pub expected: fn() -> Expected,
}

pub const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "empty-page",
        description: "existing one-page fixture without text",
        layout: LayoutKind::FirstPage,
        build: || Ok(EMPTY_PAGE.to_vec()),
        expected: || Expected::Instance(value(Value::Null)),
    },
    Fixture {
        name: "hello-page",
        description: "existing one-page fixture with `Hello`",
        layout: LayoutKind::FirstPage,
        build: || Ok(HELLO_PAGE.to_vec()),
        expected: || Expected::Instance(text("Hello")),
    },
    Fixture {
        name: "pages-256",
        description: "256 pages, one short line each, at the page limit",
        layout: LayoutKind::EveryPage,
        build: || Ok(numbered_pages(256)),
        expected: || Expected::Instance(page_groups(256)),
    },
    Fixture {
        name: "pages-257",
        description: "257 pages, one over the page limit",
        layout: LayoutKind::EveryPage,
        build: || Ok(numbered_pages(257)),
        expected: || Expected::PdfError("TooManyPages"),
    },
    Fixture {
        name: "wide-text",
        description: "one page of 256 lines x 32 words in one capture",
        layout: LayoutKind::FirstPage,
        build: || Ok(wide_text_page()),
        expected: || Expected::Instance(text(&wide_text())),
    },
    Fixture {
        name: "flate-32m",
        description: "Flate content inflating to 32 MiB of spaces before `Hello`",
        layout: LayoutKind::FirstPage,
        build: || padded_flate_page(32 * MIB),
        expected: || Expected::Instance(text("Hello")),
    },
    Fixture {
        name: "flate-320m",
        description: "Flate content inflating to 320 MiB of spaces before `Hello`",
        layout: LayoutKind::FirstPage,
        build: || padded_flate_page(320 * MIB),
        expected: || Expected::Observed,
    },
    Fixture {
        name: "late-invalid",
        description: "valid first page; second page's Flate content is corrupt",
        layout: LayoutKind::EveryPage,
        build: || Ok(late_invalid()),
        expected: || Expected::Observed,
    },
    Fixture {
        name: "truncated",
        description: "PDF header and an unterminated first object",
        layout: LayoutKind::FirstPage,
        build: || Ok(b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R".to_vec()),
        expected: || Expected::PdfError("InvalidPdf"),
    },
    Fixture {
        name: "input-over",
        description: "`hello-page` padded to 8 MiB + 1 byte",
        layout: LayoutKind::FirstPage,
        build: || {
            let mut bytes = HELLO_PAGE.to_vec();
            bytes.resize(format_pdf::MAX_INPUT_BYTES + 1, b'\n');
            Ok(bytes)
        },
        expected: || Expected::PdfError("InputTooLarge"),
    },
];

pub fn fixture(name: &str) -> Option<&'static Fixture> {
    FIXTURES.iter().find(|fixture| fixture.name == name)
}

fn value(value: Value) -> Instance {
    Instance::Group(vec![("Value".into(), Instance::Scalar(value))].into())
}

fn text(text: &str) -> Instance {
    value(Value::String(text.into()))
}

fn page_groups(pages: usize) -> Instance {
    let items = (1..=pages)
        .map(|page| text(&format!("Page {page:03}")))
        .collect();
    Instance::Group(vec![("Page".into(), Instance::Repeated(items))].into())
}

fn wide_line(line: usize) -> String {
    (0..WIDE_WORDS)
        .map(|word| format!("L{line:03}W{word:02}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn wide_text() -> String {
    (0..WIDE_LINES)
        .map(wide_line)
        .collect::<Vec<_>>()
        .join("\n")
}

/// One complete PDF: catalog, page tree, a Courier font, then one page and
/// content stream per entry. Each content entry carries its own stream
/// dictionary extras (such as a filter).
fn document(media: [u32; 2], contents: &[(Vec<u8>, &str)]) -> Vec<u8> {
    let kids = (0..contents.len())
        .map(|page| format!("{} 0 R", 4 + 2 * page))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!(
            "<< /Type /Pages /Kids [{kids}] /Count {} >>",
            contents.len()
        )
        .into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".to_vec(),
    ];
    for (page, (content, extra)) in contents.iter().enumerate() {
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] \
                 /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
                media[0],
                media[1],
                5 + 2 * page
            )
            .into_bytes(),
        );
        let mut stream = format!("<< /Length {}{extra} >>\nstream\n", content.len()).into_bytes();
        stream.extend_from_slice(content);
        stream.extend_from_slice(b"\nendstream");
        objects.push(stream);
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn show(size: u32, x: u32, y: u32, text: &str) -> Vec<u8> {
    format!("BT\n/F1 {size} Tf\n{x} {y} Td\n({text}) Tj\nET\n").into_bytes()
}

fn numbered_pages(pages: usize) -> Vec<u8> {
    let contents = (1..=pages)
        .map(|page| (show(12, 20, 150, &format!("Page {page:03}")), ""))
        .collect::<Vec<_>>();
    document([200, 200], &contents)
}

fn wide_text_page() -> Vec<u8> {
    // Courier 3 pt: 1.8 pt per glyph, 3.5 pt leading; every line fits on the page.
    let mut content = b"BT\n/F1 3 Tf\n3.5 TL\n10 990 Td\n".to_vec();
    for line in 0..WIDE_LINES {
        content.extend_from_slice(format!("({}) Tj\nT*\n", wide_line(line)).as_bytes());
    }
    content.extend_from_slice(b"ET\n");
    document([1000, 1000], &[(content, "")])
}

fn zlib(bytes: impl FnOnce(&mut ZlibEncoder<Vec<u8>>) -> std::io::Result<()>) -> Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    bytes(&mut encoder)?;
    Ok(encoder.finish()?)
}

fn padded_flate_page(padding: usize) -> Result<Vec<u8>> {
    let content = zlib(|encoder| {
        let spaces = vec![b' '; MIB];
        for _ in 0..padding / MIB {
            encoder.write_all(&spaces)?;
        }
        encoder.write_all(&show(12, 20, 150, "Hello"))
    })?;
    Ok(document([200, 200], &[(content, " /Filter /FlateDecode")]))
}

fn late_invalid() -> Vec<u8> {
    let corrupt = b"this is not a zlib stream".to_vec();
    document(
        [200, 200],
        &[
            (show(12, 20, 150, "Hello"), ""),
            (corrupt, " /Filter /FlateDecode"),
        ],
    )
}
