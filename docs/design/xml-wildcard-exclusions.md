# XSD 1.1 wildcard exclusions

Design for [#1095](https://github.com/DeandreT/ferrule/issues/1095). It defines
a bounded contract for the XSD 1.1 `notNamespace` and `notQName` attributes on
`xs:any` and `xs:anyAttribute`. The contract adds two things: a negated
namespace set and a closed set of excluded expanded names. Every other wildcard
admission rule stays as it is. Each implementation route keeps its current
refusal until its own follow-up scope qualifies it.

## Current behavior

These facts describe the code this design builds on.

- **Import refusal.** For the `xs:any` particles and `xs:anyAttribute`
  elements the importer reaches, `parse_wildcard` and
  `parse_attribute_wildcard` in `crates/format-xml/src/xsd.rs` refuse any
  attribute whose local name is `notNamespace` or `notQName`. The refusal is
  `UnsupportedXmlWildcard`/`UnsupportedXmlAttributeWildcard` with the reason
  "XSD 1.1 wildcard exclusions are not supported". `ParseState::finish`
  returns the recorded error, so the whole import fails. MFD import
  (`crates/mfd/src/import/schema/mod.rs`) then silently uses a typed entry
  schema when one exists; otherwise it warns and falls back to the entry tree.
  The [formats reference](../formats.md#unsupported-profiles),
  [`formats.toml`](../reference/formats.toml) and
  [MFD interop](../mfd-interop.md) list exclusions as unsupported. The
  `formats.md` tables are generated from `formats.toml`.
- **Open content.** `parse_complex_type` ignores `xs:openContent`, and nothing
  reads `xs:defaultOpenContent`. Wildcards there never reach `parse_wildcard`,
  so neither the exclusion refusal nor any other wildcard rule applies to them
  ([#1143](https://github.com/DeandreT/ferrule/issues/1143)).
- **Namespace attribute.** `parse_wildcard_namespace` accepts `##any`,
  `##other`, or a list of URIs, `##local` and `##targetNamespace`. It refuses
  other `##` tokens and refuses `##any`/`##other` mixed with list members. The
  target namespace comes from the owning document's `targetNamespace` or from a
  single chameleon context (`SubstitutionIndex::effective_namespace`). When the
  document has no target namespace, `##targetNamespace` becomes unqualified.
- **Model.** `crates/ir/src/lib.rs` defines `XmlWildcardNamespaceConstraint`
  as `Any`, `Other { target_namespace }` or `List { namespaces }`.
  `Other::allows` rejects both absent names and the target namespace. Also
  defined there: `XmlWildcardProcessContents`, and the `SchemaNode` fields
  `xml_wildcard_namespace` and `xml_wildcard_process_contents`. The model has
  no negated arbitrary set and no field for disallowed names.
- **Element projection.**
  - A `skip` wildcard becomes the recursive generic `element()` group, with
    `LocalName`, `NamespaceURI`, text, `attribute()` and nested `element()`.
  - A `lax` wildcard adds typed fields for the matching indexed global
    declarations, then a `lax` generic fallback. It silently skips
    declarations that recurse under a repeating wildcard, declarations with
    substitution members, and incompatible same-local declarations.
  - A `strict` wildcard becomes typed alternatives only (`parse_strict_wildcard`)
    and retains no generic group. It refuses when an unresolved include or
    import could match, or when no declaration matches.
  - A projected port whose expanded name a declared sibling already uses,
    such as `<xs:element ref="Head"/>` beside the wildcard, yields to that
    sibling (`xsd/wildcard_ports.rs`). While the complex type is parsed, each
    projected port carries the wildcard's mode in
    `xml_wildcard_process_contents`, and the wildcard's own choice carries a
    reserved member, so both are identified exactly. Named complex types keep
    their unresolved content for use as an extension base, so a derived type
    resolves the base's ports against its own elements. The marks are cleared
    when the type is complete. Names compare by `xml_namespace_matches`, and
    same-local names in different namespaces still merge into one port with
    name alternatives.
    - A repeating sibling holds every occurrence. The wildcard's choice keeps
      its name while the members stay adjacent and no other choice owns it,
      so retained element order still interleaves them; otherwise the group
      writes in schema order.
    - A sibling after the wildcard, or an alternative in the same
      `xs:choice`, takes precedence, so the wildcard never matches its name.
    - A single-occurrence sibling before the wildcard in a sequence leaves
      its extra occurrences to the wildcard. A `lax` projection drops the port
      and its choice member, so the generic fallback receives them. A
      `strict` projection has no fallback and is refused with
      `UnsupportedXmlWildcard`, also when its port merged into that sibling.
      The refusal comes from parsing the declaration, so it also fails a
      document whose wildcard projects such a global declaration.
    - These rules apply to the namespaces the declared siblings cover. A port
      with name alternatives may be covered only partly. In one `xs:choice`
      it keeps the other namespaces, which then merge into the declared
      element as name alternatives. Elsewhere a `lax` projection drops the
      port and the generic fallback receives the other namespaces, while a
      `strict` projection would need a second port with the same local name
      and is refused.
  - Supported occurrences are `1..1`, `0..1` (not `skip`) and `0..unbounded`.
  - The importer refuses `xs:any` inside `xs:all`, multiple element wildcards
    in one type, and a `skip` wildcard beside declared elements.
    `xsd/restriction.rs` refuses wildcards in `complexContent` restrictions.
- **Attribute projection.** `lax`/`strict` add typed fields for matching global
  attribute declarations, then one `attribute()` group carrying the namespace
  constraint and mode. A local-only `skip` wildcard keeps the legacy group
  without `NamespaceURI`. The importer refuses more than one attribute wildcard
  per type.
- **Declaration index.** `crates/format-xml/src/xsd/substitution.rs` indexes
  top-level element declarations (abstract ones included and flagged) and
  attribute declarations across the local include/import graph. It records
  the namespace of every unresolved include or import. It does not process
  `xs:redefine` or `xs:override`.
- **Instance I/O.** `crates/format-xml/src/instance/generic.rs` works as
  follows.
  - `lax`/`strict` generic groups receive only elements and attributes that
    match no typed field. `skip` attribute groups keep every attribute.
  - A non-repeating typed field takes only its first matching element. A
    later occurrence of that name is a surplus occurrence. A `lax` generic
    group takes it through its usual checks as a generic item, which keeps
    its place in retained element order. The reader refuses it with
    `UnrepresentableXmlWildcardSurplusElement` when the group is unordered
    and the typed field follows the wildcard, since writing would put the
    extra occurrence first, and in a `strict` generic group. The importer
    never builds such a group, so this applies only to hand-built schemas.
  - Element read order (`read_group_fields`, then `read_generic_element`):
    strict-undeclared (a surplus occurrence in a `strict` group reports
    `UnrepresentableXmlWildcardSurplusElement` instead), then namespace
    (`XmlWildcardNamespaceMismatch`), then, for a surplus occurrence in an
    unordered group whose typed field follows the wildcard,
    `UnrepresentableXmlWildcardSurplusElement`. Attribute read order:
    namespace, then strict-undeclared.
  - Element write order (`write_generic_element`): name syntax, namespace,
    known typed field (`lax`/`strict`,
    `KnownXmlWildcardElementRequiresTypedField`), strict. The known typed
    field check admits a single-occurrence field's name once that field's own
    element has been written: earlier in the ordered stream, or, without
    retained order, from a populated field before the wildcard. A `strict`
    group then reports `UnrepresentableXmlWildcardSurplusElement`. In an
    ordered stream, an item that repeats an already written single-occurrence
    field goes to a `lax`/`strict` wildcard and must hold a generic element
    value; otherwise it reports the same error. Attribute write order
    (`push_generic_attributes`): name syntax, namespace, strict, known declared
    attribute (`lax`/`strict` only, `DuplicateField`), duplicate expanded name.
    `skip` attribute groups never check declared names.
  - `resolve_recursive_schema` (`instance/mod.rs`) copies wildcard metadata
    from each nested occurrence. Descendants of a generic element therefore
    carry no wildcard constraint.
- **Export.** `crates/format-xml/src/xsd/export.rs` writes `namespace` and
  `processContents`. It refuses `##other` under a different export target
  namespace. A strict element wildcard exports as its typed choice.
- **Consumers.**
  - `crates/engine/src/validate/schema.rs` validates wildcard process
    metadata.
  - Generated XML adapters refuse generic XML elements
    (`crates/codegen/src/validate/xml.rs`).
  - The JSON5 profile (`crates/codegen-schema/src/json5_profile/schema.rs`),
    the CSV boundary (`crates/codegen/src/csv_input_boundary.rs`) and the
    structured XML input (`crates/ir/src/xml_structured_input.rs`) refuse any
    wildcard metadata. The first two destructure `SchemaNode` exhaustively.
  - `SchemaNode` deserialization ignores unknown members. Project files name
    required reader capabilities through `crates/mapping/src/file_codec/mod.rs`.

Existing metadata is not enough. `Other` cannot express an arbitrary negated
set, and nothing records disallowed names. The runtime cannot evaluate
`##defined` or `##definedSibling` from the IR either: lax projections drop some
declarations, strict element wildcards keep no generic group, and the IR keeps
no global declaration set.

## Standard semantics

The reference is
[XSD 1.1 Part 1: Structures](https://www.w3.org/TR/xmlschema11-1/), §3.10 and
Appendix F.1.

- **Mapping (§3.10.2.2).**
  - `notNamespace` produces variety *not*. Its members are URIs, `##local`
    (absent) or `##targetNamespace` (the document's target namespace,
    otherwise absent).
  - `##other` is *not* {absent, target namespace}.
  - `notQName` produces {disallowed names}: expanded names, plus the keyword
    *defined* (`##defined`) and the keyword *sibling* (`##definedSibling`).
  - The default `processContents` is `strict`.
- **Representation (§3.10.3, Appendix A).** `namespace` and `notNamespace`
  must not both be present. `xs:anyAttribute` does not accept
  `##definedSibling`.
- **Properties Correct (§3.10.6.1).**
  - A *not* constraint has at least one member.
  - Each disallowed expanded name must be in a namespace the constraint allows.
  - An attribute wildcard never contains *sibling*.
- **Matching (§3.10.4.1–3).** An item matches when its namespace is allowed
  and its expanded name is not disallowed.
  - *defined* also rejects any name that resolves to a top-level element
    declaration (element wildcards) or attribute declaration (attribute
    wildcards).
  - *sibling* rejects any name that matches an element declaration contained
    directly, indirectly or implicitly in the content model of the governing
    type.
- **Chameleon inclusion (Appendix F.1).** QNames in `notQName` that have no
  namespace take the including document's target namespace.

## Contract

### Accepted syntax

Admitted forms on `xs:any` and `xs:anyAttribute`:

- `notNamespace`: one to 64 whitespace-separated tokens. Each token is a
  non-empty URI that does not start with `##`, or `##local`, or
  `##targetNamespace`. It cannot appear with `namespace`.
- `notQName`: whitespace-separated tokens. Each token is a lexical `QName`
  (an optional prefix plus an NCName), `##defined`, or (on `xs:any` only)
  `##definedSibling`. An empty value is an empty set. Repeated tokens are one
  member.
- `notQName` combines with any `namespace` value or with `notNamespace`.
  Both attributes work with every occurrence the importer supports today, with
  `skip` and `lax` element wildcards, and with every attribute-wildcard mode.
  They also apply through named model groups and attribute groups. Exclusions
  never admit a particle, occurrence, compositor or wildcard combination that
  is refused today.
- Strict element wildcards with exclusions stay refused. They keep no generic
  group, and the ordinary-group reader ignores unmatched children, so an
  excluded name would be accepted and dropped rather than refused. They become
  admissible only once that reader refuses unmatched names.
- Open content (`xs:openContent`, `xs:defaultOpenContent`) is outside this
  contract. Its wildcards are not processed today, with or without exclusions
  ([#1143](https://github.com/DeandreT/ferrule/issues/1143)).

### Resolution at import

Import resolves every exclusion to exact expanded names.

- **`notNamespace` members.** These resolve like `namespace` members today:
  `##local` is unqualified, and `##targetNamespace` is the owning document's
  target namespace (or unqualified when there is none).
- **QName tokens.** A prefix resolves against the in-scope namespace
  declarations of the wildcard element. An unprefixed QName takes the default
  namespace in scope; with no default namespace, it is unqualified. In a
  chameleon-included document, a QName without a namespace takes the adopted
  target namespace. The name must be allowed by the wildcard's namespace
  constraint.
- **`##defined`.** This resolves to every indexed global declaration whose
  namespace the constraint allows: elements (including abstract ones) for
  `xs:any`, attributes for `xs:anyAttribute`. The set must be closed. Import
  refuses when an unresolved include or import namespace is allowed by the
  constraint (the existing strict-closure test), or when the graph contains
  `xs:redefine` or `xs:override`.
- **`##definedSibling`.** This resolves to every element declaration contained
  in the governing complex type's content model whose namespace the constraint
  allows:
  - local and referenced element particles in its model group tree;
  - the members of named model groups it references;
  - for a `complexContent` extension, the base content plus the extension's
    own particles;
  - the concrete substitution members of those declarations.

  The governing type is the instance's `xsi:type` when present, otherwise the
  element's declared type. When a derived type extends a base that holds the
  wildcard, the derived type governs, and the siblings include the elements
  its extension adds. Particles with
  `maxOccurs="0"` (`is_disabled_particle`) correspond to no declaration and add
  nothing. Neither do typed fields projected from wildcards. Siblings in a
  namespace the constraint already rejects are dropped. They could never match
  the wildcard, and keeping them would break the invariant that every excluded
  name is allowed.

  Resolution runs when the governing type is complete, not at the wildcard
  particle or in the cached expansion of a named base type
  (`ParseState::complex_types`). A wildcard in a shared named group or base
  type therefore gets each governing type's siblings, including siblings
  declared after the group reference. A sibling without an exact expanded name
  is refused.

  `attach_type_alternatives` merges the declared type and its `xsi:type`
  alternatives into one group by member name, so one `element()` field serves
  every alternative and can hold only one exclusion set. Import therefore
  refuses an element whose declared type and any attached alternative resolve
  the shared wildcard to different exclusion sets. Today, members that differ
  make `attach_type_alternatives` return without alternatives and without an
  error. The native codec must report this case as the typed refusal rather
  than inherit that silent return.

The excluded set is the union of explicit names and keyword results, after the
namespace filter. It is deduplicated and sorted canonically. An empty result
records no metadata. More than 256 names is refused. The keywords themselves
are not retained.

### Model

Follow-up scope 1 adds:

- `XmlWildcardNamespaceConstraint::Not { namespaces: XmlWildcardNamespaceList }`,
  serialized as `{"kind":"not","namespaces":[...]}`. `allows` returns true when
  no member matches. `##other` stays `Other`. A `notNamespace` set is never
  rewritten as `Other`, and `Other` is never rewritten as `Not`.
- `XmlExpandedName { namespace: XmlNamespace, local: String }`, where `local`
  is an NCName. `XmlWildcardExcludedNames` is a non-empty, duplicate-free set
  of at most 256 names in canonical order: unqualified first, then namespace
  URI bytes, then local-name bytes.
- `SchemaNode::xml_wildcard_excluded_names: Option<XmlWildcardExcludedNames>`,
  omitted from serialization when `None`. It is valid only on a node with valid
  `xml_wildcard_namespace` metadata, and every name must be allowed by that
  constraint. Engine schema validation reports any other placement.
- When a wildcard has exclusions, import always uses the namespace-aware
  generic form, including the `##local` `skip` attribute case.
  `resolve_recursive_schema` copies the new field from the occurrence, so
  descendants never inherit the anchor's exclusions.
- The file codec writes a new required capability, `xml-wildcard-exclusions-v1`,
  whenever any project schema uses `Not` or excluded names. Older readers would
  otherwise drop excluded names silently and widen the wildcard.

### Reading and writing

The exclusion check reports the new `XmlWildcardExcludedName { name, namespace }`
or `XmlAttributeWildcardExcludedName { name, namespace }`. It runs immediately
after the namespace check on every path. Every other existing check keeps its
current relative order:

| Path | Order |
| --- | --- |
| Element read | strict-undeclared, namespace, **exclusion** |
| Attribute read | namespace, **exclusion**, strict-undeclared |
| Element write | name syntax, namespace, **exclusion**, known typed field, strict |
| Attribute write | name syntax, namespace, **exclusion**, strict, known declared attribute, duplicate expanded name |

Existing strict attribute groups without exclusions therefore report the same
errors as today. An excluded name reports the exclusion error even when it is
also a declared sibling or declared attribute. Every check runs before the
element's start tag is emitted.

Attribution rules:

- Declared and projected typed fields receive their own names.
- The lax projection leaves out every excluded declaration, so an excluded
  global name reaches the generic group and is refused.
- An attribute wildcard with exclusions gives declared attributes to their
  typed fields only, in every mode, `skip` included. On read they never enter
  `attribute()`. On write, the known-declared-attribute check, which today
  runs only for `lax`/`strict`, also refuses them through a `skip` group with
  exclusions. Reads and writes are therefore symmetric.
- `skip` attribute wildcards without exclusions keep their current behavior.

**Surplus occurrences.** A surplus occurrence of a non-repeating declared
element reaches the generic group's checks (see
[Current behavior](#current-behavior)), so an excluded sibling name such as
the second `<t:Head/>` in E2 reaches the exclusion check.

Exclusions apply only to the wildcard's own matches. Descendants of generic
elements stay unconstrained. The `xsi` attribute handling, and the reader's
leniency about particle order and unknown children of ordinary groups, are
unchanged.

### XSD export

- `Not` exports as `notNamespace`, and no `namespace` attribute is written.
  An unqualified member becomes `##local`. A member equal to the export target
  namespace becomes `##targetNamespace`. Any other member is written as its
  URI.
- Excluded names export as one `notQName` of explicit QNames in canonical
  order. The wildcard element declares its own prefixes (`fwx1`, `fwx2`, …) for
  qualified names. When an unqualified name is present, it also writes
  `xmlns=""`, so unprefixed tokens never take the schema's default namespace.
- Keywords are never exported. The explicit set is exact for the imported
  declaration graph.
- The existing `##other` target-namespace refusal still applies.
- Output that uses either attribute requires an XSD 1.1 processor. Ferrule
  adds no `vc:` version attributes.

### Refusals

The importer reports each case below as `UnsupportedXmlWildcard` or
`UnsupportedXmlAttributeWildcard`, with a specific reason:

| Case | Rule |
| --- | --- |
| `namespace` and `notNamespace` together | §3.10.3 |
| Empty `notNamespace`, more than 64 members, or `##any`, `##other` or an unknown `##` token in it | §3.10.6.1, bound |
| Undeclared prefix or lexically invalid QName in `notQName` | QName resolution |
| QName in a namespace the constraint does not allow | §3.10.6.1 clause 4 |
| `##definedSibling` on `xs:anyAttribute` | Appendix A, §3.10.6.1 |
| `##defined` with an allowed unresolved include or import, or `xs:redefine`/`xs:override` in the graph | closed declaration set |
| `##targetNamespace` or a namespace-less QName in a document reached through more than one chameleon context | unknowable namespace |
| `vc:` conditional-inclusion attributes on the wildcard or an ancestor | the importer does not evaluate them |
| Sibling without an exact expanded name, or more than 256 resolved names | bound |
| An element whose declared type and `xsi:type` alternatives resolve one shared wildcard to different exclusion sets | one field, one set |
| `notNamespace`/`notQName` in a foreign namespace | kept as today |
| Any exclusion on a `processContents="strict"` (or defaulted) `xs:any` | the reader cannot refuse excluded names |

All current refusals still apply when exclusions are present: `xs:all`,
unsupported occurrences, optional single `skip`, several wildcards per type,
`skip` beside declared elements, restriction wildcards, and unresolved or empty
strict sets.

## Follow-up scopes and route gates

Each scope is a separate issue. A route keeps its current refusal until its
own scope merges with the matching tests.

1. **Model.** `ir` types and validity, engine validation, the file capability,
   and fail-closed handling in every consumer. The exhaustive destructuring in
   the JSON5 profile and the CSV boundary must refuse the new field, and the
   structured XML input must refuse it too. The field-by-field
   `same_recursive_anchor_definition` comparisons in `xsd/export.rs` and
   `xsd/export/set.rs` must compare the new field, so that anchors differing
   only by exclusions refuse. Until scope 2, the canonical `attribute()`
   equality check in `validate_export_node` refuses attribute groups that carry
   exclusions. XSD export also refuses explicitly any element or attribute
   group whose wildcard metadata uses `Not` or excluded names. That includes a
   `Not`-only constraint from `notNamespace` without `notQName`:
   `wildcard_namespace_attribute`, which serves both `xs:any` and
   `xs:anyAttribute`, gains a refusing `Not` arm. MFD export refuses exclusion
   metadata atomically. No importer change.
2. **Native codec.** Covers `format-xml`
   resolution, the reader/writer checks, XSD export (including the canonical
   `attribute()` form with exclusions), and the frozen fixtures below. It also
   narrows the unsupported row: edit `docs/reference/formats.toml`, regenerate
   the generated block of `formats.md` with `cargo run -p reference-docs`, and
   hand-edit the XML narrative that follows the generated block in
   `formats.md`. `import-xsd`, the CLI and native runs gain the profile in this
   scope. MFD import treats an exclusion-bearing schema like today's wildcard
   import error.
3. **MFD route.** Admit exclusion-bearing component schemas on import and
   export their siblings. Self-authored designs cover this route, while
   reference-application qualification stays external and separate.
   [MFD interop](../mfd-interop.md) changes here.
4. **Generated Rust and C#.** No scope is opened. Generated adapters refuse
   generic XML wildcards altogether. Any future generated wildcard support must
   implement exclusions in both languages in the same change, or refuse them.

## Frozen fixtures

Namespaces: `T` = `urn:ferrule:wx:t`, `A` = `urn:ferrule:wx:a`,
`B` = `urn:ferrule:wx:b`, `X` = `http://www.w3.org/XML/1998/namespace`. All
schemas are self-authored. A T-qualified schema has `targetNamespace` T,
`elementFormDefault="qualified"`, unqualified attributes, and no default
namespace. Its root binds `t`, `a` and `b` to T, A and B. In the instance
tables, the root is `<t:Root xmlns:t="T" xmlns:a="A" xmlns:b="B">`.

`a.xsd` declares A-qualified global elements `Known`, `Note` and `Secret`
(`xs:string`) and the global attribute `flag`. `b.xsd` declares the
B-qualified global element `Note` (`xs:string`).

**E1. Negated namespaces with an explicit name, `lax`, `0..unbounded`.** A
T-qualified schema imports `a.xsd` and declares:

```xml
<xs:element name="Root"><xs:complexType><xs:sequence>
  <xs:element name="Head" type="xs:string"/>
  <xs:any xmlns:a="urn:ferrule:wx:a" notNamespace="##targetNamespace ##local"
          notQName="a:Secret" processContents="lax"
          minOccurs="0" maxOccurs="unbounded"/>
</xs:sequence></xs:complexType></xs:element>
```

Import gives `Head`, typed `{A}Known` and `{A}Note`, and a `lax` `element()`.
The `element()` group has `Not [{T}, unqualified]` and excludes `{A}Secret`.

| Content after `<t:Head/>` | Outcome |
| --- | --- |
| `<a:Known>k</a:Known><b:Extra/>` | Accepted. `Extra` is the only `element()` item, with `NamespaceURI` B |
| `<a:Other/>` | Accepted as a generic item |
| `<b:Secret/>` | Accepted as a generic item, because exclusion matches the exact expanded name |
| `<a:Secret>s</a:Secret>` | `XmlWildcardExcludedName { Secret, A }` |
| `<t:Tail/>` | `XmlWildcardNamespaceMismatch { Tail, T }` |
| `<Tail xmlns=""/>` | `XmlWildcardNamespaceMismatch { Tail, ##local }` |

Writing a generic item `{Secret, A}` reports the exclusion error before any
output. Writing `{Known, A}` reports `KnownXmlWildcardElementRequiresTypedField`.
Writing `{Extra, B}` round-trips. Exporting and re-importing gives the same
schema.

**E2. Keywords through a named group.** A T-qualified schema declares a global
`Global` element and:

```xml
<xs:group name="Open"><xs:sequence>
  <xs:any namespace="##targetNamespace" notQName="##defined ##definedSibling"
          processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
</xs:sequence></xs:group>
<xs:element name="Root"><xs:complexType><xs:sequence>
  <xs:element name="Head" type="xs:string"/>
  <xs:group ref="t:Open"/>
  <xs:element name="Tail" type="xs:string" minOccurs="0"/>
</xs:sequence></xs:complexType></xs:element>
```

The excluded names are `{T}Global`, `{T}Head`, `{T}Root` and `{T}Tail`. `Tail`
comes after the group reference, which shows resolution waits for the complete
type. The lax projection adds no typed fields.

| Content | Outcome |
| --- | --- |
| `<t:Head/><t:Note>n</t:Note><t:Tail/>` | Accepted. `Note` is generic |
| `<t:Head/><t:Global/>` | `XmlWildcardExcludedName { Global, T }` |
| `<t:Head/><t:Root/>` | `XmlWildcardExcludedName { Root, T }` |
| `<t:Head/><b:X/>` | `XmlWildcardNamespaceMismatch` |
| `<t:Head/><t:Head/>` | `XmlWildcardExcludedName { Head, T }` |

Writing a generic `{Tail, T}` reports the exclusion error. Export writes the
four names as explicit QNames, and re-import gives the same schema.

**E3. Same-local names across imports.** A T-qualified schema imports `a.xsd`
and `b.xsd` and declares one
`<xs:any namespace="##other" notQName="a:Secret" processContents="lax"
minOccurs="0" maxOccurs="unbounded"/>` in `Root`.

- Import gives typed `Known` and a typed `Note` that accepts both A and B, plus
  the generic fallback. `Secret` is not projected.
- With `notQName="a:Secret a:Note"`, `Note` accepts only B. `<b:Note/>` reads
  into the typed field, while `<a:Note/>` reaches the generic group and reports
  `XmlWildcardExcludedName { Note, A }`.
- The same wildcard with `processContents="strict"` (or no `processContents`)
  is refused, because strict element wildcards with exclusions stay refused.

**E4. Siblings outside the namespace constraint.** A T-qualified schema imports
`a.xsd` and declares:

```xml
<xs:element name="Root"><xs:complexType><xs:sequence>
  <xs:element name="Head" type="xs:string"/>
  <xs:element ref="a:Known"/>
  <xs:any namespace="##other" notQName="##definedSibling"
          processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
</xs:sequence></xs:complexType></xs:element>
```

`{T}Head` is dropped because `##other` already rejects T, so import succeeds
and excludes only `{A}Known`. The lax projection adds typed `Note` and
`Secret`. `<t:Head/><a:Known/><b:Y/>` is accepted with `Y` as the generic
item. Writing a generic `{Known, A}` reports the exclusion error.

**E5. Extension and disabled particles.** This schema matches the T-qualified
setup except `elementFormDefault="unqualified"`, so local elements are
unqualified:

```xml
<xs:complexType name="Base"><xs:sequence>
  <xs:element name="Head" type="xs:string"/>
  <xs:any namespace="##local" notQName="##definedSibling"
          processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
</xs:sequence></xs:complexType>
<xs:complexType name="Derived"><xs:complexContent>
  <xs:extension base="t:Base"><xs:sequence>
    <xs:element name="Extra" type="xs:string" minOccurs="0"/>
    <xs:element name="Gone" type="xs:string" minOccurs="0" maxOccurs="0"/>
  </xs:sequence></xs:extension>
</xs:complexContent></xs:complexType>
<xs:element name="Plain" type="t:Base"/>
<xs:element name="Root" type="t:Derived"/>
```

The global declarations are in T, so `##local` projects none of them.

- Importing `Root` succeeds. Nothing derives from `Derived`, so `Root` has no
  `xsi:type` alternatives. Its wildcard excludes `{}Extra` and `{}Head`; a
  set reused from the cached `Base` expansion would hold only `{}Head`. `Gone`
  excludes nothing.
- Importing `Plain` is refused with `UnsupportedXmlWildcard`. Its declared
  type `Base` would exclude `{}Head`, while its `Derived` alternative would
  exclude `{}Extra` and `{}Head`, and one shared `element()` field cannot hold
  both sets.

| `Root` content | Outcome |
| --- | --- |
| `<Head/><Gone/>` | Accepted. `Gone` is generic |
| `<Head/><Note/>` | Accepted. `Note` is generic |

Writing a generic `{Extra}` under `Root` reports
`XmlWildcardExcludedName { Extra, ##local }`.

**A1. Attribute exclusions.** A T-qualified schema imports `a.xsd` and
declares:

```xml
<xs:element name="Root"><xs:complexType>
  <xs:attribute name="id" type="xs:string"/>
  <xs:anyAttribute notNamespace="##targetNamespace"
                   notQName="##defined id internal xml:space"
                   processContents="skip"/>
</xs:complexType></xs:element>
```

The excluded names are `{}id`, `{}internal`, `{X}space` and `{A}flag`.

| Attributes on `Root` | Outcome |
| --- | --- |
| `id="1" mode="m" b:z="z" xml:lang="en"` | Accepted. `id` is typed only. `attribute()` holds `mode`, `{B}z` and `{X}lang` |
| `internal="x"` | `XmlAttributeWildcardExcludedName { internal, ##local }` |
| `a:flag="x"` | `XmlAttributeWildcardExcludedName { flag, A }` |
| `xml:space="preserve"` | `XmlAttributeWildcardExcludedName { space, X }` |
| `t:code="x"` | `XmlAttributeWildcardNamespaceMismatch` |

Writing an `attribute()` item `{id}` or `{internal}` reports the exclusion
error. Export and re-import give the same schema.

**R. Refusals.** Each refusal row above has a one-wildcard schema in the style
of the existing `rejects_wildcard_profiles_that_cannot_be_projected` harness:

- `R-unresolved` imports `urn:ferrule:wx:missing` without a `schemaLocation`
  and uses `notQName="##defined"` with `##any`.
- `R-chameleon` includes one namespace-less document from two target
  namespaces.
- `R-retained` repeats each current refusal with a `notQName` added.

The existing attribute case `namespace="##local" notQName="blocked"` becomes
admissible, so its refusal test switches to a still-refused form.

## Acceptance plan

- **Model scope.**
  - Wire round trips for `Not` and excluded names.
  - Validity refusals: wrong node, disallowed namespace, duplicate, empty,
    over-limit and invalid NCName.
  - Engine validation issues.
  - Writing the capability, and older readers refusing it.
  - Unchanged wire form for existing `Any`/`Other`/`List` fixtures.
  - Refusals in every generated profile and the XML adapter.
  - Recursive-anchor comparison that refuses anchors differing only by
    exclusions, and explicit XSD export refusal.
  - Atomic MFD export refusal.
- **Native codec scope.**
  - Every E1–E5 and A1 import shape, read row and write row.
  - Export/re-import equality and instance equality.
  - Unchanged MFD import fallback for an exclusion-bearing component schema.
  - Every refusal row and every retained refusal.
  - Unchanged check order and errors for existing strict attribute groups.
  - The existing lax, QName and import/export wildcard suites still pass, with
    only the one exclusion refusal case updated.
- **MFD scope.** Self-authored designs over E1 and A1 import, run, export and
  re-import without semantic drift. Reference-application evidence stays
  separate.
