# MapForce Interoperability

ferrule provides clean-room, best-effort import and export for MapForce
`.mfd` mapping designs. Vendor samples may be used as black-box behavioral
references, but ferrule's implementation and committed fixtures are original.

## Import

```sh
cargo +nightly run -p cli -- import-mfd --mfd design.mfd --out project.json
```

For a mapping package whose resources sit above or beside the design directory,
declare the trusted package root:

```sh
cargo +nightly run -p cli -- import-mfd \
  --mfd package/maps/design.mfd \
  --package-root package \
  --out project.json
```

Resource references accept both slash styles and may contain parent components
when their canonical target remains inside the package. Symlink escapes,
absolute Windows paths, ambiguous case-insensitive matches, and traversal above
the root are rejected.

If EDI configurations live in a separately managed release catalog, declare
each trusted catalog in search order:

```sh
cargo +nightly run -p cli -- import-mfd \
  --mfd package/maps/design.mfd \
  --package-root package \
  --edi-catalog-root edi-configs/current \
  --edi-catalog-root edi-configs/archive \
  --out project.json
```

Package-contained configurations take precedence. Catalog lookup accepts
portable and Windows-style locators, including leading installation-relative
parent components, but re-anchors them under the declared catalog instead of
performing filesystem traversal. Direct files and bounded adjacent ZIP packages
must remain canonically contained in that catalog.

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
Imported XSD contracts expand bounded named model and attribute groups, retain
typed scalar element/simple-content/attribute defaults, and materialize those
defaults at the XML input boundary.
Structured XML database columns reuse that typed serializer with compact output,
so document-valued TEXT fields execute without flattening the source subtree.
SQLite `LocalRelationsStorage` declarations are retained as exact typed relation
endpoints, validated against the physical columns, and exported canonically. This
keeps nested relational reads executable when the database omits foreign-key metadata.
Filter components downstream from grouping retain their operator order: a
group survives when any member satisfies the predicate, and sparse typed member
ports resolve within that retained group.
External EDI configurations may be ordinary package resources, explicitly
trusted catalog resources, or adjacent ZIP packages. Packages are extracted
under strict path, entry-count, compressed, and expanded-size limits; the
resulting X12/EDIFACT schema and lexical metadata are embedded in the imported
project, so execution and later export do not depend on the package or catalog
remaining available.
When an external EDI configuration cannot be resolved, its original reference
is retained for `.mfd` export and re-import instead of being discarded. That
keeps the design round-trippable, but the boundary remains explicitly
non-executable until the referenced configuration is supplied and compiled.
Zero-input `create-guid` generator components execute in the interpreter and
generated Rust/C# mappings and round-trip as native `lang` components. Scalar
and record-producing filter lookup UDFs accept typed XML, EDI, or database
inputs. Scalar and nested scalar UDFs can also tokenize text, split by fixed
length or bounded regular expressions, or generate an inclusive integer range,
then select one 1-based item, test a filtered sequence for a match, or apply
count, sum, average, minimum, maximum, or string-join to raw, filtered, or
per-item computed generated values. Import lowers those compositions to
ferrule's native generated-sequence reducers, so interpreter execution,
Rust/C# generation, and export/re-import share one bounded implementation.
Filtered predicates and computed values can use the generated item's 1-based
`position()` as well as the item value.

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

The main remaining gaps are some XML derived-type input shapes beyond compatible
`complexContent` and scalar-text/attribute-only `simpleContent` hierarchies,
XSD 1.1 wildcard exclusions, unordered wildcard compositors, unresolved strict
wildcard declaration sets, numeric-range-bearing scalar unions and general heterogeneous arrays,
overlapping cross-mode, and incompatible typed-wrapper JSON union composition,
first-class sequence composition, general SQL and database mutation, broader
XLSX/PDF/FlexText configuration shapes, taxonomy-level XBRL execution, and
direct execution of unrecognized or external-service user components. Bounded
cross-namespace substitution groups, heterogeneous scalar type arrays,
pairwise-disjoint scalar `oneOf`, exact scalar `anyOf` unions, and array `anyOf`
branches whose scalar item domain subsumes every narrower branch are preserved,
including required or optional typed
`const` or singleton-`enum` discriminators and JSON-null discriminators.
Flat nullable compositions may combine null with multiple compatible object,
scalar-union, or subsumed-array branches.
JSON components retain exact ordinary integer and finite-number ranges from
their referenced schemas, including nullable numeric fields and compatible
`allOf` intersections. Those constraints apply when imported mappings read
source documents and write targets; malformed, empty, or precision-ambiguous
ranges trigger the component's existing schema-fallback diagnostic instead of
silently widening the mapping boundary.
Referenced JSON arrays likewise retain exact `minItems`/`maxItems` intervals
through references, nullable wrappers, and compatible compositions. Invalid or
nonrepresentable item-count unions use the same actionable schema fallback;
valid constraints remain executable after MFD import.
Referenced string-capable JSON fields retain exact `minLength`/`maxLength`
intervals measured in Unicode scalar values, including nullable fields, array
items, typed dynamic properties, and compatible compositions. The constraints
remain executable on imported source and target boundaries and round-trip
alongside opaque `format` annotations.
Expanded-name identity for ordinary elements and attributes is preserved;
foreign declarations export as an atomic graph of local XSD siblings.
Compatible strict-wildcard declarations that share one local name across
namespaces use one mapping port with exact expanded-name alternatives. Selected
QName ports narrow that ambiguity before target construction; incompatible
same-local shapes remain actionable warnings.
Namespace-constrained optional/unbounded element wildcards with
`processContents="skip"` round-trip as recursive generic element groups, while
closed strict wildcards become exact singular or repeating typed choices. Lax
element wildcards in sequences and repeating choices expose resolved global
declarations as typed fields and route only undeclared matching names to the
generic fallback. Direct or named-attribute-group wildcards preserve namespace
constraints and processing mode; known declarations remain typed, lax unknowns
remain generic, and strict unknowns reject at the XML boundary.

The exact supported surface evolves quickly. The
[workflow-parity roadmap](../ROADMAP.md) records the strategic gaps, while the
`mfd` test suite contains self-authored regression designs for executable
behavior.

## Trademark

MapForce is a trademark of its owner. ferrule is an independent project and is
not affiliated with or endorsed by that owner.
