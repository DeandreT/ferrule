# Supported Formats

ferrule converts every supported input into the shared `ir::Instance` tree and
writes target instances through a separate adapter. Format selection normally
comes from the input or output extension; embedded `FormatOptions` provide the
layout and dialect details that an extension cannot express.

| Format | Source | Target | Current scope |
| --- | :---: | :---: | --- |
| XML | Yes | Yes | Hierarchical instance I/O; namespace-aware element and attribute names; XSD-lite with local import graphs, compatible `complexContent` and scalar-text/attribute-only `simpleContent` derivations, namespace-constrained skip wildcards, declaration-aware lax element/attribute wildcards, and closed strict wildcard choices; bounded DTD import with internal content-model parameter entities; attributes, `xsi:nil`, generic elements, and ordered mixed content; external DTD identifiers are never loaded |
| JSON | Yes | Yes | Hierarchical instance I/O and JSON Lines; confined external and local JSON Schema references, compatible structural `allOf` intersections, ordinary scalar `const` and singleton `enum`, exact object-property presence requirements, heterogeneous scalar type arrays, exact scalar `anyOf`, pairwise-disjoint scalar `oneOf`, scalar-domain-subsumed array `anyOf`, compatible object alternatives and multi-branch nullable compositions, nullable scalar/object/array shapes, and typed or unconstrained dynamic properties |
| CSV | Yes | Yes | Delimited flat rows with configurable delimiter and headers |
| Fixed-width | Yes | Yes | Validated Unicode-scalar column layouts, configurable fill, record separators, and empty-value handling |
| XLSX | Yes | Yes | Typed worksheets, flat and selected composite/grid source shapes, hierarchical targets, and update-existing writes |
| SQLite | Yes | Yes | Table introspection, typed reads, imported relational query shapes, validated declared relations, structured XML text columns, and idempotent full-replace writes |
| X12 / EDIFACT | Yes | Yes | Schema-guided interchange I/O, custom syntax separators, repetitions, qualifier loops, retained field lengths/code lists, and optional lenient parsing |
| HL7 v2 / TRADACOMS | Yes | Yes | Bounded schema-guided message I/O, retained field lengths/code lists, HL7 escapes/subcomponents, and TRADACOMS release escaping |
| SAP IDoc | Yes | Yes | Embedded fixed-record layouts with bounded byte-position parsing and deterministic output |
| SWIFT MT | Yes | No | Input through embedded message and field grammars |
| FlexText | Yes | Yes | Embedded recursive split/store/switch layouts, including fixed-width and delimited records |
| Protocol Buffers | Yes | Yes | Bounded proto2/proto3 binary I/O with self-contained local import graphs, public imports, nested messages, enums, repeated fields, and packed scalars |
| XBRL | Yes | Yes | Typed instance facts, contexts, dimensions, units, and namespace-qualified concepts |
| PDF | Yes | No | Layout-driven visual extraction from positioned text and painted edges, typed BasicVisual text capture, and visible inherited CropBox/page-rotation normalization |

## Important Boundaries

- PDF targets are not supported.
- XBRL taxonomy formula, presentation, calculation, and linkbase execution are
  outside the current runtime.
- XML preserves declared expanded-name identity for elements and attributes.
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
  Strict attribute processing rejects undeclared runtime names. XSD 1.1
  exclusions, unordered wildcard compositors, and unresolved strict declaration
  sets remain outside the subset.
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
  Object-shaped `required` declarations preserve property presence independently
  from value nullability, including declared names on closed objects and named
  runtime properties on open objects. Required-only schemas without an object
  shape remain outside the subset because they also admit every non-object value.
  Ordinary scalar `const` and equivalent singleton `enum` constraints are
  enforced on both input and output and survive export. A `const` combined with
  a larger `enum` is supported when the constant is a member; general
  multi-value enums remain unsupported until the IR can retain an allowed-value
  set without widening the boundary.
  Ordinary `minimum`, `maximum`, `exclusiveMinimum`, and `exclusiveMaximum`
  constraints are likewise enforced for concrete integer and finite-number
  scalars, including nullable scalar wrappers. Integer constraints normalize to
  one exact inclusive `i64` interval; number constraints retain inclusive or
  exclusive finite endpoints and reject intervals containing no representable
  finite value. Import accepts both modern numeric exclusive bounds and Draft 4
  boolean exclusives regardless of the declared dialect for interoperability;
  export emits the canonical normalized form. Range-bearing general scalar
  unions and `multipleOf` remain unsupported.
  Concrete arrays retain exact non-negative `minItems` and `maxItems`
  intervals through references, nullable wrappers, compatible `allOf`
  intersections, and exactly representable `anyOf` unions. Input and output
  enforce the interval before visiting array items. JSON Lines applies a root
  array interval to the total nonblank line count; nullable root arrays reject
  because line-oriented null-container semantics are ambiguous. Disjoint count
  unions and independently constrained nested array wrappers reject rather than
  widen. `$ref` siblings follow the dialect declared by their physical schema
  resource: Draft 4, 6, and 7 ignore them, while Draft 2019-09, 2020-12, and
  schemas without `$schema` apply Ferrule's supported numeric, item-count, and
  annotation metadata. Unknown and empty string `format` annotations are
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
  General heterogeneous array composition, validation-bearing scalar unions,
  and mixed structural unions remain unsupported.
  Shape-neutral validation keywords are
  accepted for schema recovery but are not enforced by the mapping runtime.
- Database execution is SQLite-specific and does not yet provide a general SQL
  mutation or multi-database connector model.
- Complex XLSX, PDF, EDI, and FlexText layouts depend on an embedded validated
  configuration; unsupported imported commands remain explicit warnings.
- EDI output validates every present configured field after wire lexical
  formatting. The bounded report includes all length and code-list violations
  it finds, and validation failures do not replace the destination.

The [workflow-parity roadmap](../ROADMAP.md) tracks the remaining format and
connector work.
