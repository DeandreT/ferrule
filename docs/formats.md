# Supported Formats

ferrule converts every supported input into the shared `ir::Instance` tree and
writes target instances through a separate adapter. Format selection normally
comes from the input or output extension; embedded `FormatOptions` provide the
layout and dialect details that an extension cannot express.

<!-- BEGIN GENERATED formats from docs/reference/formats.toml; edit that file and run `cargo run -p reference-docs` -->
The tables below are generated from
[`docs/reference/formats.toml`](reference/formats.toml). Each claim belongs to
one category:

- **Implemented capability**: behavior of the native format adapters and the CLI
  that uses them, backed by the source and tests cited in the canonical
  reference.
- **Qualified route**: a separately named route, such as a generated library
  adapter or an editor workflow, with its own contract or qualification record.
  It covers only that route and profile.
- **Unsupported profile**: a named shape, direction, or behavior outside the
  implemented scope. The runtime refuses it or, where stated, deliberately does
  not apply it. The list is not exhaustive.

These references describe finite, tested coverage. They are not a parity claim
for a complete format standard or another product.

| Format | Source | Target | Selected by | Implemented capability |
| --- | :---: | :---: | --- | --- |
| XML | Yes | Yes | `.xml` or `xml_document` | Hierarchical instance I/O with namespace-aware element and attribute names; XSD-lite with local import graphs, compatible `complexContent` and scalar-text/attribute-only `simpleContent` derivations, and named model and attribute groups; `xs:any` and `xs:anyAttribute` wildcards with `##any`, `##other`, `##local`, `##targetNamespace`, or listed-namespace constraints under `skip`, declaration-aware `lax`, or closed `strict` processing; bounded DTD import with internal content-model parameter entities; `xsi:nil`, generic elements, and ordered mixed content |
| JSON | Yes | Yes | `.json`; `.jsonl`, `.ndjson`, or `json_lines` for JSON Lines | Hierarchical instance I/O and JSON Lines; JSON Schema import with traversal-confined local-file and document-local references, compatible structural `allOf` intersections, bounded exact scalar `const`/`enum` domains, numeric ranges and decimal `multipleOf`, array-count, `contains`, object-property-count, and Unicode string-length intervals, structural `uniqueItems`, bounded portable `pattern` assertions, closed homogeneous `patternProperties`, property dependencies, dependent-schema predicates, single-property-presence conditionals, `propertyNames`, open or closed object contracts, heterogeneous scalar type arrays, exact scalar `anyOf`, pairwise-disjoint scalar `oneOf`, selected object alternatives, nullable scalar/object/array shapes, and typed or unconstrained dynamic properties |
| JSON5 | Yes | Yes | `.json5` or `json5` | One schema-shaped document in JSON5 instance syntax with ordinary JSON schemas and the same typed mapping boundary; input accepts comments, trailing commas, and single-quoted strings, and is limited to 64 MiB and 128 nested containers; output omits property-name quotes where JSON5 allows; CLI and browser runtime |
| CSV | Yes | Yes | `.csv` or `.txt` | Delimited flat rows with a configurable delimiter, printable or disabled quoting, headers, optional present-empty text, and optional UTF-8 BOM output |
| Fixed-width | Yes | Yes | `fixed_width` layout | Validated Unicode-scalar column layouts, configurable fill, record separators, and empty-value handling |
| XLSX | Yes | Yes | `.xlsx` | Typed worksheets, flat, transposed, composite, worksheet-set, and grid source shapes, flat and hierarchical targets, and update-existing flat writes |
| SQLite | Yes | Yes | `.db`, `.sqlite`, or `.sqlite3` | Read-only table introspection, typed reads, imported relational query shapes, declared read-only single-table queries with typed named parameters, validated declared relations, structured XML text columns, and idempotent full-replace writes |
| X12 / EDIFACT | Yes | Yes | `.edi`, `.x12`, or `.edifact` with a schema dialect, or `edi_kind` | Schema-guided interchange I/O, custom syntax separators, repetitions, qualifier loops, retained field lengths and code lists, optional lenient parsing, and bounded output validation that leaves the destination unchanged on failure |
| HL7 v2 / TRADACOMS | Yes | Yes | `.hl7` or an EDI schema dialect | Bounded schema-guided message I/O, retained field lengths and code lists, HL7 escapes and subcomponents, and TRADACOMS release escaping |
| SAP IDoc | Yes | Yes | `idoc` layout | Embedded fixed-record layouts with bounded byte-position parsing and deterministic output |
| SWIFT MT | Yes | No | `swift_mt` layout | Input through embedded message and field grammars |
| FlexText | Yes | Yes | `flextext` layout imported from `.mft` | Embedded recursive split, store, and switch layouts, including fixed-width and delimited records |
| Protocol Buffers | Yes | Yes | `protobuf` schema and root message | Bounded proto2/proto3 binary I/O with self-contained local import graphs, public imports, nested messages, enums, repeated fields, and packed scalars |
| XBRL | Yes | Yes | `xbrl` options | Typed instance facts, contexts, dimensions, units, and namespace-qualified concepts through table-shaped boundaries |
| PDF | Yes | No | `pdf` extraction options | Layout-driven visual extraction from positioned text, anchors, painted edges, and table regions, typed BasicVisual text capture, and visible inherited CropBox and page-rotation normalization |

## Qualified Routes

