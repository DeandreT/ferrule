# Supported Formats

ferrule converts every supported input into the shared `ir::Instance` tree and
writes target instances through a separate adapter. Format selection normally
comes from the input or output extension; embedded `FormatOptions` provide the
layout and dialect details that an extension cannot express.

| Format | Source | Target | Current scope |
| --- | :---: | :---: | --- |
| XML | Yes | Yes | Hierarchical instance I/O; namespace-aware element and attribute names; XSD-lite with local import graphs, compatible `complexContent` and scalar-text/attribute-only `simpleContent` derivations, namespace-constrained skip wildcards, declaration-aware lax element/attribute wildcards, and closed strict wildcard choices; bounded DTD import with internal content-model parameter entities; attributes, `xsi:nil`, generic elements, and ordered mixed content; external DTD identifiers are never loaded |
| JSON | Yes | Yes | Hierarchical instance I/O and JSON Lines; confined external and local JSON Schema references, compatible structural `allOf` intersections, bounded exact scalar `const`/`enum` domains, exact numeric ranges and decimal `multipleOf`, exact array-count, `contains` match-count, object-property-count, and Unicode string-length intervals, exact structural `uniqueItems`, bounded portable string `pattern` assertions, exact closed homogeneous `patternProperties`, exact object-property presence, property dependencies and whole-object dependent-schema predicates, exact single-property-presence conditionals, property-name constraints, and open/closed object semantics, heterogeneous scalar type arrays, exact scalar `anyOf`, pairwise-disjoint scalar `oneOf`, scalar-domain-subsumed array `anyOf`, compatible object alternatives and multi-branch nullable compositions, nullable scalar/object/array shapes, and typed or unconstrained dynamic properties |
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
  portable `pattern` conjunctions/disjunctions, and ordered `format`
  annotations retained without vocabulary assertion. Finite complements
  intersect by excluding the union of their sets; compatible `anyOf` branches
  exclude only names common to every branch, and a double complement restores
  the finite allowed set. Pattern, length, correlated, and other infinite
  complements reject instead of being widened. Draft 4
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
  finite value. Import accepts both modern numeric exclusive bounds and Draft 4
  boolean exclusives regardless of the declared dialect for interoperability;
  export emits the canonical normalized form. Positive finite `multipleOf`
  divisors are retained as canonical decimal coefficients and exponents, with
  no floating-point tolerance. Compatible `allOf` branches form conjunctions
  and exact `anyOf` branches form disjunctions. Contiguous same-type numeric
  range branches normalize to one exact interval when their divisor constraints
  are identical. When an `anyOf` varies both its numeric range and divisor
  constraint, Ferrule rejects the correlated union rather than independently
  widening either axis. Numeric-range-bearing heterogeneous scalar unions remain
  unsupported.
  Concrete arrays retain exact non-negative `minItems` and `maxItems`
  intervals through references, nullable wrappers, compatible `allOf`
  intersections, and exactly representable `anyOf` unions. Input and output
  enforce the interval before visiting array items. JSON Lines applies a root
  array interval to the total nonblank line count; nullable root arrays reject
  because line-oriented null-container semantics are ambiguous. Disjoint count
  unions and independently constrained nested array wrappers reject rather than
  widen. `$ref` siblings follow the dialect declared by their physical schema
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
  unions, ambiguous untyped assertions, and constrained nested arrays reject
  rather than move or widen an assertion.
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
