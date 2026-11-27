# Exact JSON Schema tuple arrays

A positional JSON array uses Draft 2020-12 `prefixItems` or an older draft's
array-valued `items`. Ferrule uses an ordinary homogeneous array whenever it
can represent the schema exactly. Otherwise a bounded *tuple frame*
holds the distinct positions and the optional tail.

<a id="refusals-this-design-replaces"></a>

Different positional schemas, a reachable heterogeneous tail, and the direct
`unevaluatedItems` form below can all be represented. Unsupported compositions
are refused rather than widened to accept extra values.

## Dialects

The keywords keep their current dialect gates (`ValidationDialect` in
`crates/format-json/src/json_schema/files.rs`):

| Dialect | Positional keyword | Tail keyword | `unevaluatedItems` |
| --- | --- | --- | --- |
| Draft 4, 6, 7 | array `items` | `additionalItems` | ignored as unknown |
| Draft 2019-09 | array `items` | `additionalItems` | active |
| Draft 2020-12 and unrecognized URIs | `prefixItems` | `items` | active |
| No `$schema` | either; both together are refused | as for the keyword used | active |

Draft 4 refuses boolean members but accepts a boolean `additionalItems`.
Draft 4 through 2019-09 ignore `prefixItems`, and Draft 2020-12 ignores
`additionalItems`. Array-valued `items` stays refused in Draft 2020-12.

## Selection and canonical shape

The importer first validates every member and tail schema. It then applies
every count from the schema's own `minItems`/`maxItems`, from modern `$ref`
siblings and from count-only `allOf` branches. Only after all of these
intersections does it put the shape into canonical form:

1. Drop positions beyond the effective maximum. With a maximum of N or less,
   the tail becomes closed.
2. If the remaining members are identical and the tail is closed, identical,
   or open over an arbitrary-JSON member, use the existing homogeneous
   lowering. A maximum of zero gives the existing empty-array form.
3. Otherwise build a frame.

An `allOf` branch can lower to a frame before it is intersected with the other
branches. Canonical form therefore runs again on the intersected result, which
can turn a frame into a homogeneous array.

A heterogeneous tuple is never widened into a homogeneous array, a
scalar-union item, or arbitrary-JSON items. If the frame cannot represent a
shape exactly, the shape is refused.

## Tuple frame

A frame is a `SchemaKind::Group` with layout metadata:

```rust
pub json_tuple: Option<JsonTupleLayout>,

pub struct JsonTupleLayout {
    /// Bounds on the complete array length: positions plus tail items.
    /// Absent means unconstrained, as for `item_count_range`; a closed frame
    /// always has one.
    pub length: Option<ItemCountRange>,
    pub tail: JsonTupleTail,
}

pub enum JsonTupleTail { Closed, Open }
```

Its children are fixed by role:

- `item1` through `itemN`, with 1 ≤ N ≤ 4,096 members in schema order.
- With `Open`, one more child named `rest`. It is repeating and holds the tail
  item schema. An absent or `true` tail gives a `json_any` item.

**No member, and no `rest` item, may itself be a JSON array.** That excludes
repeating nodes, nested-array wrappers and nested frames, and matches the
existing "cannot contain a nested array wrapper" refusal. A member may be a
scalar, a scalar union, an object (including alternatives and dynamic
properties) or an arbitrary-JSON string.

The [tuple-frame validator](../../crates/ir/src/schema/json_tuple.rs) enforces:

- **Children:** exactly the child list above, each following the member rule.
- **Canonical form:** a closed frame has a length maximum of exactly N; an
  open frame has no length bounds, no maximum, or a maximum above N; and
  selection step 2 would not lower the frame to a homogeneous array.
  Positions are identical when their JSON shapes are equal: the position's
  own name is ignored, and so is XML, database and value-generation metadata
  anywhere in its tree, because JSON reading, writing and export ignore it.
  A tail item is compared with its object-item nullability in place of a
  member's container nullability.
- **Not allowed on the frame:** `required` names, alternatives, dynamic
  properties, property constraints, scalar value metadata, `recursive_ref`,
  XML metadata, database relations, `json_array_wrapper`, `json_contains` and
  `json_unique_items`.
- **Not allowed on `rest`:** its own count, `contains`, `uniqueItems` or
  container nullability; the frame owns the array.
- **Placement:** a frame is never a dynamic property value
  (`additionalProperties` or `patternProperties`), and never appears inside a
  `contains` or dependent-schema predicate schema.

Every reader of saved schemas, both JSON writers, export, engine validation
and descriptor decoding call this one check.

The array length belongs to `layout.length`. On a non-repeating frame,
`item_count_range` is unset. On a repeating frame (an array of tuples), it
bounds the outer array as it does today.