| Format | Route | Scope | Record |
| --- | --- | --- | --- |
| XML | Generated Rust and C# XML document APIs | Structured or admitted root-view XML boundaries in the indexed route families | [docs/generated-xml-qualification-index.md](generated-xml-qualification-index.md) |
| XML | Primary filename document runs (`run-document`, `generate --primary-documents`, and editor Run and Preview) | The XML document a literal or runtime-parameter filename names, or the document set a filename wildcard selects, beneath an explicit local input root, mapped once into one XML document | [conformance/primary-xml-documents/README.md](../conformance/primary-xml-documents/README.md) |
| JSON | Ordered JSON source documents in the native and generated C# runtimes | One complete UTF-8 JSON value with ordered and duplicate members, checked against a self-authored corpus | [conformance/json-source/README.md](../conformance/json-source/README.md) |
| JSON5 | Generated Rust and C# JSON5 companions (`generate --json5-adapters`) | One JSON5 object in and strict JSON out, for closed nonrepeating object schemas with scalar leaves and static target construction | [docs/generated-json5-contract.md](generated-json5-contract.md) |
| CSV | Generated Rust and C# complete CSV document adapters (`generate --csv-document-adapters`) | One positional CSV document in and out, including header, BOM, and raw-delimited CRLF output modes | [conformance/csv-documents/README.md](../conformance/csv-documents/README.md) |
| CSV | Primary filename document runs (`run-document`, `generate --primary-documents`, and editor Run and Preview) | Every CSV document a literal or runtime-parameter filename selects beneath an explicit local input root, mapped once into one CSV document | [conformance/primary-filename-documents/README.md](../conformance/primary-filename-documents/README.md) |
| Fixed-width | Primary filename document runs (`run-document`, `generate --primary-documents`, and editor Run and Preview) | Ordered fixed-width documents using saved Unicode-scalar layouts, mapped once with bounded input and output | [conformance/primary-fixed-width-documents/README.md](../conformance/primary-fixed-width-documents/README.md) |
| XLSX | Native editor flat-worksheet setup, save, and reopen | One flat-row XLSX source and target configuration on the Linux desktop | [docs/qualification/flat-workbook-desktop-2026-10-08.md](qualification/flat-workbook-desktop-2026-10-08.md) |
| X12 / EDIFACT | Generated C# X12 companions (`generate --x12-adapters`) | Raw X12 text and bytes with single, grouped, or MapForce-shaped envelope profiles around the ordinary typed mapping | [docs/design/generated-x12-csharp.md](design/generated-x12-csharp.md) |
| X12 / EDIFACT | Primary filename document runs (`run-document`, `generate --primary-documents`, and editor Run and Preview) | Explicit X12 or EDIFACT direct documents or wildcard document sets, parsed in order and mapped once into one same-family output | [conformance/primary-edi-documents/README.md](../conformance/primary-edi-documents/README.md) |
| X12 / EDIFACT | Generated C# static EDIFACT document adapters | Primary and named static inputs and outputs, with strict interchange validation and syntax-v4 repetitions | [docs/design/static-edifact-documents.md](design/static-edifact-documents.md) |
| HL7 v2 / TRADACOMS | Primary filename document runs (`run-document`, `generate --primary-documents`, and editor Run and Preview) | Explicit HL7 or TRADACOMS direct documents or wildcard document sets, parsed in order and mapped once into one same-family output | [conformance/primary-edi-documents/README.md](../conformance/primary-edi-documents/README.md) |
| SAP IDoc | Primary filename document runs (`run-document`, `generate --primary-documents`, and editor Run and Preview) | IDoc documents using embedded layouts, parsed in order and mapped once into one bounded IDoc output | [conformance/primary-edi-documents/README.md](../conformance/primary-edi-documents/README.md) |

## Unsupported Profiles

| Format | Profile |
| --- | --- |
| XML | XSD 1.1 wildcard exclusions (`notNamespace`, `notQName`) and `xs:any` inside `xs:all` |
| XML | Strict wildcards whose matching declaration set is not closed, same-local-name strict declarations with incompatible typed shapes, strict wildcards that follow a single-occurrence declaration of a name they match, and strict wildcards outside a choice whose port shares a local name with a declaration covering only some of its namespaces |
| XML | Element wildcard occurrences other than `1..1`, `0..1`, or `0..unbounded`, and optional single-occurrence `skip` wildcards |
| XML | Sibling elements or attributes that differ only by namespace, because mapping paths use local field names |
| XML | External DTD subsets and external entities, which are never loaded |
| XML | Output to a local XML file set or an HTTP URL (`local_xml_file_set` and HTTP sources are input-only) |
| JSON | General heterogeneous array composition, heterogeneous or correlated numeric-range scalar unions, and mixed structural unions |
| JSON | Heterogeneous `prefixItems` or legacy tuple members and active `unevaluatedItems` |
| JSON | Required-only schemas without an object shape |
| JSON | External references from in-memory schema import (`json_schema::import_str`) |
| JSON | Enforcement of other shape-neutral validation keywords, which are accepted only for schema recovery |
| JSON5 | Combining JSON5 with JSON Lines |
| JSON5 | Nonfinite numbers such as `Infinity` and `NaN` |
| JSON5 | Comment preservation; comments are not mapped or written back |
| JSON5 | JSON5 in ordinary generated Rust and C# entry points, which exchange strict JSON |
| JSON5 | Native `.mfd` export of JSON5 boundaries |
| CSV | Unquoted output of field text that requires escaping when quoting is disabled |
| Fixed-width | Combining a fixed-width layout with CSV dialect options |
| XLSX | Grid, worksheet-set, composite, and transposed output, which are input-only layouts |
| XLSX | Update-existing writes for hierarchical output |
| SQLite | Other database engines, general SQL mutation, and multi-database connections |
| SQLite | SQL text, joins, views and declared queries on payload, stream, REST, pipeline, generated-code and MFD routes |
| X12 / EDIFACT | X12 CR LF segment terminators, and whitespace or invisible X12 element, component, repetition or release separators |
| SAP IDoc | `.idoc` input without an embedded layout |
| SWIFT MT | SWIFT MT output |
| Protocol Buffers | Recursive messages, which the tree-shaped instance model cannot represent |
| XBRL | Taxonomy formula, presentation, calculation, and linkbase execution |
| PDF | PDF output |
<!-- END GENERATED formats -->

## Important Boundaries

