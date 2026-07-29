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

For a relocatable package, put `ferrule-package.json` at its root and select it
explicitly:

```json
{
  "schemaVersion": 1,
  "kind": "ferrule.mapping-package",
  "catalogs": [
    { "kind": "edi-config", "root": "resources/edi" },
    { "kind": "json-schema", "root": "resources/json-schema" }
  ]
}
```

```sh
cargo +nightly run -p cli -- import-mfd \
  --mfd package/maps/design.mfd \
  --package-manifest package/ferrule-package.json \
  --out package/projects/project.json
```

The manifest must be selected by the host or user; a mapping cannot grant
itself filesystem access. Its directory is the package trust boundary, and
catalog entries are relative, traversal-free directories inside that boundary.
Direct catalog flags are searched before manifest catalogs. The GUI stores an
explicitly selected manifest as a host preference, never in the mapping
project. `--package-root` and `--package-manifest` are mutually exclusive.

Resource references accept both slash styles and may contain parent components
when their canonical target remains inside the package. Symlink escapes,
absolute Windows paths, ambiguous case-insensitive matches, and traversal above
the root are rejected.
External FlexText `.mft`, visual PDF `.pxt`, and XBRL `.sps` compiler inputs
use this same package boundary. Their compiled layouts or fact metadata are
embedded in the imported project, and FlexText data paths remain portable
relative to the mapping even when the configuration is in a sibling directory.
Protocol Buffers components likewise resolve their declared root through the
package boundary, accepting Windows separators and safe parent traversal. The
selected package root is the virtual include root; exported `*-protobuf`
directories remain narrower self-contained include roots. Every reachable
`.proto` is loaded with bounded canonical containment and embedded under its
portable logical path, so execution and later export do not reopen the package.
Typed HTTP response XSDs, HTTP POST request JSON Schemas, and WSDL message XSDs
also resolve from the package boundary. Local references below an HTTP request
JSON Schema remain confined to the root that authorized the schema; endpoint
URLs, preview instance paths, and retained WSDL service contract locators are
host metadata and are not rewritten as package resources.

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
Every transitive EDI include, selected message configuration, and SWIFT common
definition remains confined to the root that authorized the main configuration.

Separately managed JSON Schema catalogs use the same explicit ordered trust
model:

```sh
cargo +nightly run -p cli -- import-mfd \
  --mfd package/maps/design.mfd \
  --package-root package \
  --json-schema-root schemas/current \
  --json-schema-root schemas/archive \
  --out project.json
```

Package-contained schemas take precedence. A catalog lookup normalizes Windows
separators and re-anchors only leading installation-relative parent components
under each catalog root. Parent traversal after a real path segment, absolute
paths, ambiguous case-insensitive matches, and symlink escapes reject. Once the
root schema matches a catalog, its nested local `$ref` graph remains confined
to that same canonical root.

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
An EDI boundary with neither a compiled configuration nor an embedded typed
layout is also retained as a distinct typed missing-configuration dependency.
It never borrows the untyped entry tree as an executable schema. Export and
re-import preserve that state without inventing a resource path, and CLI or
payload execution rejects it before publishing any output.
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

