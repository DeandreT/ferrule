# Endpoint Codec Preparation

Generated JSON5, XML and X12 boundaries used to decode and validate their
embedded descriptors on every call. Both runtimes now prepare each descriptor
once per process and share the immutable result across repeated and concurrent
calls, as [JSON endpoint preparation](json-endpoint-preparation.md) does for
ordinary JSON. Every call still keeps its own work accounting. For a small
document this removes most of the per-call cost.

With preparation in place, the measurements below attribute the remaining
per-call costs. Three text copies were attributed and removed: the growing JSON
output buffer in Rust, and the UTF-16 output string on the C# X12 and JSON5 byte
routes. The other remaining costs are listed with their measurements.

## Ownership

| Codec | Prepared once, immutable, shared | Per call |
| --- | --- | --- |
| Rust JSON5 | Decoded source and target profile schemas with their format-level reader and writer preparation | Original byte check; UTF-8 check; normalization and its work budget; parsed and normalized trees; output byte check |
| Rust XML | Decoded schema, shared by the structured and observed readers, the document writer and the expression-node writer | Document byte check; UTF-8 check; each route's schema checks; parser and projection budgets; schema hints; writer; namespace and character checks; output byte check |
| Rust X12 | Profile: decoded schema, version, envelope, separators, constraints, implied decimals, lexical formats and completion | Document byte check; UTF-8 check; segment, node, loop and name budgets; wire validation; output reservation and byte check |
| C# JSON5 | Admitted and parsed profile schema graph | Original byte check; normalization; `NodeBudget`; parsed document; output byte check |
| C# XML | Flat-root input schema; structured input schema; document output schema with its namespace metadata check; expression-node schema | Input byte check; UTF-8 decoding; parser and projection budgets; schema hints; writer; character and output byte checks |
| C# X12 | Input profile and output profile | Document byte check; UTF-8 decoding; segments; `Budget`; output text and byte count |

Rust's descriptor-taking functions, which generated mappings call with their
descriptor constants, look up preparations in bounded process-wide registries
keyed by descriptor text. Each registry retains at most 64 descriptors. The
JSON5 and X12 registries hold 4 MiB of descriptor text and the XML registry
8 MiB, so one maximal XML descriptor fits. Admitting one more starts a registry
over, and a descriptor larger than its registry's budget is prepared for its
call without being retained. A descriptor used for both input and output shares
one XML or X12 preparation. JSON5 source and target descriptors share one
registry.

C# keys each role's preparation by descriptor string identity in a
`ConditionalWeakTable`, as the ordinary JSON codec does. Generated descriptor
constants are interned, so each one is prepared once per role. A descriptor
built at run time is prepared once per string object and released with it.

Neither runtime retains a failed preparation. A rejected descriptor is prepared
again by every call and reports its original error in its original order. Each
route still applies its own checks in their original positions. Document byte
limits and UTF-8 checks come first, and the observed XML root policy and
default-namespace arguments are checked per call. In C#, the XML document writer
used to parse its schema twice per call; it now uses the schema its own schema
stage prepared.

Preparation is a runtime concern keyed by the descriptor constants that the
emitters already render. Generated Rust and C# mapping sources are unchanged,
and the codegen `BoundaryEmissionPlan` gains no runtime state. The embedded C#
runtime gains `FerruleXml.Prepared.cs`.

## Measurement

Rust: `crates/codegen-runtime/tests/endpoint_codec_preparation.rs` uses the
counting allocator in `tests/support/allocation.rs`, shared with the JSON
measurement. Each row reports mean allocations, allocated bytes and peak live
bytes on the measuring thread, plus mean wall-clock time:

```sh
cargo test -p codegen-runtime --release --test endpoint_codec_preparation -- --nocapture
```