- MFD import constructs a required, non-null singleton XML scalar document root
  from one matching String, Bool, Int or Float constant, or one required XML
  element path whose root and ancestors are singular. The selected scalar
  declaration uses qualified `xs:string`, `xs:boolean`, `xs:long` or `xs:double`.
  Its own XSD must prove default or explicit 1..1 occurrences through each
  selected element and containing sequence or all particle. Constants must have
  valid saved lexical values; Float constants must be finite. Primary and named
  targets retain their own schemas and output identities. Repeated, optional,
  nullable, competing and metadata-dependent root feeds, custom scalar facets,
  lists, unions, attributes and simple-content paths produce repair warnings
  and refuse executable import. XML instance readers and writers retain their
  existing typed errors, including nonfinite output refusal. Singleton scalar
  root MFD export remains unsupported and refuses before publishing artifacts.
- JSON5 input accepts comments, trailing commas, single-quoted strings, and
  other supported JSON5 syntax; comments are not represented in mapping values
  or written back. Output omits quotes around property names when JSON5 allows
  it. JSON5 uses one document, so `json5` and `json_lines` cannot be combined.
  Input is limited to 64 MiB and 128 nested containers; exact `uniqueItems`
  checks retain numeric token values, including hexadecimal numbers. Nonfinite
  values are rejected. JSON5 file parsing and writing are provided by the CLI
  and browser runtime. Ordinary generated Rust and C# library entry points
  exchange strict schema-shaped JSON with their hosts; the explicit
  `--json5-adapters` profile adds singular JSON5-input, strict-JSON-output
  companions for closed object schemas, as described in the
   [generated JSON5 companion contract](generated-json5-contract.md). MFD import
   retains observed JSON5 metadata separately and refuses execution/export: the
   vendor setting, filename selection and numeric behavior differ from these
   codecs. See the [interchange observation](conformance/json5-interchange.md).
- PDF targets are not supported.
- XBRL taxonomy formula, presentation, calculation, and linkbase execution are
  outside the current runtime.
- XML preserves declared expanded-name identity for elements and attributes.
  Scalar, simple-content, and retained mixed-content text preserve carriage
  returns through XML character references; attributes and generated XSD
  fixed/default values also preserve tabs and line breaks.
  Generic elements matched by a wildcard keep every text node between their
  child elements, including whitespace, and the writer adds no indentation
  inside them. In a generic element without child elements, whitespace-only
  text containing a line break reads as empty text.
  Declared mixed content (XSD `mixed`, DTD mixed `#PCDATA`, and DTD `ANY`)
  likewise keeps its text exactly, and the writer adds no indentation inside a
  mixed element, including inside its element-only children, so the output
  reads back unchanged. Element-only content outside mixed elements stays
  indented.
  MFD export partitions foreign declarations into deterministic local XSD
  siblings and publishes the complete graph atomically. Bounded local-graph
  substitution groups, compatible element-only or mixed `complexContent`
  extension/restriction `xsi:type` hierarchies,
  scalar-text/attribute-only `simpleContent` derivations,
  optional/unbounded named model groups with exactly one nonrepeating member,
  namespace-constrained optional/unbounded `processContents="skip"` element
  wildcards declared inline or through named model groups, declaration-aware
  lax wildcards inside sequences or repeating choices, closed strict wildcards
  resolved to exact singular or repeating typed choices, and namespace-aware
  attribute wildcards are supported. Lax processing gives resolved declarations
  typed mapping fields and reserves the generic fallback for undeclared names.
  An extra occurrence of a single-occurrence declared element is a generic
  item after the typed field takes the first one; reads refuse extras the
  wildcard cannot keep, such as names outside its namespace constraint. A
  wildcard's typed port for a declaration the type already declares yields to
  that declaration for the namespaces it covers. A strict wildcard after a
  single-occurrence declaration of the same name is refused, because it has
  no generic fallback for that declaration's extra occurrences.
  Strict attribute processing rejects undeclared runtime names. XSD 1.1
  exclusions, unordered wildcard compositors, and unresolved strict declaration
  sets remain outside the subset.
  XSD import keeps at most 256 nesting and reference levels open at once.
  Each element declaration or reference, named complex type reference
  (including a derivation base), complex type content, compositor, and model
  or attribute group reference takes one level, so about 64 nested elements of
  named types, 85 of anonymous types, or 128 derivation or group hops fit.
  Deeper schemas return a typed depth-limit error before stack exhaustion; MFD
  import then falls back to the component's entry tree with a warning.
  Following `xs:include` and `xs:import` links from one schema file to the
  next takes no level and no stack: the documents are visited depth first in
  document order from a heap worklist, and a document already visited is not
  expanded again, so cycles end. A filesystem import may therefore follow a
  chain of any length, and each file in it costs only its bounded read.
  Imports through an `XmlSchemaByteReader`, including MFD packages, read at
  most 64 distinct documents and make at most 4,096 reads.
  Because mapping paths use local field names, strict wildcard declarations
  with the same local name collapse onto one port only when their complete
  typed shapes match. The port retains every exact expanded name and ordered
  runtime occurrences retain the selected namespace. Incompatible same-local
  declarations reject explicitly.