On a non-repeating frame, `container_nullable` lets the tuple be `null`. On a
repeating frame, `container_nullable` covers the outer array and
`json_object_item_nullable` covers each tuple. A repeating frame must set
`json_object_item_nullable` explicitly, because
`json_single_container_nullable` (`crates/ir/src/lib.rs`) otherwise falls back
to `container_nullable`.

The frame check does not follow `recursive_ref`, so a member referring to
the frame itself can pass this local check. Import refuses cycles through
a position; the native codec also refuses a recursive
frame instead of expanding it.

In the mapping graph a frame is an ordinary group: paths address
`/point/item2` and iterate `/point/rest`. XML, CSV, XLSX, EDI, IDoc, SWIFT,
XBRL and database boundaries refuse schemas that contain frames.

## Reading and writing

Positions follow JSON Schema: a tuple requires no position unless `minItems`
does. Absence of a single tuple or of a position is always
`Instance::Scalar(Value::Null)`, never an empty group, so the original JSON is
recoverable exactly.

- **A missing non-repeating frame property** reads as `Scalar(Null)`, as
  missing nullable containers do today. Today a missing non-nullable group
  reads as an empty group (`missing_instance` in
  `crates/format-json/src/reader.rs`), and `is_boundary_absence` in
  `crates/format-json/src/lib.rs` never treats a group as absent. For frames,
  `Scalar(Null)` at a frame property is absence and the property is omitted.
  A present `[]` is a frame occurrence whose positions are all absent.
- **A missing repeating frame property** (an array of tuples) keeps today's
  array rule unchanged. It reads as an empty `Repeated` unless the outer array
  is nullable, and the writer omits an empty array only when its minimum is
  above zero. Otherwise it writes `[]`, as an ordinary missing array does
  today.
- **A position:**

  | JSON at position *k* | Field `item`*k* |
  | --- | --- |
  | Array length < *k* | `Scalar(Null)` |
  | `null`, member nullable | `JsonNull`, or `Scalar(JsonNull)` for a nullable object |
  | `null`, member not nullable | error at position *k* |
  | Any other value | the member's ordinary instance |

  `rest` reads as `Repeated` of the items after position N. It is empty when
  the array has N or fewer items.

Reading follows these steps, and the first error is reported:

1. Apply the frame's null rule.
2. Require a JSON array.
3. Check the length against `layout.length`. A closed tail fails here when the
   array is longer than N.
4. Read the positions in order.
5. Read the tail items in order.

Writing follows these steps:

1. Apply the null and absence rules. `JsonNull` writes `null`.
2. Require a group whose fields are all frame children. A position is absent
   if its field is missing, `Scalar(Null)`, or an empty `MappedSequence`.
3. The length is the last present position plus the number of `rest` items.
   An absent position before a present position, or before a non-empty
   `rest`, is a typed error naming that one-based position. Ferrule never
   pads a gap with `null`.
4. Check the length bounds.
5. Write the positions, then the tail items, using the ordinary normalization
   for array items.

`XmlNil` and `Decimal` keep their existing JSON write errors. The frame adds no
work budget.

Root-row output (an `Instance::Repeated` written against a non-repeating root)
and JSON Lines both refuse a non-repeating frame root. A repeating frame root
writes one tuple per line.

## References and composition

- Members may use any reference or composition that the importer already
  supports for an array item.
- A `$ref` to a tuple definition imports the frame. In Draft 2019-09, Draft
  2020-12 and schemas without `$schema`, `minItems`/`maxItems` siblings of that
  `$ref` intersect with the length. Draft 4, 6 and 7 ignore those siblings.
  Structural siblings stay refused.
- `allOf` may contain one positional branch. Every other branch may
  contribute only `type: array` and item-count bounds, and an empty
  intersection is refused.
- The existing nullable wrappers apply: `type` arrays with `null`, and `anyOf`
  or `oneOf` with a `{"type":"null"}` branch.

## Unsupported shapes

These shapes are refused at import. None of them is approximated by a
homogeneous array.

| Shape | Reason |
| --- | --- |
| Different positional schemas in two `allOf` branches | Needs per-position intersection |
| Tuple shapes as `anyOf`/`oneOf` alternatives, other than null | Needs positional alternative selection |
| A frame beside its own `allOf`/`anyOf`/`oneOf`, or under `not`, `if`, `then`, `else`, `contains` or `dependentSchemas` | Needs annotation or predicate support |
| A frame as an `additionalProperties` or `patternProperties` value | Needs runtime-named positional values |
| `contains`, `minContains`, `maxContains` or `uniqueItems: true` on a frame | Needs cross-position predicates |
| A `false` member | Would mean "shorter than *k*" |
| A member or `rest` item that is a JSON array, including a nested tuple | Member rule |
| A reference cycle through a position | Needs recursive frames |
| `prefixItems` beside `additionalItems` without `$schema`, when the result would be a frame | Ambiguous tail spelling; shapes that lower homogeneously today keep their current import |
| Every other `unevaluatedItems` | Depends on annotations |

