# JSON Endpoint Preparation

Generated JSON boundaries used to decode and validate their embedded schema
descriptor on every call. Both runtimes now prepare each descriptor once per
process and share the immutable result across repeated and concurrent calls.
Every call still keeps its own work accounting. For a small document this
removes most of the per-call cost. Large documents remain dominated by owned
JSON trees, which this change leaves in place.

## Ownership

| | Prepared once, immutable, shared | Per call |
| --- | --- | --- |
| Rust | Decoded `SchemaNode` with boundary constraints; the constraint-free reader/writer schema; compiled pattern programs; the `uniqueItems` presence flag | Document byte check; UTF-8 check; pattern work budget; recursive-anchor view; parsed and normalized JSON trees; output byte check |
| C# | Parsed `JsonSchemaNode` graph, including compiled patterns and bound recursive references | Schema and document byte checks; UTF-8 decoding; `NodeBudget` (depth, recursion, pattern work); parsed document; output buffer and byte check |

In Rust, `codegen_runtime::PreparedJsonSchema` owns a prepared descriptor and
exposes the same parse and serialize entry points as the descriptor-taking
functions. Those functions, which generated mappings call with their descriptor
constants, use a process-wide registry keyed by descriptor text. It retains up
to 64 descriptors totalling 4 MiB of descriptor text, and admitting one more
clears it. `format_json::PreparedSchema` splits the format reader's pattern
programs from each document's work budget.

In C#, `FerruleJson` keys prepared schemas by descriptor string identity in a
`ConditionalWeakTable`. Generated descriptor constants are interned, so each one
is prepared once. A descriptor built at run time is prepared once per string
object and released with it. The JSON5, XML and X12 codecs now prepare their
descriptors the same way; see [endpoint codec preparation](endpoint-codec-preparation.md).

Neither runtime retains a failed preparation. A rejected descriptor is parsed
again by every call, so it reports its original error in its original order:
the document byte limit first, then descriptor errors, then document errors.
Format-level schema errors in Rust still follow JSON syntax errors. Generated
Rust and C# mapping sources are unchanged; the embedded C# runtime gains
`Json/FerruleJson.Prepared.cs`.

## Measurement

Rust: `crates/codegen-runtime/tests/json_endpoint_preparation.rs` installs a
counting global allocator. For each operation it reports the mean allocations,
allocated bytes and peak live bytes on the measuring thread, plus mean
wall-clock time:

```sh
cargo test -p codegen-runtime --release --test json_endpoint_preparation -- --nocapture
```

C#: the `JSON prepared schema allocation accounting` smoke test reads
`GC.GetAllocatedBytesForCurrentThread()`. It compares the shared descriptor with
a fresh copy of the same text, which misses the identity table and so pays for
preparation again:

```sh
FERRULE_JSON_PREPARATION_REPORT=1 dotnet run -c Release --project runtime/csharp/Ferrule.Runtime.SmokeTests
```

Both measurements use the same self-authored order schema: 53 fields, 16 of
them with patterns, an integer range, and a repeated line group. The Rust
descriptor is 5,352 bytes and the C# descriptor 4,326 bytes. The small document
has 3 lines (352 bytes). The large one has 20,000 lines (1,585,234 bytes) and
serializes to 2,425,267 bytes. Both tests assert only the shape of the result:
in Rust a small call allocates fewer times than one descriptor decode, and in
C# a shared-descriptor call allocates less than half as much as a fresh copy.
Exact counts depend on the toolchain and allocator. The "before" columns come
from the same measurements run on the parent revision. Timings come from a
shared machine and are indicative only.

### Rust (1.98.1, release)

| Operation | Allocations before | Allocations after | Allocated bytes before | Allocated bytes after | Peak live bytes before | Peak live bytes after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `parse_json`, small | 4,354 | 163 | 400,183 | 12,653 | 138,268 | 8,676 |
| `serialize_json`, small | 4,279 | 88 | 399,552 | 12,022 | 138,592 | 9,000 |
| `parse_json`, large | 444,301 | 440,110 | 42,210,435 | 41,822,905 | 25,911,564 | 25,781,972 |
| `serialize_json`, large | 304,247 | 300,056 | 28,282,012 | 27,894,482 | 18,913,501 | 18,806,537 |

