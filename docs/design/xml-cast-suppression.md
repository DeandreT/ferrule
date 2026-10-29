# Explicit XML target-cast suppression

An XML target with native `suppress-cast-in-subtree` metadata requests lexical
output, not a change to its declared schema. Ferrule retains that request in
`FormatOptions.xml_scalar_output`. No setting is inferred from absent metadata.

## Supported policies

`SuppressCastInSubtreeV1` carries the complete, bounded, sorted set of target
fields declared as `xs:double`. The target stays a numeric schema. This flat
profile is a closed, unqualified group with ordinary String/Float fields,
ordinary root construction and no named endpoints. Repeated scalar occurrences
are independent values; they are not collapsed into a winner. Its declarations
must be exactly representable, so native export restores them.

`SuppressCastInDocumentV1` covers any other ordinary document: a bounded,
plain, unqualified element tree of groups and String/Int/Float/Bool fields.
Every scope constructs a group through the ordinary item path, with optional
source iteration, filters, sorting and grouping; scalar, dynamic, concatenated
and owned scopes are refused. It retains no declarations beyond the target
schema, so declared-XSD regeneration follows ordinary XML export.

Both profiles refuse fixed/default values, nil, abstract or substitutable
elements, mixed content, attributes, choices, nested cast modes and other
local overrides. The flat profile skips adaptation only for its double fields
in the root group; the document profile skips it for every scalar binding in
every scope of the primary target. The XML writer preserves String values
without parsing or trimming; integer, finite floating-point and (document
profile) Boolean values use the existing lexical formatters.
It retains normal XML shape validation, escaping, character checks and output
limits. A private writer-local projection does not change the project schema
or any embedded public schema descriptor.

This explicit mode can produce XML that fails the declared XSD: for example,
`not-a-number` remains text in a declared double field. It does not disable XML
safety or expression errors. Ordinary XML writers remain strict, and generated
JSON APIs retain their numeric schema and existing normalization/refusal rules.

## Refusals and persistence

Known unsupported requests are retained as `Unresolved`, with a closed reason
and bounded explanatory detail. They are not executable approximations. An
unowned request in a transformed component or user function is refused rather
than lost. Source-side policy, named endpoints and pipeline execution are
unsupported. Removing or replacing the request is an explicit user edit.

Project and pipeline file codecs declare the
`xml-target-scalar-output-v1` required capability whenever the option is present,
including invalid placements and unresolved requests. Older file readers
therefore refuse rather than silently ignore the semantics. This guarantee
belongs to the file codec, not ordinary model serde.

## Boundaries and native interchange

Native import verifies the exact document wrapper, namespace and declaration
ownership against confined XSD metadata. The flat profile applies when its
occurrence bounds and `xs:double` declarations are exactly representable;
otherwise an ordinary document keeps the request through the document profile.
The authoritative XSD may use named types, references and value facets, which
never change written text, but not the element settings listed above. An
explicit `AddSchemaLocation="1"` stays unresolved; an absent setting is
imported like every other XML target, without the reference application's
automatic schema URI. Native export emits the suppression token on the document
wrapper and, for the flat profile, restores the double declarations. For the
flat profile, export also refuses a binding order it cannot preserve. Both keep
any explicit conversion helpers.

File and compatibility-payload output paths must identify XML. Bounded payload
profiles validate their declared codec identity instead of using the artifact
filename, as they do for other formats. Preview and browser XML output use the
same explicit policy. Generated XML adapter admission is mandatory when this
policy is present: a failure cannot silently produce a typed-only fallback.

The [native controls](../conformance/xml-target-casting.md) distinguish this
mode from enabled casting. Suppression is not a lossless native replacement
for the separate [`coerce_float` contract](target-float-cast.md).