## `unevaluatedItems`

`unevaluatedItems` is separate from the draft-specific positional keywords. It
is supported only on a direct concrete array schema that has none of these:

- `allOf`, `anyOf`, `oneOf` or `not`;
- `if`, `then` or `else`;
- `dependentSchemas` or `contains`;
- a `$ref`, `$recursiveRef` or `$dynamicRef` sibling.

The schema object is checked on its own. A schema that is referenced from
elsewhere qualifies.

Under those conditions, the evaluated items are known without inspecting any
instance:

- **Every item is evaluated** when one of these is present: schema-valued
  `items` in any active dialect (with or without `prefixItems`), or
  `additionalItems` after array-valued `items`. In that case
  `unevaluatedItems` is parsed and validated, and has no effect.
- **Otherwise `unevaluatedItems` is the tail.** It accepts the same `false`,
  `true` and schema forms. With no positional members it acts as
  schema-valued `items`.

## Export and round trip

Export writes a frame as a schema without a `$schema` keyword:

- `type: "array"`;
- `prefixItems` with the member schemas;
- `items: false` for a closed tail, the tail schema for an open tail, and no
  `items` for an arbitrary tail;
- `minItems` when it is above zero;
- `maxItems` only on an open frame that has a maximum.

Null layers and outer arrays use the existing wrappers. Export never writes
`additionalItems`. Because the frame check admits only canonical shapes,
importing an export reproduces the identical frame. Legacy inputs are
converted to the `prefixItems` spelling, and homogeneous tuples keep their
ordinary `items` export. This is JSON Schema export; MFD entry-tree export
has a separate refusal.

## Examples

When an instance is accepted, writing it back produces the same JSON, except
where a row notes the existing missing-array rule.

**1. Closed Draft 2020-12 tuple.**

```json
{"$schema": "https://json-schema.org/draft/2020-12/schema", "type": "array",
 "prefixItems": [{"type": "string"}, {"type": "integer"}, {"type": ["boolean", "null"]}],
 "items": false, "minItems": 2}
```

This is a closed frame with length 2..3.

| Instance | Outcome |
| --- | --- |
| `["a", 1]` | accept; `item3` is `Null` |
| `["a", 1, null]` | accept; `item3` is `JsonNull` |
| `["a", 1, true]` | accept |
| `["a"]` | reject: length below 2 |
| `["a", 1, true, 4]` | reject: length above 3 |
| `[1, "a"]` | reject: position 1 expects a string |
| `["a", null]` | reject: position 2 is not nullable |

Writing `{item1: "a", item2: Null, item3: true}` fails with a gap at position 2.
Writing `{item1: "a", item2: Null, item3: Null}` fails because the length is
below 2.

**2. Draft 7 legacy tuple with references and a schema tail.**

```json
{"$schema": "http://json-schema.org/draft-07/schema#",
 "definitions": {"code": {"type": "string", "minLength": 1},
                 "quantity": {"type": "integer", "minimum": 0}},
 "type": "array",
 "items": [{"$ref": "#/definitions/code"}, {"$ref": "#/definitions/quantity"}],
 "additionalItems": {"$ref": "#/definitions/code"}, "maxItems": 4}
```

This is an open frame with a `rest` of codes and length 0..4.

| Instance | Outcome |
| --- | --- |
| `[]` | accept; both positions absent, `rest` empty |
| `["A", 2, "B", "C"]` | accept; `rest` is `["B", "C"]` |
| `["A", 2, "B", "C", "D"]` | reject: length above 4 |
| `["A", -1]` | reject: position 2 is below the minimum |
| `["", 2]` | reject: position 1 is shorter than `minLength` |
| `["A", 2, 3]` | reject: tail item 3 expects a string |

**3. Draft 2019-09 references, `$ref` siblings, an optional frame property and
an array of tuples.**

```json
{"$schema": "https://json-schema.org/draft/2019-09/schema",
 "$defs": {"point": {"type": "array",
   "items": [{"type": "number"}, {"type": "number"}, {"type": "string"}],
   "additionalItems": false}},
 "type": "object", "required": ["origin"], "additionalProperties": false,
 "properties": {
   "origin": {"$ref": "#/$defs/point", "minItems": 2},
   "target": {"anyOf": [{"$ref": "#/$defs/point"}, {"type": "null"}]},
   "label": {"$ref": "#/$defs/point"},
   "path": {"type": "array", "items": {"$ref": "#/$defs/point"}, "maxItems": 3}}}
```

