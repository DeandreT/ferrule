# XLSX instance I/O

The existing readers and writers support the crate's flat and structured layouts.
They retain their existing behavior and do not acquire resource limits implicitly.

## Opt-in bounded flat workbooks

`from_bytes_bounded`, `to_bytes_bounded`, and
`to_bytes_with_options_bounded` accept an immutable `XlsxLimits`. Construct it
with `XlsxLimits::new(XlsxLimitOptions { ... })`, or use its finite default.
The options accessor is read-only; changing a configuration requires validation
again. Zero limits, values above `isize::MAX`, coordinates above Excel's limits,
and XML depth above 256 reject as `InvalidLimit`.

These APIs are in-memory only. They use the same flat schema, one-based sheet
coordinates, selected columns, header detection, explicit physical headers,
null/empty-cell handling, and typed values as the existing flat APIs. Integer
writes retain the exact f64 range check; numeric XML that could round before
conversion to a selected integer field rejects as `Parse`. Float fields retain
the finite f64 domain, including large/small values. Formula computation, external
resource fetching, update-existing writes, and non-flat layouts are not included.

### Read boundary

Before calamine is constructed, the reader:

- Checks compressed input bytes and the actual central-directory entry count.
- Validates central/local header agreement, contiguous non-overlapping entries,
  canonical part names, CRCs, and actual expanded lengths. Every stream is read
  in fixed chunks and stopped at one byte beyond either its per-entry or shared
  expansion budget. Declared sizes are never used for buffer reservation.
- Scans every XML part under shared node/text budgets and a per-document depth
  cap. Text limits include decoded attributes; each attribute and concatenated
  cell/shared string also has a string-size cap. DTDs and custom entities reject.
- Bounds both declared shared-string reservation counts and actual strings.
- Checks every worksheet's explicit row/cell coordinates, cell count and dense
  bounding rectangle, including empty cells and unselected sheets. A misleading
  `dimension` cannot hide a sparse faraway cell. Declared dimensions are separately
  bounded because calamine can reserve from them.
- Bounds the selected row-by-field projection and a conservative materialized
  text charge before creating a range. The charge includes shared-string
  replication, copies in the returned rows, field names and scalar formatting.

`Rows` and `Columns` cap physical coordinates as well as data size. `Cells` is
shared across worksheets; `DenseCells` applies to each worksheet's bounding
rectangle, not merely the number of populated cells. `MaterializedCells` caps the
selected physical row span times selected field count, including skipped gaps.
`XmlNodes` counts XML events (including closing tags and comments), not just
elements. `XmlTextBytes` counts UTF-8 text and attribute values across the package.

The bounded profile deliberately accepts a narrower package syntax: classic
single-disk ZIP with stored/deflate streams, optional standard data descriptors,
no archive/entry comments, extra fields, ZIP64, encryption, directories, prefixes
or hidden entries. Names are canonical ASCII and case aliases/duplicates reject.
Allowed parts are the content-types manifest, package/workbook relationships,
workbook, styles, shared strings, numbered worksheet/theme parts, and core/app
properties. The operational XML uses the transitional OOXML default namespaces;
prefixed elements and nested namespace changes reject. Rows/cells must have
explicit, increasing coordinates. Worksheet formulas/extensions and external
relationships reject rather than being silently reinterpreted. Metadata and
theme XML are scanned too; no embedded binaries, macros, links or images load.

### New-workbook write boundary

The writer checks row/group shape, field types, physical coordinates, counts,
strings (including lexical numeric input), and conservative XML growth before
constructing a workbook. It reuses the existing value conversion rules. A
`Write` adapter refuses any write that would grow the returned ZIP buffer beyond
`OutputBytes`; actual generated ZIP/XML is validated again before return. No
partial buffer is returned on failure. Limits are per call, so a failed workbook
does not poison a corrected retry.

The write preflight deliberately overcounts repeated strings, field names and
fixed metadata. Consequently a tight budget may reject a workbook whose final
compressed bytes would fit. Input-byte limits apply to reads; output-byte limits
apply to writes. The other limits apply to the corresponding structures in each
operation.

### Guarantee and limitations

`LimitExceeded { limit, maximum, actual }` identifies a budget failure; malformed
packages and unsupported profile features have separate typed error variants.
Budgets bound format-owned data growth and the dimensions/counts supplied to
calamine/rust_xlsxwriter. They are **not an exact process-RAM ceiling**: the input
and caller-owned schema/instances already exist, vectors/allocators have overhead
and spare capacity, decompression has fixed internal buffers, and the writer
materializes XML internally before emitting ZIP bytes. Several bounded copies
can coexist. Use a host process/memory limit when a hard RSS ceiling is required.
