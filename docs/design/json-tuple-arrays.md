# Exact JSON Schema tuple arrays

A tuple array gives each array position its own schema: Draft 2020-12
`prefixItems`, or array-valued `items` in earlier drafts. Ferrule imports a
tuple only when it reduces exactly to an ordinary homogeneous array
(`crates/format-json/src/json_schema/positional_items.rs`). This design adds
one bounded representation for the remaining exact cases, the *tuple frame*,
and fixes which shapes stay refused. It changes no behavior by itself; the
ownership lanes below implement it.

## Refusals this design replaces

Three import refusals are replaced. Every other existing refusal stays.

- Positional members that differ after parsing ("positional item schemas must
  have one identical item schema").
- A tail that is reachable and either differs from the members or is open
  over a constrained member. A tail counts as reachable unless an explicit
  `maxItems` of N or less rules it out. An open tail is an absent or `true`
  tail. A `false` tail closes the tuple and is already supported.
- Active `unevaluatedItems` (`reject_unsupported_object_keywords` in
  `crates/format-json/src/json_schema.rs`), but only in the direct form
  specified below.

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

A frame is a `SchemaKind::Group` with new metadata:

```rust
pub json_tuple: Option<JsonTupleLayout>,

pub struct JsonTupleLayout {
    /// Bounds on the complete array length: positions plus tail items.
    pub length: ItemCountRange,
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

The frame check, modeled on `check_json_array_wrapper_frame`
(`crates/ir/src/schema/json_array_wrapper.rs`), enforces:

- **Children:** exactly the child list above, each following the member rule.
- **Canonical form:** a closed frame has `length.maximum() == N`; an open
  frame has no maximum or one above N; and selection step 2 would not lower
  the frame to a homogeneous array.
- **Not allowed on the frame:** `required` names, alternatives, dynamic
  properties, property constraints, scalar value metadata, `recursive_ref`,
  XML metadata, database relations, `json_array_wrapper`, `json_contains` and
  `json_unique_items`.
- **Not allowed on `rest`:** its own count, `contains` or `uniqueItems`.

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

In the mapping graph a frame is an ordinary group: paths address
`/point/item2` and iterate `/point/rest`. XML, CSV, XLSX, EDI and database
boundaries refuse schemas that contain frames.

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
current `items` export. MFD export goes through its existing re-import fidelity
check (`crates/mfd/src/export/json_schema_fidelity.rs`).

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

## Ownership lanes

Each lane is a separate issue with its own reserved paths. A lane removes only
the refusals listed for it. Until it merges, those refusals stay.

1. **Schema model.** This lane has three parts:
   - **Model.** `JsonTupleLayout` and the frame check in `crates/ir`, serde
     and tree validation, engine schema validation, and refusal at non-JSON
     boundaries and in schema edits that break a frame.
     `crates/codegen-schema/src/json5_profile/schema.rs` destructures
     `SchemaNode` exhaustively, so this part also adds the generated JSON5
     refusal for frames.
   - **Gates.** A `json_tuple_arrays_v1` file capability in
     `crates/mapping/src/file_codec`, descriptor version 5 in
     `crates/codegen-schema`, and a generation-time refusal in
     `crates/codegen-csharp` for schemas that contain frames. The graph-level
     `json_parse_field` and `json_parse_object_exists` functions take plain
     `SchemaNode` text with no capability or descriptor version, so older
     readers would silently drop frame metadata there. Frames therefore never
     enter those strings. This part refuses a frame descriptor in the Rust
     evaluator, `crates/functions/src/json.rs`; lanes 3 and 4 own the C#
     evaluator and the producer.
   - **Import and export.** Selection, canonical form, composition, refusals
     and export in `crates/format-json/src/json_schema/`. This part merges
     only after lane 2 and after the first step of lane 4.
2. **Native codec.** Covers reading and writing, frame-property absence, gap
   and length errors, prepared codecs, JSON5, JSON Lines and the borrowed
   writer in `crates/format-json`. It also includes engine handling of
   `Scalar(Null)` for absent frames and positions.
3. **Generated Rust and C#.** Generated Rust already shares the native codec
   through `format_json::PreparedSchema`; this lane adds generated-host tests
   for it. For C#, the lane adds the version 5 descriptor, frame check,
   reader, writer and first-error order in `runtime/csharp/Ferrule.Runtime/`,
   then removes lane 1's generation-time refusal. It refuses frame descriptors
   in the C# function evaluator,
   `runtime/csharp/Ferrule.Runtime/FerruleFunctions.Json.cs`. It also updates
   `Ferrule.Runtime.SmokeTests/JsonEmbeddedSchemaTests.cs`, which uses `/5` as
   its unsupported-version sample.
4. **MFD.** First, refuse frames explicitly at the MFD JSON schema boundary
   and in the JSON string parser recipe encoder
   (`crates/mfd/src/json_parser_recipe.rs`). That encoder is the only producer
   of graph-level function schema strings; `json_parse_field` is hidden from
   the editor palette. Affected projects keep today's schema-fallback
   diagnostic. Next, a black-box study establishes how the
   reference application names positional entries. Only after that do frames
   map to entries, become executable, and round-trip through export and
   re-import.
5. **`unevaluatedItems`.** The direct form above. It depends on the import and
   export part of lane 1.

Each lane updates [formats](../formats.md),
[compatibility baseline](../compatibility-baseline.md),
[code generation](../code-generation.md) or [MFD interop](../mfd-interop.md),
and the [file capabilities](../project-files.md#file-capabilities) list, in the
PR whose behavior those pages describe.

## Acceptance tests

A shared self-authored corpus,
`crates/format-json/tests/fixtures/json_tuple_arrays/cases.json`, follows
`nested_array_wrappers/cases.json`. Each case contains:

- the schema and the expected model;
- accepted instances with their instance trees;
- rejected instances with their error kind and one-based position;
- write-only instances covering gaps, length bounds, and absent frames and
  positions.

| Lane | Coverage |
| --- | --- |
| 1 | Import, canonical form, export, re-import and refusals, including examples 1 to 6; refusal by older readers and the Rust graph-level JSON function evaluator |
| 2 | Native, prepared and JSON Lines reads and writes |
| 3 | The same cases through Rust and C# hosts under `crates/cli/tests/code_generation/`, compared with `engine::run`; the C# graph-level JSON function refusal |
| 4 | Self-authored MFD fixtures |

The existing importer tests `prefix_items.rs`, `tuple_items.rs` and
`unsupported_keywords.rs` keep passing unchanged, except that heterogeneous
cases accepted by a lane move into the corpus as accepted cases.
