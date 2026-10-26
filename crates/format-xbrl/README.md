# Native XBRL table bytes

`from_bytes_bounded(bytes, schema, options, limits)` and
`to_bytes_bounded(schema, instance, options, limits)` are opt-in in-memory
boundaries. Construct immutable `XbrlLimits` from `XbrlLimitOptions`, or use
`XbrlLimits::default()`. Existing file/string APIs retain their behavior.

The bounded APIs reuse the native typed table reader and renderer. Namespace
bindings in `XbrlBoundaryOptions` are required for selected concepts; equal local
names in different namespaces remain distinct. The schema must contain exactly
one non-root repeating group. Context order, typed parsing failures, absent
values versus zero, omission of missing facts, fact-empty row pruning, unit
defaults and rendered bytes follow the native codec.

## Closed instance profile

Input is UTF-8 XML with one namespace-qualified XBRL instance root. Accepted
content is:

- Root `context`, `unit`, `link:schemaRef`, and namespace-qualified leaf facts.
- A context with one entity identifier (nonempty `scheme` and text), and
  one instant, forever, or ordered start/end period. Context IDs are unique.
- Optional explicit dimensions in one `entity/segment`, with unique expanded
  axis QNames and namespace-resolvable member QNames.
- Units with one measure, or a divide with one numerator measure and one
  denominator measure. Unit IDs are unique and QNames must resolve.
- Fact attributes `contextRef`, `unitRef`, `decimals`, `precision`, `id` and
  `xml:lang`. Every fact, including unselected concepts, must reference an
  existing context; every supplied unit reference must resolve. Duplicate
  expanded concept/context pairs fail. References may precede their definitions.
- Ordinary comments, processing instructions, CDATA, the five XML character
  references and numeric character references, subject to XML validity/budgets.

The reader projects the native connected fact/identifier/period fields, not
dimension ports or root unit definitions. A schema requesting `explicitMember`
or `typedMember` input ports is explicitly refused rather than filled with null
values. Explicit dimensions can be validated on input and rendered from native
target dimension groups. This is not taxonomy validation: period values use the
native lexical handling, and measures are not classified against a taxonomy.
Target dimension groups require namespace bindings for both the axis and its
`xbrldi:explicitMember` port. Simple-content target fact groups and conflicting
target periods/unit forms are refused instead of silently dropping values.

DTD/entity declarations, arbitrary general entities, tuples/nested facts,
typed dimensions, scenario content, multi-measure units, nil facts, unknown
structural elements/attributes, inline XBRL and linkbase execution are refused.
QName values use a closed lexical subset (letter/underscore start followed by
letters, digits, underscore, hyphen or period). References in namespace
declarations are refused because the XML token scanner and DOM parser otherwise
interpret their namespace identities differently. No taxonomy, schema, entity,
network or filesystem resource is loaded, even if a URI/path is present.

## Budgets and allocation boundary

| Resource | Default |
| --- | ---: |
| Input / output bytes, independently | 16 MiB |
| XML depth | 64 |
| XML nodes / attributes, independently | 1,000,000 |
| XML text bytes | 16 MiB |
| Facts / explicit dimensions, independently | 100,000 |
| Contexts / units, independently | 10,000 |
| Model depth / nodes | 64 / 1,000,000 |
| Model text / individual string bytes | 64 MiB / 1 MiB |

Zero allows no occurrences; depth must be 1–256. Limits must fit in `isize`.
Limits are inclusive, and additions are checked. Failures return typed
`LimitExceeded { limit, maximum, actual }`, `InvalidLimit`, or explicit profile
errors. Existing scalar, duplicate-fact, unit and target errors stay typed.

On read, the input byte bound and streaming XML preflight run **before**
`roxmltree` creates a DOM. Every element, including unselected content, counts.
The node count includes a document node and start/empty, text, CDATA, reference,
comment, declaration and PI events. End tags do not add nodes. Adjacent text
fragments may count separately even when the DOM merges them. Attribute counts
include namespace declarations. Text bytes include raw attribute values,
comments, declaration/PI content and text/reference spellings. These
conservative wire counts bound normalized DOM data without entity expansion.
Selected materialized nodes and strings are checked before retaining them.

Schema and option names/paths count toward the model budget. On write, the
entire supplied instance counts too, including fields the schema does not
select. A separate model budget bounds collected target rows/facts, including
candidate rows before pruning and copies of shared identifiers/defaults.
The shared renderer first writes to a checked counting sink, enforcing XML and
output byte limits without retaining the document. Escaped text is streamed
without an expanded string copy. Only a successful count allocates an
exact-length output buffer, and the second rendering pass is checked again.
Generated XML is checked against the same closed profile before return.

These are finite data/work bounds, **not an exact process-RSS ceiling**. Caller
storage, allocator capacity/overhead, parser and attribute/event scratch buffers,
schema metadata and the native intermediate representation also consume memory.
There is no timeout or cancellation API. Callers needing process isolation or a
wall-clock deadline must supply those separately. Every call owns its budgets;
a failed call retains no reusable parse/output state and a corrected retry is
independent.