Static source, target, named-source, and named-target paths are rebased when
the generated project is written somewhere other than the design directory.
HTTP URLs and graph-computed paths are unchanged. Moving or using Save As on a
project applies the same rebasing rule, including wildcard input paths.

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
wildcard declaration sets, heterogeneous or correlated numeric-range scalar
unions and general heterogeneous arrays,
overlapping cross-mode, and incompatible typed-wrapper JSON union composition,
first-class sequence composition, general SQL and database mutation, broader
XLSX/PDF/FlexText configuration shapes, taxonomy-level XBRL execution, and
direct execution of unrecognized or external-service user components. Bounded
cross-namespace substitution groups, heterogeneous scalar type arrays,
pairwise-disjoint scalar `oneOf`, exact scalar `anyOf` unions, bounded exact
multi-value scalar `enum` domains, and array `anyOf` branches whose scalar item
domain subsumes every narrower branch are preserved, including required or
optional typed and JSON-null discriminators.
Flat nullable compositions may combine null with multiple compatible object,
scalar-union, or subsumed-array branches.
JSON components retain exact ordinary integer and finite-number ranges from
their referenced schemas, including nullable numeric fields and compatible
`allOf` intersections. Those constraints apply when imported mappings read
source documents and write targets; malformed, empty, or precision-ambiguous
ranges trigger the component's existing schema-fallback diagnostic instead of
silently widening the mapping boundary.
Positive finite JSON Schema `multipleOf` divisors likewise retain exact
canonical decimal semantics through referenced schemas, compatible
compositions, MFD import, and generated schema export. Native and generated
Rust/C# boundaries enforce the divisor on input and normalized output.
Correlated unions that vary both a numeric range and its paired divisor fall
back with an actionable diagnostic rather than being widened.
Referenced JSON arrays likewise retain exact `minItems`/`maxItems` intervals
through references, nullable wrappers, and compatible compositions. Invalid or
nonrepresentable item-count unions use the same actionable schema fallback;
valid constraints remain executable after MFD import.
Referenced array `uniqueItems: true` assertions also remain executable across
MFD import and generated-schema export, with exact structural comparison on
native and generated Rust/C# JSON boundaries.
Referenced arrays also retain bounded `contains` assertions through MFD import,
canonical schema export, and re-import. Plain assertions require at least one
matching member; Draft 2019-09 and newer `minContains`/`maxContains` modifiers
retain an exact match-count interval. Compatible `allOf` assertions remain
conjunctive, nullable array null bypasses them, and predicate patterns use the
same bounded document matcher budget as ordinary string constraints.
Referenced JSON objects preserve their exact openness contract as well.
Omitted or `true` `additionalProperties` remains an unconstrained dynamic
property domain, schema-valued declarations remain typed, and explicit `false`
rejects undeclared input properties instead of discarding them. MFD export
writes a canonical JSON Schema sibling and re-import retains the same open,
typed-open, or closed behavior.
Referenced schemas also retain exact closed homogeneous `patternProperties`.
The containing schema must explicitly type `object` or `object | null`, use
`additionalProperties: false`, and assign every portable selector in a
nonempty bounded map one identical exactly representable value schema. Scalar,
structured object, and homogeneous array values reuse the ordinary supported
JSON value profile. Selectors are ORed in declaration order. Any matching fixed
property must have that schema; nonmatching fixed properties remain
independent. Runtime decoding selects fixed properties first, then applies the
selector set to remaining names, while `propertyNames` continues to constrain
every key independently. Dependency rules whose triggers are neither fixed nor
selected normalize away as semantically unreachable. Nullable null bypasses
the object checks. Native and generated Rust/C# boundaries enforce the
selectors on input, normalized output, and every JSON Lines row with the shared
per-document pattern work budget. Canonical MFD schema export and re-import
preserve the selector map and closed fallback. An empty map is a no-op.
Referenced JSON objects also retain exact `minProperties`/`maxProperties`
intervals through nullable wrappers, dialect-aware references, compatible
`allOf`, and alternatives with one common effective interval. Imported
mappings enforce each distinct parsed input-property set and normalized output
object in native and generated Rust/C# boundaries; canonical MFD export and
re-import retains the interval. Unsatisfiable required/closed-object bounds and
correlated alternative intervals produce the existing actionable
schema-fallback diagnostic rather than silently widening the contract.
Referenced JSON objects retain executable property dependencies as well.
Modern `dependentRequired` and legacy property-array `dependencies` normalize
to the same bounded relation and export canonically as `dependentRequired`.
Native and generated Rust/C# boundaries require every dependent name whenever
its trigger is present, counting explicit JSON null as input presence and
checking normalized output after absent fields are omitted. Nullable object
null bypasses the rule, compatible `allOf` branches unite rules, and alternatives
must share one identical effective relation. Schema-valued legacy dependencies
and modern `dependentSchemas` are executable when their whole-object predicate
fits Ferrule's retained JSON subset. Required-only predicates lower to the
property-dependency relation; nontrivial predicates retain nested ordinary
object/array constraints and export canonically as `dependentSchemas`.
Repeated rules for a trigger remain conjunctive through export and re-import;
ordered outer `allOf` branches preserve interleaved trigger declaration order.
Nested dependent schemas retain the same recursively bounded behavior. Draft 7,
2019-09, 2020-12, and undeclared explicitly typed object schemas may also
express one of these rules as an `if` with exactly one required-property
presence trigger and a supported `then` predicate. A nullable outer object is
supported with an absent or `true` `else` only when the `if` explicitly proves
`type: "object"`. Exact `else: false` requires a trigger that can be represented
as an ordinary required field; it removes the nullable bypass when the false
branch rejects null and retains any supported `then` dependency. It does not
combine with existing object alternatives, and a closed object must already
declare the trigger. Import normalizes these forms to `required` plus
`dependentRequired` or `dependentSchemas` metadata as appropriate, which
canonical MFD schema export preserves. Value-sensitive, multi-trigger,
general-`if`, other nontrivial `else` schemas, distinct per-selector
`patternProperties` schemas, open or typed pattern-property fallbacks, general
selector-overlap intersection, value shapes outside the ordinary exact JSON
profile, pattern-property objects under active `allOf`, alternatives, or
structural `$ref` siblings, unevaluated property keywords, and heterogeneous
positional array schemas still produce the existing actionable schema-fallback
diagnostic rather than being widened or discarded. Declared Draft 4/6/7
resources use legacy schema-valued `dependencies`; declared 2019-09/2020-12 use
modern `dependentSchemas`. Schemas without `$schema` intentionally accept both
spellings while retaining modern reference-sibling behavior, which preserves
older MFD schema packages without weakening explicitly declared dialects.
Referenced JSON arrays using homogeneous legacy tuple-form `items` also remain
executable through MFD import. Draft 4, 6, 7, and 2019-09 resources, plus
schemas without `$schema`, normalize 1 to 4,096 identical positional members
to one repeated item shape. An identical `additionalItems` tail remains
unbounded, while `false` closes the array at the tuple length. An absent or
`true` tail is accepted for an arbitrary-JSON item shape or when an explicit
maximum makes the tail unreachable; a different schema tail requires that
same maximum proof. The derived maximum intersects explicit
item-count bounds. MFD schema export writes ordinary schema-valued `items` and
an optional `maxItems`, so export/re-import and generated Rust/C# use the existing
homogeneous-array path. Draft 2020-12 array-valued `items`, contradictory
counts, and reachable heterogeneous members or tails produce the actionable
schema-fallback diagnostic instead of being reinterpreted as `prefixItems`.
Referenced JSON objects retain supported `propertyNames` schemas as well.
Exact false, finite `const`/`enum` names and their finite `not` complements,
Unicode-scalar length intervals, bounded portable pattern
conjunctions/disjunctions, and nonasserting `format` annotations remain
executable across import, canonical schema export, and re-import. Native and
generated Rust/C# boundaries check every raw parsed input key and normalized
emitted key, including runtime-named and empty-string properties.
Unconstrained forms normalize away; correlated and infinite complements
produce the existing actionable schema-fallback diagnostic. Each
referenced schema resource retains its dialect: Draft 4 ignores
`propertyNames`, while Draft 6 and newer resources apply it.
Referenced string-capable JSON fields retain exact `minLength`/`maxLength`
intervals measured in Unicode scalar values, including nullable fields, array
items, typed dynamic properties, and compatible compositions. The constraints
remain executable on imported source and target boundaries and round-trip
alongside opaque `format` annotations. Referenced `pattern` assertions use
Ferrule's bounded Unicode-scalar pattern language, survive dialect-aware
references, conjunctive `allOf`, exact disjunctive `anyOf`, and disjoint scalar
`oneOf`, and export canonically. Null bypasses string assertions in nullable
domains; native and generated Rust/C# boundaries apply them identically to
input and normalized output. Nonportable regex constructs reject during schema
import and trigger the component's existing actionable schema-fallback
diagnostic rather than changing meaning between runtimes.
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