- JSON Schema supports compatible structural `allOf` intersections across objects,
  scalar domains, and matching arrays, selected object alternatives, exact nullable
  scalar/object/array wrappers and flat compatible multi-branch nullable
  compositions, heterogeneous scalar type arrays, exact scalar
  `anyOf` unions, pairwise-disjoint scalar `oneOf`, and identical or
  scalar-domain-subsumed array `anyOf` branches, including local references.
  Admitted scalar `anyOf` and `oneOf` compositions may carry a redundant
  `type` sibling. The declaration must be a standard type name or a nonempty
  array of distinct standard names, and must contain the complete retained
  scalar domain, including JSON null. `number` contains integer values;
  `integer` does not contain an unbounded finite-number domain. Finite
  `const`/`enum` compositions prove containment after exclusive-branch
  filtering, using mathematical JSON integer semantics for integral values.
  Broader type declarations do not widen the branch domain. Nullable scalar
  wrappers prove containment after null-overlap normalization. Restrictive
  intersections and type siblings on structured nullable or modern `$ref`
  compositions retain their existing refusal contracts.
  File and in-memory imports share a 64-hop reference-depth limit, a 64-edge
  schema-materialization nesting limit, and a 100,000-step parsing budget.
  Excess returns a typed resource-limit error before stack exhaustion. The
  main parser resolves flat references iteratively, so those hops do not
  consume materialization depth. In-memory
  `format_json::json_schema::import_str` resolves document-local references
  without opening files and rejects external resources.
  A cyclic or unresolved reference inside a schema degrades to a string field.
  A root whose whole `$ref` chain is cyclic or unresolved, and a schema
  position holding a JSON value other than an object or boolean, instead return
  a typed `ContentlessSchema` error; non-UTF-8 schema files return a typed
  `SchemaEncoding` error naming the file. MFD import then falls back to the
  component's entry tree with a warning. The `true` and `{}` schemas still
  import as an arbitrary JSON value.
  Object-shaped `required` declarations preserve property presence independently
  from value nullability, including declared names on closed objects and named
  runtime properties on open objects. Required-only schemas without an object
  shape remain outside the subset because they also admit every non-object value.
  Objects with omitted or `true` `additionalProperties` retain an unconstrained
  dynamic field, while a schema-valued declaration retains that exact dynamic
  value shape. Explicit `false` remains closed: native and generated Rust/C#
  input boundaries reject undeclared properties instead of silently dropping
  them. Canonical export writes `{}` for an unconstrained open object and
  `false` for a closed object, so MFD export/re-import preserves the behavior.
  Compatible `allOf` object branches intersect their declared and dynamic
  property permissions rather than widening a closed branch.
  A nonempty `patternProperties` map is retained exactly for an explicitly
  typed `object` or `object | null` with `additionalProperties: false` when
  every bounded portable selector has one identical exactly representable
  value schema. Scalar, structured object, and homogeneous array values reuse
  the ordinary supported JSON value profile. Selectors are ORed and retain
  declaration order. A fixed property matched by any selector must have that
  same schema; nonmatching fixed properties may differ. Runtime decoding
  checks fixed properties first, then admits a remaining name when any
  selector matches and validates its value against the common schema.
  `propertyNames` remains an independent constraint on every key. Dependency
  rules whose triggers are neither fixed nor selected are semantically
  unreachable and normalize away. Nullable object null bypasses the object
  checks. Native, generated Rust, and generated C# input, normalized output,
  and JSON Lines boundaries share the portable matcher and one per-document
  pattern work budget. Canonical export writes the selectors and
  `additionalProperties: false`; re-import preserves the contract. An empty
  `patternProperties` object is a no-op.
  Concrete objects retain exact non-negative `minProperties` and
  `maxProperties` intervals through references, nullable wrappers, compatible
  `allOf` intersections, and object alternatives that share one identical
  effective interval. Input counts distinct parsed properties before
  required-property and openness validation, so explicit JSON null and
  undeclared names both count; duplicate names use the parser's last value and
  count once. Output counts the normalized object after Ferrule `Null` fields
  are omitted. A nullable object value of JSON null bypasses the object
  interval; every JSON Lines row is checked independently. A maximum below the
  required property count, a closed-object minimum beyond its declared
  capacity, and alternatives with differing correlated intervals reject rather
  than widen.
  Object property dependencies are retained as bounded trigger-to-required-name
  relations. A present trigger requires every named dependent property:
  explicit JSON null counts as present on input, while output checks the
  normalized object after omitted Ferrule `Null` fields are removed. Nullable
  object null bypasses the relation, compatible `allOf` branches unite their
  rules, and object alternatives must produce one identical effective relation
  rather than correlating different dependencies with different branches.
  Modern `dependentRequired` imports directly. Legacy property-array
  `dependencies` normalizes to the same model and canonical export writes
  `dependentRequired`. Metadata is limited to 256 triggers, 4,096 dependency
  edges, and 256 KiB of property-name text per object. Unconditional
  required-property closure must remain possible under the object's closed
  shape and `maxProperties` interval.
  Schema-valued legacy `dependencies` and modern `dependentSchemas` apply a
  schema to the complete containing object when their trigger property is
  present. The containing schema must have a concrete object shape; a typeless
  schema that also admits non-object values rejects rather than narrowing those
  values away. Required-only predicates lower to the property-dependency model.
  Other predicates retain the currently executable scalar, object, and array
  JSON constraints, including nested ordinary schemas and bounded patterns.
  Input uses raw property presence, so explicit null activates a rule; output
  uses the normalized object after Ferrule `Null` omission. Repeated entries
  for one trigger are conjunctive. Canonical export groups contiguous entries
  for that trigger with an inner `allOf`; when a trigger reappears after another
  trigger, ordered outer `allOf` branches preserve the complete retained rule
  order through re-import. Compatible outer `allOf` branches append rules in
  declaration order, while alternatives must have one identical effective rule
  set. Each object retains at most 32 nontrivial predicates and 256 KiB of
  trigger-name text.
  Nullable object null bypasses the rules, and JSON Lines checks every object
  row independently. Predicate patterns share the document matcher budget.
  Nested dependent schemas are supported recursively, with the same per-object
  bounds. Draft 7, 2019-09, 2020-12, and schemas without `$schema` also support
  an exact conditional on an explicitly typed object schema whose `if` tests
  the presence of exactly one required property and whose `then` fits the
  supported dependent-schema predicate subset. A nullable outer object is
  supported with an absent or `true` `else`
  only when the `if` explicitly proves `type: "object"`. Exact `else: false`
  requires a trigger that can be represented as an ordinary required field; it
  removes the nullable bypass when the false branch rejects null and retains
  any supported `then` dependency. It does not combine with existing object
  alternatives, and a closed object must already declare the trigger.
  Canonical export writes `required` and, when `then` retains a dependency,
  `dependentRequired` or `dependentSchemas` as appropriate. Value-sensitive,
  multi-trigger, general-`if`, and other nontrivial `else` schemas remain unsupported.
  `unevaluatedProperties`, `unevaluatedItems`, and heterogeneous positional
  arrays likewise reject instead of being approximated. A lone
  validation-neutral conditional keyword is ignored.
  Declared Draft 4/6/7 resources accept schema-valued legacy `dependencies`
  and ignore modern `dependentSchemas`/`dependentRequired`; Draft 4 requires
  those schema values to use its object form, while Draft 6/7 also accept
  boolean schemas. Declared
  2019-09/2020-12 resources accept modern dependency keywords and ignore
  legacy `dependencies`. A resource without `$schema` intentionally uses a
  compatibility dialect: modern reference-sibling and structural behavior
  applies, while both legacy and modern dependency spellings are accepted.
  Object `propertyNames` constraints apply to every actual property name,
  including declared fields, runtime-named fields, and the empty string. Input
  checks raw parsed keys before object decoding; output checks the normalized
  emitted key set after absent Ferrule values are omitted. Exact `false`
  rejects every nonempty object, while `true` and an unconstrained schema
  normalize away. Nullable object null bypasses name assertions, and each JSON
  Lines object row is checked independently. Supported string assertions are
  finite `const`/`enum` name sets, their exact finite complements through
  `not`, `minLength`/`maxLength` measured in Unicode scalar values, bounded
  portable `pattern` conjunctions/disjunctions and their exact complements,
  and ordered `format` annotations retained without vocabulary assertion.
  Finite complements intersect by excluding the union of their sets;
  compatible `anyOf` branches exclude only names common to every branch, and
  a double complement restores the positive finite or pattern predicate.
  Correlated predicates and complements involving length, format, or mixed
  assertions reject instead of being widened. Draft 4
  resources ignore `propertyNames`; Draft 6 and newer resources apply it.
  Finite name domains are limited to 4,096 names, 256 KiB per name, and 1 MiB
  total, while name patterns share the document's bounded matcher budget.
  Array `contains` assertions retain a bounded conjunction of schema-shaped
  item predicates and exact match-count intervals. Plain `contains` defaults
  to one or more matches; Draft 2019-09 and newer `minContains`/`maxContains`
  set explicit bounds. Raw parsed members and normalized emitted members are
  counted, nullable array null bypasses the assertions, and predicate patterns
  share the document matcher budget. Compatible `allOf` branches append their
  assertions. Array alternatives retain only identical assertions or an exact
  containment relation rather than weakening correlated predicates and
  intervals. Draft 4 ignores `contains`; Draft 6/7 apply the default minimum
  and ignore the newer modifiers.
  Bounded scalar `const` and `enum` constraints are enforced exactly on both
  input and normalized output and survive canonical export. Sets may combine
  strings, booleans, signed integers, exactly representable finite numbers, and
  JSON null. References and compatible `allOf` intersect their values; finite
  scalar `anyOf` takes their union, while `oneOf` retains values admitted by
  exactly one branch. Structured object and array enum members remain outside
  the scalar mapping model.
  Ordinary `minimum`, `maximum`, `exclusiveMinimum`, and `exclusiveMaximum`
  constraints are likewise enforced for concrete integer and finite-number
  scalars, including nullable scalar wrappers. Integer constraints normalize to
  one exact inclusive `i64` interval; number constraints retain inclusive or
  exclusive finite endpoints and reject intervals containing no representable
  finite value. Signed 64-bit integer JSON tokens on `number` boundaries
  normalize to the nearest binary64 endpoint with the inclusion flag that
  preserves the exact original inequality over Float values. This includes
  endpoints beyond binary64's contiguous integer precision and both signed
  extremes; an exact endpoint retains its original inclusion flag. Instance
  and constant precision checks remain separate from boundary normalization.
  At actual schema positions, native import preserves exact integral decimal
  and exponent spellings of the four range bounds as signed or unsigned JSON
  integer tokens, including negative zero. This applies to inline text, file
  roots, and confined local reference resources. Fractional or out-of-token-domain
  endpoints retain their existing admission rules; a rounded near-integer is
  never treated as an exact integral spelling. Opaque annotations and data,
  `multipleOf`, runtime instance numbers, duplicate-key behavior, dialect policy,
  and schema resource limits are unchanged. Native and generated Rust/C# adapters enforce the resulting exact bounds. Import accepts both modern numeric
  exclusive bounds and Draft 4
  boolean exclusives regardless of the declared dialect for interoperability;
  export emits the canonical normalized form. Positive finite `multipleOf`
  divisors are retained as canonical decimal coefficients and exponents, with
  no floating-point tolerance. Compatible `allOf` branches form conjunctions
  and exact `anyOf` branches form disjunctions. Contiguous same-type numeric
  range branches normalize to one exact interval when their divisor constraints
  are identical. When an `anyOf` varies both its numeric range and divisor
  constraint, Ferrule rejects the correlated union rather than independently
  widening either axis. An exact String-or-Int union can retain one integer
  interval. An exact String-or-Float union can retain one finite-number interval
  when its String branch is unconstrained. Numeric input and output enforce the
  interval after the ordinary Float precision/coercion checks; text retains its
  String tag even when it looks numeric. Nullable forms additionally admit
  explicit JSON null. Canonical type-array export and re-import preserve these
  domains and bounds.
  Scalar `anyOf` and `oneOf` apply outer numeric bounds to the represented
  concrete numeric or exact String-or-Int/String-or-Float domain. Concrete
  intervals and String-or-Int intervals retain exact compatible intersections.
  A branch-bounded String-or-Float interval composition refuses additional outer
  numeric, length, pattern, multiple-of, const, or enum assertions. Its direct
  type-array form admits one interval without those other assertions. Redundant
  outer type containment and nonasserting annotations remain separate. This
  String-or-Float range composition requires exactly one unconstrained String
  branch and one bounded Number branch. Wider range-bearing heterogeneous
  domains and correlated range predicates remain outside this subset.
  Homogeneous numeric interval unions retain their separate compatible-union
  behavior.
  Concrete arrays retain exact non-negative `minItems` and `maxItems`
  intervals through references, nullable wrappers, compatible `allOf`
  intersections, and exactly representable `anyOf` unions. Input and output
  enforce the interval before visiting array items. JSON Lines applies a root
  array interval to the total nonblank line count; nullable root arrays reject
  because line-oriented null-container semantics are ambiguous. Disjoint count
  unions reject rather than widen. Nested schema-valued arrays retain each
  level in an explicitly marked repeating group whose sole repeating child is
  `item`. Each outer item is a group containing the complete inner array;
  empty outer arrays and arrays containing empty inner arrays remain distinct.
  Item-count, uniqueness, supported `contains`, and container nullability apply
  to their own level. Array-item nullability is separate from outer-container
  nullability. Canonical JSON Schema export and `.mfd` array/item ports retain
  those levels and declared paths. An ordinary object property named `item`
  does not acquire the wrapper role. XML, CSV, and EDI boundaries reject the
  JSON wrapper role. Arrays of objects also keep the array's nullability
  independent from the object's nullability: a nullable array of nonnullable
  objects accepts `null` and `[{}]` but rejects `[null]`; a nonnullable array
  of nullable objects accepts `[null]` and rejects `null`. The optional
  `json_object_item_nullable` schema field carries the item setting on genuine
  repeated object groups. When absent, existing schemas retain their legacy
  item behavior. Arrays of scalars keep the array's nullability separate from
  the items' own `nullable` domain: a nullable array of nonnullable integers
  accepts `null` and `[1]` but rejects `[null]` and `[1,null]`. Positional
  nested arrays and unsupported structured predicates remain outside this
  subset. `$ref` siblings follow the dialect declared by their physical schema
  resource: Draft 4, 6, and 7 ignore them, while Draft 2019-09, 2020-12, and
  schemas without `$schema` apply Ferrule's supported numeric, `multipleOf`,
  item-count, string-length, and annotation metadata. Concrete string-capable scalar
  domains retain exact non-negative `minLength` and `maxLength` intervals.
  Ferrule supports an exact bounded homogeneous `prefixItems` subset for Draft
  2020-12 and schemas without `$schema`.
  Every entry in the bounded finite prefix must normalize to one identical,
  non-repeating item shape. The tail must use that same shape, be `false`
  (which derives an exact `maxItems` at the prefix length), or be provably
  unreachable under an existing maximum; an unconstrained tail is also exact
  when the common prefix shape accepts arbitrary JSON. Conflicting item-count
  bounds, heterogeneous entries or tails, concrete nested-array entries, and
  active `unevaluatedItems` reject rather than widen. Canonical export writes
  the equivalent ordinary `items` schema plus
  `maxItems` when the tail is closed, so no tuple-specific runtime or mapping
  IR is required. Draft 4 through Draft 2019-09 treat
  `prefixItems` as an unknown keyword according to their dialects.
  The complementary bounded homogeneous legacy tuple profile accepts
  array-valued `items` in Draft 4, 6, 7, and 2019-09 resources and in schemas
  without `$schema`. Each tuple must contain 1 to 4,096 positional members,
  all of which normalize to one identical, non-repeating item shape. The
  `additionalItems` tail must use that same shape, be `false` so the tuple
  length becomes `maxItems`, or be provably unreachable under an existing
  maximum. An absent or `true` tail is exact
  only when the common item shape accepts arbitrary JSON or the explicit
  maximum makes the tail unreachable. Explicit `minItems`/`maxItems` and the
  derived closed-tail maximum are intersected, so contradictory bounds reject.
  Canonical export writes ordinary schema-valued `items` plus `maxItems` when
  closed; no positional-array runtime or mapping IR is retained. Draft 2020-12
  requires schema-valued `items`, so array-valued `items` are rejected there
  rather than being interpreted as `prefixItems`. Heterogeneous members, reachable
  heterogeneous tails, and concrete nested-array members remain unsupported.
  Concrete array wrappers also retain `uniqueItems: true` through supported
  references and compatible composition. Native and generated Rust/C#
  boundaries compare complete raw input values and normalized output values:
  object member order is irrelevant, nested array order is significant, and
  mathematically equal JSON numbers are duplicates before typed input
  normalization. Inside private `contains` and `dependentSchemas` predicates,
  `uniqueItems` rejects item shapes that admit non-integral `number` values or
  arbitrary JSON, including through nested fields, until predicate matching
  can retain raw numeric lexemes. Closed integer, string, boolean, and object
  item shapes remain supported.
  Ferrule measures them in Unicode scalar values, applies them only when a
  scalar union's runtime value is a string, and enforces them on native and
  generated input and output boundaries. They survive nullable wrappers,
  references, compatible `allOf`, contiguous exact `anyOf` unions, array-item
  projection, typed dynamic properties, and canonical export. Disjoint length
  unions and ambiguous untyped assertions reject rather than move or widen an
  assertion.
  Concrete string-capable fields and scalar unions also retain JSON Schema
  `pattern` assertions. Null bypasses a pattern in nullable domains, while
  string values in array items and typed dynamic properties are checked.
  Compatible `allOf` branches form a conjunction, exact `anyOf` branches form
  a disjunction of conjunctions, and `oneOf` remains limited to provably
  disjoint scalar branches. References and `$ref` siblings follow the same
  per-resource dialect policy as the other supported assertions. Canonical
  export uses direct `pattern`, `allOf`, or `anyOf` shapes as needed.
  Native and generated Rust/C# boundaries enforce the same portable,
  Unicode-scalar matcher on both input and normalized output.
  The portable syntax includes unanchored matching, `^` and `$`, dot,
  positive and complemented scalar classes and ranges, alternation,
  capturing or noncapturing groups, `*`, `+`, `?`, bounded or open counted
  repetition, lazy quantifier suffixes, and explicit character/Unicode
  escapes. Backreferences, lookaround, named groups, inline flags, Unicode
  properties, shorthand classes such as `\d`, octal/control escapes, and
  class-set operators reject rather than acquire backend-specific semantics.
  Distinct per-selector `patternProperties` value schemas, open or typed
  `additionalProperties` fallbacks, general overlap intersection, value shapes
  outside the ordinary exact JSON profile, and pattern-property objects under
  active `allOf`, object alternatives, or structural `$ref` siblings remain
  outside the exact homogeneous profile. `unevaluatedProperties` also remains
  unsupported. These forms reject rather than silently weakening an otherwise
  typed object.
  Each source is limited to 64 KiB, 256 nesting levels, 8,192 syntax nodes,
  and 16,384 compiled instructions. Each constrained node retains at most 32
  alternatives and 64 total terms; a complete schema retains at most 64
  distinct sources, 256 KiB of distinct source text, and 65,536 distinct
  compiled instructions. Each JSON document parse or serialization call shares
  one deterministic 100-million-unit pattern work budget across that document.
  Unknown and empty string `format` annotations are
  retained exactly on string-capable values and array items, accumulated in
  order through compatible `allOf`, references, and exact scalar/array unions,
  and exported without turning them into assertions. Ferrule does not validate
  values against named format vocabularies. Annotations on non-string values
  and arbitrary-JSON nodes are syntactically validated but not retained. Each
  node retains at most 64 distinct annotations, each at most 1 KiB and at most
  16 KiB in total.
  Unsupported modern structural intersections reject explicitly instead of
  widening silently; external resources select their own policy.
  Export emits the canonical normalized constraint form.
  General heterogeneous array composition, heterogeneous or correlated
  numeric-range scalar unions,
  and mixed structural unions remain unsupported.
  Other shape-neutral validation keywords are accepted for schema recovery but
  are not enforced by the mapping runtime.