C#: the `endpoint prepared codec allocation accounting` smoke test reads
`GC.GetAllocatedBytesForCurrentThread()`. It compares the shared descriptor with
a fresh copy of the same text, which misses the identity tables and pays for
preparation again. The `endpoint JSON5 output limit on both routes` smoke test
also reports its 64 MiB calls:

```sh
FERRULE_ENDPOINT_PREPARATION_REPORT=1 dotnet run -c Release --project runtime/csharp/Ferrule.Runtime.SmokeTests
```

The fixtures are self-authored:

- **JSON5**: a closed 50-field object with two nested objects, as source and
  target. The Rust descriptors are 4,316 bytes each and the C# descriptors 3,326
  bytes each. The small document is 279 bytes. The large one carries a 1,000,000
  byte string (1,000,247 bytes) and maps to 1,000,281 bytes of output. The
  mapping returns the input as the target.
- **XML**: the pattern-free JSON order schema, with 53 fields and a repeated
  line group. The descriptor is 4,571 bytes in Rust and 3,545 bytes in C#. The
  small document has 3 lines (627 bytes) and the large one 20,000 lines
  (2,725,338 bytes). The observed-route rows use a flat root with two `xsi:type`
  alternatives.
- **X12**: a 004010 single envelope with a repeated two-element `NTE` segment.
  The descriptor is 3,818 bytes in Rust and 5,329 bytes in C#. The small
  interchange has 2 notes (277 bytes; 282 in C#) and the large one 20,000 notes
  (849,093 bytes; 849,098 in C#).

Both tests assert only the shape of the result. In Rust, a small call through a
shared JSON5, structured-XML, XML-document or XML-node preparation allocates
less than one decode of its descriptor. In C#, a small shared-descriptor call
allocates less than half of a fresh-copy call, or less than three quarters for
the observed flat root, whose reader dominates its small call. Exact counts
depend on the toolchain and allocator. The "before" columns come from the same
measurements on the parent revision. Rust 1.98.1 and .NET 10.0.112
(Microsoft.NETCore.App 10.0.12) ran on an AMD Ryzen 7 5800X3D (16 logical
CPUs). The host was heavily shared, with a load average of 37 to 86 during the
runs, so timings are indicative only. Allocation counts repeated exactly between
runs.

### Rust preparation (release)

| Operation | Allocations before | Allocations after | Allocated bytes before | Allocated bytes after | Peak live bytes before | Peak live bytes after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `map_json5_with`, small | 2,826 | 214 | 380,766 | 19,947 | 125,729 | 14,145 |
| `parse_structured_xml`, small | 367 | 127 | 122,940 | 29,112 | 81,587 | 12,936 |
| `parse_xml` (observed flat root), small | 98 | 66 | 15,789 | 9,506 | 5,169 | 2,339 |
| `serialize_xml_document`, small | 305 | 65 | 97,367 | 3,539 | 81,587 | 1,872 |
| `serialize_xml_document` with schema hints (observed flat root), small | 55 | 23 | 8,185 | 1,902 | 5,169 | 1,253 |
| `serialize_xml` (expression node), small | 302 | 62 | 95,750 | 1,922 | 81,587 | 1,003 |
| `parse_x12`, small | 730 | 510 | 102,840 | 21,235 | 48,624 | 10,921 |
| `serialize_x12`, small | 736 | 516 | 96,107 | 14,502 | 48,624 | 5,883 |
| `parse_structured_xml`, large | 140,359 | 140,119 | 72,478,196 | 72,384,368 | 50,457,940 | 50,405,376 |
| `serialize_xml_document`, large | 240,281 | 240,041 | 14,185,569 | 14,091,741 | 7,496,494 | 7,443,930 |
| `parse_x12`, large | 599,743 | 599,523 | 35,494,524 | 35,412,919 | 20,088,186 | 20,050,667 |
| `serialize_x12`, large | 579,728 | 579,508 | 25,544,473 | 25,462,868 | 11,414,315 | 11,376,796 |

The byte-taking variants match their text rows. Small calls took 500–1,260 µs
before and 24–69 µs after for JSON5, 190–400 µs before and 34–73 µs after for
structured XML input, 150–460 µs before and 24–44 µs after for XML documents, and
510–960 µs before and 54–113 µs after for X12. One decoded JSON5 descriptor
retains 49,785 live bytes, and one decoded XML descriptor 52,564 bytes, about
11.5 times their text.

### C# preparation (Release)

| Operation | Fresh copy before | Shared before | Fresh copy after | Shared after |
| --- | ---: | ---: | ---: | ---: |
| `FerruleJson5.Execute`, small | 587,400 | 574,040 | 587,703 | 41,688 |
| `FerruleX12.ParseEmbedded`, small | 120,232 | 109,552 | 120,372 | 22,792 |
| `FerruleX12.SerializeEmbedded`, small | 112,448 | 101,768 | 112,589 | 15,008 |
| `FerruleXml.ParseStructuredEmbedded`, small | 134,581 | 127,471 | 134,645 | 58,984 |
| `FerruleXml.ParseEmbedded` (observed flat root), small | 31,501 | 30,388 | 31,641 | 21,008 |
| `FerruleXml.SerializeDocumentEmbedded`, small | 46,040 | 38,928 | 34,700 | 8,424 |
| `FerruleXml.SerializeEmbedded` (expression node), small | 28,984 | 21,872 | 29,268 | 6,680 |

Values are allocated bytes per call. Before, a shared descriptor differed from a
fresh copy only by the copy itself. A fresh-copy document call now allocates
less because the writer no longer parses its schema a second time. Repeated C#
runs vary by up to about 1 KB per small call.

## Copy reductions

### Rust JSON and JSON5 output text

The JSON measurement attributed 8,388,480 of `serialize_json`'s 27,894,482
allocated bytes to growing the pretty text buffer, which peaked at 6,291,456
bytes for 2,425,267 bytes of output. JSON5 output showed the same pattern:
3,000,392 bytes allocated for 1,000,281 bytes of text.

Both boundaries now measure the text with one serialization pass that writes
nothing, apply their output byte limit to that length, and write the text once
into a buffer of exactly that size. Serialization errors and their order are
unchanged, because the measuring pass reports the same errors as the writing
pass. An output above the limit is refused before it is written.

| Operation | Allocations before | Allocations after | Allocated bytes before | Allocated bytes after | Peak live bytes before | Peak live bytes after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `serialize_json`, large | 300,056 | 300,041 | 27,894,482 | 21,931,269 | 18,806,537 | 14,940,348 |
| `serialize_json`, small | 88 | 86 | 12,022 | 11,637 | 9,000 | 9,000 |
| `map_json5_with`, large (after preparation) | 228 | 226 | 10,116,506 | 8,116,395 | 6,013,919 | 4,013,936 |

Allocation drops by 21% and the peak by 21% for large JSON output. For JSON5,
allocation drops by 20% and the peak by 33%. The measuring pass costs time: it
took about 2.0 ms for the 2.4 MB JSON output, against 5.3 ms to write the same
text into a growing buffer, and 0.5 ms against 1.2 ms for the JSON5 output.

### C# X12 byte output

`SerializeEmbeddedBytes` built the output in a `StringBuilder`, materialized it
as a string (2 bytes per character) and then encoded it. The writer already
counts each piece's strict UTF-8 length while appending, so byte routes now
encode the builder's chunks directly into one array of that length. The encoder
carries a surrogate pair split across two chunks.

| Operation | Before | Preparation only | After |
| --- | ---: | ---: | ---: |
| `FerruleX12.SerializeEmbeddedBytes`, large | 19,131,784 | 19,045,192 | 17,348,000 |
| `FerruleX12.SerializeEmbedded`, large | 18,282,656 | 18,196,064 | 18,196,064 |

The byte route now allocates 1,697,192 fewer bytes, about the size of the
849,098-character output string.

### C# JSON5 output

`Json5Serialize` materialized the canonical builder as a string, counted its
UTF-8 bytes, applied the output limit, and `ExecuteBytes` then encoded that
string. The output is now counted from the builder's chunks. The text route
materializes the string only after the limit admits it. The byte route encodes
the builder directly into one array, as X12 does. The canonical text is decoded
from the writer's UTF-8 and is always well-formed. Should counting ever fail,
the original boundary census runs on the materialized text and reports its
original error.

| Operation | Before | Preparation only | After |
| --- | ---: | ---: | ---: |
| `FerruleJson5.ExecuteBytes`, large | 20,611,008 | 20,078,608 | 18,078,120 |
| `FerruleJson5.Execute`, large | 17,610,176 | 17,077,776 | 17,077,824 |

At the 67,108,864-byte output limit of the [#160](json5-allocations.md) and
[#765](json5-newline.md) studies, one call each:

| Operation | Before | After |
| --- | ---: | ---: |
| `Execute`, output of exactly the limit | 738,763,264 | 738,230,912 |
| `ExecuteBytes`, output of exactly the limit | 805,872,216 | 671,122,160 |
| `Execute`, output one byte above the limit | 738,766,152 | 604,016,344 |
| `ExecuteBytes`, output one byte above the limit | 738,765,680 | 604,023,968 |

These totals include building the mapped 64 MiB string inside each call. The
byte route and both refusals no longer allocate the 134 MB output string. The
#765 change had shown no improvement on the refusal path. Both routes return
exactly the limit and refuse one more byte with `OutputDocumentBytes`, requested
67,108,865 and maximum 67,108,864, both before and after the change.

## Remaining per-call costs

| Runtime | Cost | Large-document measurement | Share | Status |
| --- | --- | ---: | ---: | --- |
| Rust | `roxmltree` DOM of structured XML input | 72,382,352 bytes allocated, 50,405,376 peak for 2,725,338 bytes | 99.9% of `parse_structured_xml` | Owned by the format reader; removing it means a streaming reader |
| Rust | Owned JSON input tree (see [JSON](json-endpoint-preparation.md)) | 29,308,461 bytes, 19,669,370 peak | 70% of `parse_json` | Remaining |
| Rust | Normalized JSON `serde_json::Value` of the output | 19,618,380 bytes, 12,538,652 peak | 89% of `serialize_json` | Remaining |
| Rust | JSON5 normalized strict text | 2,097,157 bytes, 1,572,872 peak | 26% of `map_json5_with` | Remaining; see below |
| Rust | X12 filtered native input text | about 1.7 MB including growth | 5% of `parse_x12` | Measured, not changed; see below |
| C# | UTF-8 to UTF-16 decoding of byte input | JSON5 2,000,520; X12 1,698,224; XML 5,450,704 bytes | 11%, 6% and 4% of their byte routes | Remaining; the readers take strings |
| C# | Ordinary JSON final newline and byte encoding | See [JSON](json-endpoint-preparation.md) | 7.6% and 3.7% | Remaining; `FerruleJson` is being changed by other work |

Two candidates were measured and left unchanged:

- Reserving the exact X12 native input text cut `parse_x12` allocation by
  907,511 bytes (2.6%) but its peak by only 39,265 bytes. The peak is reached
  later, in the native reader.
- The JSON5 normalizer grows its output from empty. Reserving the input length
  would remove the growth for ordinary documents, but a comment-padded document
  would reserve up to 64 MiB for a few bytes of output.

## Limits

The 64 MiB document limits count document bytes, not process memory. A
2.7 MB structured XML document still peaks at 50 MB of live heap in Rust, and a
C# JSON5 call near its output limit still allocates 600–740 MB in total. The registry bounds
count descriptor text, while prepared schemas take about an order of magnitude
more memory and stay alive for the process lifetime, or until a C# descriptor
string is collected.
