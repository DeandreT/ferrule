# MapForce Interoperability

ferrule provides clean-room, best-effort import and export for MapForce
`.mfd` mapping designs. Vendor samples may be used as black-box behavioral
references, but ferrule's implementation and committed fixtures are original.

## Import

```sh
cargo +nightly run -p cli -- import-mfd --mfd design.mfd --out project.json
```

Import resolves the supported component graph into ferrule schemas, graph
nodes, scopes, format options, and endpoints. Current coverage includes common
XML, JSON, CSV/fixed-width/FlexText, XLSX, SQLite, EDI, Protocol Buffers, XBRL,
HTTP XML, and visual PDF source components, together with a broad set of scalar
functions, aggregates, sequence controls, lookups, joins, exceptions, and
recognized user-function shapes. Adjacent XSLT extension modules also import
when a named one-parameter template returns a direct count, sum, average,
minimum, or maximum over a descendant path; ferrule lowers that template to a
native aggregate rather than retaining an XSLT runtime dependency.
Bounded adjacent C# and Java source modules can likewise lower direct numeric
picture wrappers to ferrule's deterministic formatter, while bounded XQuery
modules can lower scalar parameter/number arithmetic to the native call graph.
Structured XML string serializers retain the selected subtree schema and emit
attributes, nested groups, repeated children, escaping, the configured default
namespace, and optional XML declaration directly from the current source item.
Structured XML database columns reuse that typed serializer with compact output,
so document-valued TEXT fields execute without flattening the source subtree.
SQLite `LocalRelationsStorage` declarations are retained as exact typed relation
endpoints, validated against the physical columns, and exported canonically. This
keeps nested relational reads executable when the database omits foreign-key metadata.
Filter components downstream from grouping retain their operator order: a
group survives when any member satisfies the predicate, and sparse typed member
ports resolve within that retained group.
External EDI configurations may be ordinary sibling directories or adjacent
ZIP packages. Packages are extracted under strict path, entry-count, compressed,
and expanded-size limits; the resulting X12/EDIFACT schema and lexical metadata
are embedded in the imported project, so execution and later export do not
depend on the package remaining available.

Import is deliberately resilient: unsupported constructs are skipped with one
actionable warning where possible. A design is rejected only when no usable
source or target can be recovered.

## Export

```sh
cargo +nightly run -p cli -- export-mfd --project project.json --out design.mfd
```

Export writes the representable project subset plus generated schema or layout
siblings. Component kinds are selected from endpoint format metadata and paths.
Supported named sources, independent targets, dynamic XML paths, HTTP response
boundaries, selected joins, exception sinks, and configured format components
retain their ownership in the exported design. Structured XML string serializers
round-trip as native components with generated XSD siblings and structural
source connections. Declared local SQLite relations round-trip with their owning
database connection.

Export is atomic: a shape that cannot be represented safely is rejected instead
of publishing a partially wired design. Successfully exported designs are
expected to re-import and validate as ferrule projects.

## Current Boundaries

The main remaining gaps are some XML derived-type input shapes, cross-namespace
substitution export, namespace-dependent wildcards, general scalar/array,
overlapping cross-mode, and incompatible typed-wrapper JSON union composition,
first-class sequence composition, general SQL and database mutation, broader
XLSX/PDF/FlexText configuration shapes, taxonomy-level XBRL execution, and
direct execution of unrecognized or external-service user components. Bounded
same-namespace substitution groups and exact nullable scalar JSON unions are
preserved. Expanded-name identity for ordinary elements and attributes is
preserved; foreign declarations export as an atomic graph of local XSD siblings.

The exact supported surface evolves quickly. The
[workflow-parity roadmap](../ROADMAP.md) records the strategic gaps, while the
`mfd` test suite contains self-authored regression designs for executable
behavior.

## Trademark

MapForce is a trademark of its owner. ferrule is an independent project and is
not affiliated with or endorsed by that owner.
