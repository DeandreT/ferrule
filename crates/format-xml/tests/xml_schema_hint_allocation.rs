use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use format_xml::XmlWriteOptions;
use ir::{Instance, ScalarType, SchemaNode, Value, XmlNamespace, XmlSchemaHints};

thread_local! {
    static MEASURING: Cell<bool> = const { Cell::new(false) };
    static REQUESTED_BYTES: Cell<usize> = const { Cell::new(0) };
    static ALLOCATION_CALLS: Cell<usize> = const { Cell::new(0) };
}

struct CountingSystem;

fn record(bytes: usize) {
    let _ = MEASURING.try_with(|active| {
        if active.get() {
            REQUESTED_BYTES.with(|total| total.set(total.get().saturating_add(bytes)));
            ALLOCATION_CALLS.with(|total| total.set(total.get().saturating_add(1)));
        }
    });
}

unsafe impl GlobalAlloc for CountingSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        record(layout.size());
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        record(layout.size());
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let pointer = unsafe { System.realloc(pointer, layout, new_size) };
        record(new_size);
        pointer
    }
}

#[global_allocator]
static ALLOCATOR: CountingSystem = CountingSystem;

fn measure<T>(operation: impl FnOnce() -> T) -> (T, usize, usize) {
    REQUESTED_BYTES.with(|value| value.set(0));
    ALLOCATION_CALLS.with(|value| value.set(0));
    MEASURING.with(|value| value.set(true));
    let output = operation();
    MEASURING.with(|value| value.set(false));
    let bytes = REQUESTED_BYTES.with(Cell::get);
    let calls = ALLOCATION_CALLS.with(Cell::get);
    (output, bytes, calls)
}

#[test]
fn shared_namespace_hint_inspection_has_header_proportional_requested_allocation() {
    const ATTRIBUTE_COUNT: usize = 512;
    const URI_BYTES: usize = 16 * 1024;
    let uri = format!("urn:shared:{}", "x".repeat(URI_BYTES - "urn:shared:".len()));
    assert_eq!(uri.len(), URI_BYTES);
    let children = (0..ATTRIBUTE_COUNT)
        .map(|index| {
            let mut child =
                SchemaNode::scalar(format!("a{index:04}"), ScalarType::String).attribute();
            child.xml_namespace = Some(XmlNamespace::qualified(uri.clone()).unwrap());
            child
        })
        .collect();
    let schema = SchemaNode::group("Root", children);
    let input = Instance::Group(
        (0..ATTRIBUTE_COUNT)
            .map(|index| {
                (
                    format!("a{index:04}"),
                    Instance::Scalar(Value::String("v".into())),
                )
            })
            .collect::<Vec<_>>()
            .into(),
    );
    let options = XmlWriteOptions {
        schema_hints: Some(XmlSchemaHints {
            no_namespace_location: Some("root.xsd".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut expected_default =
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Root xmlns:fns1=\"{uri}\"");
    for index in 0..ATTRIBUTE_COUNT {
        expected_default.push_str(&format!(" fns1:a{index:04}=\"v\""));
    }
    expected_default.push_str("/>");

    // Schema, input, options, and the expected legacy text exist before counting.
    // Each realloc records its requested new size, not live heap or peak RSS.
    let (default, default_bytes, default_calls) =
        measure(|| format_xml::to_string(&schema, &input));
    let (hinted, hinted_bytes, hinted_calls) =
        measure(|| format_xml::to_string_with_options(&schema, &input, &options));
    let default = default.unwrap();
    let hinted = hinted.unwrap();
    assert_eq!(default, expected_default);

    let owning_root = std::env::var_os("FERRULE_XML_ALLOCATION_PROBE_ROOT");
    let retain_receipts = owning_root.is_some();
    let root = owning_root
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            std::env::temp_dir().join(format!(
                "ferrule_xml_schema_hint_allocation_{}_{}",
                std::process::id(),
                nonce
            ))
        });
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("namespace.txt"), &uri).unwrap();
    std::fs::write(root.join("schema.debug"), format!("{schema:#?}")).unwrap();
    std::fs::write(root.join("input.debug"), format!("{input:#?}")).unwrap();
    std::fs::write(root.join("default.xml"), &default).unwrap();
    std::fs::write(root.join("hinted.xml"), &hinted).unwrap();

    let document = roxmltree::Document::parse(&hinted).unwrap();
    let element = document.root_element();
    assert_eq!(element.attributes().len(), ATTRIBUTE_COUNT + 1);
    for index in 0..ATTRIBUTE_COUNT {
        let name = format!("a{index:04}");
        assert_eq!(element.attribute((uri.as_str(), name.as_str())), Some("v"));
    }
    assert_eq!(
        element.attribute((
            "http://www.w3.org/2001/XMLSchema-instance",
            "noNamespaceSchemaLocation"
        )),
        Some("root.xsd")
    );

    let extra_bytes = hinted_bytes.saturating_sub(default_bytes);
    let budget = default.len() * 16 + ATTRIBUTE_COUNT * 128;
    let record = format!(
        "{{\"attribute_count\":{ATTRIBUTE_COUNT},\"uri_bytes\":{URI_BYTES},\"default_xml_bytes\":{},\"hinted_xml_bytes\":{},\"default_requested_bytes\":{default_bytes},\"hinted_requested_bytes\":{hinted_bytes},\"extra_requested_bytes\":{extra_bytes},\"default_allocation_calls\":{default_calls},\"hinted_allocation_calls\":{hinted_calls},\"budget\":{budget}}}",
        default.len(),
        hinted.len()
    );
    println!("{record}");
    std::fs::write(root.join("requested-allocation.json"), record).unwrap();
    assert!(
        extra_bytes <= budget,
        "hint inspection requested {extra_bytes} additional allocation bytes, exceeding the header-proportional budget {budget}"
    );
    if !retain_receipts {
        std::fs::remove_dir_all(root).unwrap();
    }
}