| Instance | Outcome |
| --- | --- |
| `{"origin": [0, 0]}` | accept; `target` and `label` are `Scalar(Null)` and stay omitted; `path` reads as empty and writes back as `"path": []` under the existing array rule |
| `{"origin": [0, 0], "label": []}` | accept; `label` is present with all positions absent and writes back as `[]` |
| `{"origin": [0, 0, "o"], "target": null}` | accept; `target` is `JsonNull` |
| `{"origin": [0, 0], "path": [[1, 2], [3, 4, "x"]]}` | accept |
| `{"origin": [0]}` | reject: the `$ref` sibling requires length 2 |
| `{"origin": [0, 0, "o", "extra"]}` | reject: closed tail |
| `{"origin": [0, 0], "path": [[1, 2, 3]]}` | reject: position 3 expects a string |
| `{"origin": [0, 0], "path": [null]}` | reject: tuples in `path` are not nullable |
| `{"origin": [0, 0], "path": [[], [], [], []]}` | reject: `path` has more than 3 items |
| `{"target": [1, 2]}` | reject: `origin` is required |

Under Draft 7, with `definitions` in place of `$defs`, the `minItems` sibling is
ignored and `{"origin": [0]}` is accepted.

**4. `allOf` without `$schema`.**

Here `{"allOf": [{"type": "array", "prefixItems": [{"type": "string"},
{"type": "integer"}], "items": false}, {"type": "array", "minItems": 1}]}`
gives a frame with length 1..2. `["a"]` and `["a", 1]` are accepted; `[]`,
`[1]` and `["a", 1, 2]` are rejected.

With `{"type": "array", "maxItems": 1}` as the second branch instead, the
intersected maximum drops position 2. The result is the existing homogeneous
string array with a maximum of 1.

**5. Refused schemas, one per line.** With `T` standing for
`{"type": "array", "prefixItems": [{"type": "string"}, {"type": "integer"}]}`:

- `{"allOf": [T, {"type": "array", "prefixItems": [{"minLength": 1}]}]}`:
  needs per-position intersection.
- `{"anyOf": [T, {"type": "array", "prefixItems": [{"type": "integer"}, {"type": "string"}]}]}`:
  positional alternatives.
- T plus `"uniqueItems": true`: a cross-position predicate.
- T plus `"additionalItems": false`, without `$schema`: ambiguous tail
  spelling.
- T with position 2 replaced by `{"type": "array", "items": {"type": "integer"}}`:
  violates the member rule.
- `{"$defs": {"t": {"type": "array", "prefixItems": [{"type": "string"}, {"$ref": "#/$defs/t"}]}}, "$ref": "#/$defs/t"}`:
  a recursive frame.

**6. `unevaluatedItems`.** P stands for
`"prefixItems": [{"type": "string"}, {"type": "integer"}]`. Rows use Draft
2020-12 unless noted.

| Schema | Result |
| --- | --- |
| `type: array`, P, `unevaluatedItems: false` | same as `items: false`; `["a",1]` accepted, `["a",1,2]` rejected |
| `type: array`, P, `items: {"type":"boolean"}`, `unevaluatedItems: false` | no effect; `["a",1,true]` accepted |
| `type: array`, `items: {"type":"string"}`, `unevaluatedItems: false` | no effect; `["a","b"]` accepted |
| `type: array`, `unevaluatedItems: {"type":"string"}` | same as `items: {"type":"string"}` |
| Draft 2019-09: `type: array`, array `items` [string, integer], `unevaluatedItems: {"type":"boolean"}` | Boolean tail; `["a",1,"x"]` rejected |
| `type: array`, P, `contains: {"type":"integer"}`, `unevaluatedItems: false` | refused: `contains` contributes evaluated items |
| `type: array`, `allOf: [{P}]`, `unevaluatedItems: false` | refused: depends on annotations |
| Draft 7: array `items` [string, integer], `additionalItems: {"type":"boolean"}`, `unevaluatedItems: false` | `unevaluatedItems` ignored; Boolean tail |

<a id="ownership-lanes"></a>

## Acceptance tests

The [tuple corpus](../../crates/format-json/tests/fixtures/json_tuple_arrays/cases.json)
contains schemas, canonical models, accepted trees, one-based error positions
and write-only gap cases. [Schema](../../crates/format-json/tests/json_tuple_schema.rs)
and [codec tests](../../crates/format-json/tests/json_tuple_arrays.rs) cover
different boundaries. Native prepared JSON, JSON5 and JSON Lines support
frames; generated Rust/C# JSON and JSON Lines use the same shape. The
generated JSON5 companion still refuses them.

Project files require `json_tuple_arrays_v1`; generated descriptors carry
the version that can represent the frame. Graph-level JSON parser functions
take unversioned schema strings and reject frames. MFD entry trees cannot
address positional fields: [import](../../crates/mfd/tests/json_tuple_import.rs)
keeps the schema-fallback diagnostic, and [export](../../crates/mfd/tests/json_tuple_export.rs)
refuses before writing any artifact. Homogeneous arrays remain supported.