A small `parse_json` call took 224–259 µs before and about 14 µs after. A small
`serialize_json` call took 240–270 µs before and about 10 µs after. Large-document
timings did not change beyond run-to-run noise.

The one-time `PreparedJsonSchema::prepare` costs 4,196 allocations and 388,339
bytes, and retains 129,592 live bytes, about 24 times the descriptor text. Of
that preparation cost, decoding the descriptor accounts for 1,362 allocations
(165,712 bytes) and cloning the decoded schema for 105. The rest is format-level
validation and pattern compilation, which the format reader and writer used to
repeat for every document.

### C# (.NET 10.0.112, Release)

| Operation | Allocated bytes before | Allocated bytes after |
| --- | ---: | ---: |
| `Parse`, small | 337,600 | 38,864 |
| `ParseBytes`, small | 338,328 | 39,592 |
| `Serialize`, small | 317,680 | 18,944 |
| `ParseBytes`, large | 110,608,736 | 110,310,010 |
| `Serialize`, large | 63,891,104 | 63,592,360 |
| `SerializeBytes`, large | 66,316,392 | 66,017,656 |

A fresh-copy call still pays for preparation: 346,280 bytes before and 344,661
after. Repeated C# runs vary by up to about 1 KB per call.

## Remaining per-call costs

Preparation was the only per-call cost that did not scale with the document.
The remaining costs are proportional to document size:

| Runtime | Cost | Large-document measurement | Share |
| --- | --- | ---: | ---: |
| Rust | Owned `serde_json::Value` tree of the input | 29,308,461 bytes allocated, 19,669,370 peak | 70% of `parse_json` allocation |
| Rust | Normalized `serde_json::Value` tree of the output | 19,618,380 bytes allocated, 12,538,652 peak | 70% of `serialize_json` allocation |
| Rust | Pretty text growth, since removed (see [endpoint codec preparation](endpoint-codec-preparation.md#rust-json-and-json5-output-text)) | 8,388,480 bytes over 16 allocations, 6,291,456 peak | 30% of `serialize_json` allocation |
| C# | UTF-8 to UTF-16 decoding of byte input | 3,170,496 bytes | 2.9% of `ParseBytes` |
| C# | Final newline concatenation of pretty output, since removed (see [endpoint codec preparation](endpoint-codec-preparation.md#c-ordinary-json-output)) | 4,850,560 bytes | 7.6% of `Serialize` |
| C# | UTF-16 to UTF-8 encoding of byte output; byte routes now encode the builder without the UTF-16 string (see [endpoint codec preparation](endpoint-codec-preparation.md#c-ordinary-json-output)) | 2,425,296 bytes | 3.7% of `SerializeBytes` |

Rust re-encodes nothing. `parse_json_bytes` validates UTF-8 in place,
`serialize_json_bytes` reuses the string's buffer, and the output text, including
its newline, is now written into a buffer of its exact length. `System.Text.Json` parses C# input from pooled
buffers, which these counts do not include. The rest of the C# parse cost is the
owned instance tree.

The largest remaining costs are the two owned Rust JSON trees. Removing the
input tree requires reading schema-shaped instances directly from the JSON text.
Removing the output tree requires applying output constraints to the borrowed
writer that the filesystem JSON writer already uses. Both are streaming
rewrites and are not pursued. The Rust output text growth and the C# newline
and UTF-16 output copies were later removed; see
[endpoint codec preparation](endpoint-codec-preparation.md).

## Limits

The 64 MiB document limits count document bytes, not process memory. In the
Rust fixture, a 1.58 MB input peaks at 25.8 MB of live heap during parsing. The
Rust registry bounds count descriptor text, while prepared schemas take about
an order of magnitude more memory and stay alive for the process lifetime.