- Database execution is SQLite-specific and does not yet provide a general SQL
  mutation or multi-database connector model.
- Complex XLSX, PDF, EDI, and FlexText layouts depend on an embedded validated
  configuration; unsupported imported commands remain explicit warnings.
- EDI output validates every present configured field after wire lexical
  formatting. The bounded report includes all length and code-list violations
  it finds, and validation failures do not replace the destination.

The [workflow-parity roadmap](../ROADMAP.md) tracks the remaining format and
connector work.

### SQLite metadata inspection

Schema introspection, relational-schema validation, and foreign-key metadata
resolution open existing SQLite files with explicit read-only connection flags.
They do not create a missing main database or retry with a writable connection.
A hot rollback journal therefore causes the metadata operation to fail instead
of repairing database pages or deleting that journal. MFD import retains its
existing embedded-type and untyped fallback rules when inspection fails.

The metadata connection treats its path as a filesystem filename, without
enabling SQLite URI options or immutable mode. This connection policy does not
provide complete filesystem isolation: [SQLite WAL readers](https://www.sqlite.org/wal.html#read_only_databases)
can require or create shared-memory and WAL sidecars. Hosts that require an
offline filesystem boundary must also confine readable resources and enforce
read-only access to their input filesystem.

### Declared SQLite queries

`source_options.sqlite_read`, or a static named source's `options.sqlite_read`,
selects the closed `sqlite_read_v1` profile described in the
[declared read design](design/sqlite-read-query-v1.md). It reads one ordinary
rowid table in `main` into a repeating group of plain String, Int, Float or Bool
fields through an ordered projection, a conjunction of `eq`, `ne`, `lt`, `le`,
`gt` and `ge` comparisons with typed named parameters or explicit NULL tests,
and an order that ends with the proved `INTEGER PRIMARY KEY` ascending. The
descriptor carries no SQL text. It cannot be combined with other format options
and never falls back to an ordinary table read. Existing table reads and
imported query lowering are unchanged. When a full-replace write creates a
missing table, it declares a sole generated column `INTEGER PRIMARY KEY` and
Bool columns `BOOLEAN`, so the table's `import-db` schema can carry a query
without edits.

Each query parameter binds one required runtime parameter of exactly its
declared type, supplied with `--param NAME=VALUE` or as a typed host value and
resolved with the same coercion as graph evaluation. Duplicate and unknown
bindings, then missing, NULL, wrong-type and non-finite values, refuse before
any input is opened. String comparison and ordering use BINARY collation. SQL
NULL becomes a present `Null` field, and zero rows produce an empty sequence.

Acquisition opens the existing literal path read-only with a private cache,
`query_only`, an untrusted schema and an authorizer limited to the admitted
table columns. It admits table and key metadata and validates every referenced
cell in key order within one read snapshot before running the SELECT, so a
filter cannot hide malformed storage. A busy database refuses immediately.
Cancellation, a 5-second deadline, and limits of 100,000 candidate and output
rows, 1 MiB per cell, 64 MiB of decoded payload, 64 KiB of generated SQL and
about 10 million VM instructions are cooperative and refuse rather than
truncate. The connection closes before mapping evaluation. Failures are typed
`format_db::query::QueryReadError` values carrying the phase, parameter,
column, one-based row and SQLite codes, never parameter values.

Filesystem `run` and `cli::preview_project_value_from_files` support primary
and static named query inputs, and restricted policies confine the database
path. Payload, standard-stream, REST JSON, pipeline, offline bundle,
generated-code and MFD export routes refuse the profile before reading. Saved
designs that carry it require the [`sqlite_read_v1` file
capability](project-files.md#file-capabilities).

### Native X12 ISA encoding

Native X12 output encodes a complete 16-position ISA as fixed-width ASCII.
ISA02 and ISA04 are right-padded with spaces to 10 bytes; ISA06 and ISA08 are
right-padded to 15 bytes. Already padded values retain their text. The remaining
fields require their exact byte widths, including the caller's nine-byte ISA13
control number. Every field requires printable ASCII and excludes reserved
delimiter and release characters, except the designated ISA11 and ISA16 syntax
positions. The complete header occupies 106 bytes through its terminator.

Encoding applies after configured autocomplete has performed its existing
lexical normalization and completion. Without autocomplete, date, time and
control values retain their supplied spelling. Incomplete ISA schemas retain
positional serialization. Invalid complete ISA data returns its typed boundary
error before the public file writer replaces its destination.

### Native X12 segment terminators

Native input discovers each interchange's segment terminator from the
character after ISA16, so LF and CR terminators read like any other. Whitespace
between segments is ignored, so CR LF input reads as a CR terminator.

Explicit output separators may select LF or CR as the segment terminator. A
visible terminator is followed by a line feed, giving one segment per line; an
LF or CR terminator is written alone. An interchange read with LF or CR
terminators therefore writes back byte for byte with the same terminator
selected. A two-character CR LF terminator cannot be selected, and a
CR-terminated boundary writes CR alone. Element, component, repetition and
release separators must remain visible, non-alphanumeric characters.

Data containing the selected terminator follows the same rule as the other
separators: without a release character the writer refuses it before the
destination is replaced. The fixed-width ISA always refuses it because every
ISA field requires printable ASCII.

In `.mfd` designs, an LF or CR terminator is stored as `%0A` or `%0D` with
`terminatewithlinefeed="false"`. Import refuses a line-break terminator with
`terminatewithlinefeed="true"`, because that requests CR LF or a blank line
after every segment, and keeps the standard separators with a warning.
Generated Rust and C# X12 profiles accept LF but not CR.

### CSV empty text

`FormatOptions.csv_preserve_empty_strings` defaults to false. When true,
physically present empty string cells survive as empty text; missing trailing
columns and empty numeric or boolean cells remain `Null`. File, payload, and
browser execution share this policy. The low-level `CsvReadOptions` and
`read_with_options`/`from_str_with_options` APIs expose the same setting; legacy
CSV entry points retain the default. Selecting another format with the option
set rejects explicitly.

`FormatOptions.csv_utf8_bom` defaults to false. When true, every CSV output
starts with the UTF-8 bytes `EF BB BF`, including a headerless output with no
rows. Primary and named targets select the setting independently. File,
payload, and browser output share `CsvWriteOptions` and the
`write_with_options`/`to_string_with_options` APIs; legacy writer entry points
omit the BOM. Readers accept UTF-8 with or without a BOM. CSV target setup
exposes “UTF-8 byte order mark.” Selecting another format with this option
rejects before output publication.

The CSV setup form selects double quotes, a custom printable ASCII character,
or disabled quoting independently for source and target. Source previews use
the selected dialect. If the first preview exceeds its width or byte limit,
the form retains the file so you can correct its delimiter or quote settings;
creation stays blocked until a bounded preview succeeds. Missing or unreadable
file selections preserve the previous source draft. Saved CSV format identity
also supports extensionless and unfamiliar data filenames. Recognized non-CSV
filename extensions still reject during setup, matching runtime dispatch.

Named CSV inputs and outputs have the same delimiter, quote, and header
controls. Input empty-text handling and output BOM settings are independent.
Invalid pending edits block Add/Save while retaining the saved boundary; they
can be abandoned. Opening the editor preserves existing defaults and native
repair dependencies.

Generated C# [CSV input to JSON companions](code-generation.md#flat-csv-input-to-json)
use the same flat positional CSV input contract with explicit source format
identity. They parse all rows before one ordinary selected-target execution,
then write one strict JSON primary or named document using that owner's
schema. Repeated nested output scopes require an explicit graph iteration;
CSV absence remains Null rather than becoming JSON null. A scalar lookup
without iteration retains ordinary Ferrule first-row or empty-collection
semantics. These scope rules describe Ferrule's own execution contract.

The new-mapping form offers fixed-width setup after importing a flat XSD or
JSON Schema for either side. Source and target retain independent layouts and
optional data paths; configuration does not open the data file. Named inputs
and outputs with flat scalar schemas also offer fixed-width setup. Named inputs
require their own data path. Widths count Unicode
characters, and each layout retains its fill character and fill-only input
policy. Delimited records write LF and accept LF/CRLF on input;
contiguous records have no line endings. Pending changes leave the existing
format untouched until Add/Save, and can be abandoned. Saved layouts survive
name and path edits, including filenames with other format extensions.
Replacing their ordered schema requires explicitly changing format first.

### Creating FlexText mappings

In the native editor's new mapping form, choose a `.mft` layout for either
side through “Choose schema or layout.” Source and target keep independent
layouts, including their delimiters, output line endings, and UTF-8 BOM
settings. Optional data paths start empty; importing a layout does not open
its configured data file. The editor derives the visible schema once and
keeps the layout in the saved project, so runs do not need the original `.mft`.
Named input and output schema choosers also accept `.mft`. Each boundary keeps
its own layout through undo/redo and save/reopen after the configuration is
removed. An input requires its own data path; an output path may remain empty.
Pending layout changes remain separate from the saved boundary until Add/Save.
An ordinary schema replacement must exactly match the embedded layout's schema,
or you must explicitly change format first. Abandoning a pending source layout
after changing a SQLite path or table requires a new table schema.

Failed configuration imports leave both staged boundaries and the open mapping
intact. The shared configuration reader stops after the 4 MiB limit plus one
byte, before decoding or parsing oversized files.

### Creating Protocol Buffers mappings

In the native editor's new mapping form, choose a `.proto` schema for either
side and select its root message. Source and target roots are independent;
each Protocol Buffers boundary requires a supported selection. Optional input/output
paths provide run-time defaults. Saved projects embed the schema and its
confined local imports, so execution does not reopen the original `.proto`
files. Unsupported recursive roots and imports outside the selected schema
folder show an error without replacing the open mapping.

Named input and output schema choosers also accept `.proto` files. Select a
supported root before adding or saving the boundary. Undo/redo and save/reopen
retain embedded imports after the original schema files are removed. Failed
imports and unsupported root changes leave the open mapping intact.

Named input and output editors preserve embedded layouts and explicit XML/JSON
formats when filenames change. Opening the format editor leaves saved CSV
defaults intact; choosing another format clears the previous format's settings.
Tabular fallback identities defer to recognized filename extensions, and
update-existing workbook controls are available only for flat XLSX output.
Replacing a Protocol Buffers boundary schema requires its exact embedded root
projection, or an explicit format change first.

Explicit live JSON requests are available through the opt-in native
[JSON REST host route](rest-json-host.md). Captured-response metadata and
offline execution keep their existing behavior.
