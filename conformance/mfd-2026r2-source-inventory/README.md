# Supplemental source inventory

These files review documentation bodies beyond the original topic inventory in
[`mfd-2026r2.source-contracts.json`](../mfd-2026r2.source-contracts.json).
They do not change the canonical capability ledger or establish execution support.

| File | Source coverage |
| --- | --- |
| `nonleaf.json` | 191 parent-topic bodies, including additional operations and existing-context refinements |
| `functions.json` | 319 named function leaves, parameter tables, language declarations and unresolved contract facets |
| `constructors.json` | 43 `xs:` constructor leaves and their shared lexical/type statements |
| `com-api.json` | Separate public COM API catalog; release binding remains unestablished |
| `com-name-alignment.json` | Name-only interface, enum and event comparison against the release-bound type library, preserving discrepancies and dump-rendering gaps |
| `com-enum-values.json` | All 20 release-bound enum tables and 130 signed constants, compared with the pinned online catalog without establishing runtime support |
| `supporting.json` | Public I/O classes, wrapper-library exceptions and the named installation-artifact requests |
| `schema-wrappers.json` | Four optional schema-wrapper source profiles, actual schema identities and build/dependency metadata; no compiled or runtime qualification |
| `external-references.json` | Linked historical DOM algorithm and redirected .NET interface contract, kept separate from vendor claims |

The manual reviews use the same pinned PDF and layout-text hashes as the original
source companion. Line numbers refer to that exact extraction; physical PDF page
numbers remain distinct from printed headers. The public PDF can be retrieved from
<https://www.altova.com/documents/MapForceEnterprise.pdf>; verify its recorded hash
before reusing those anchors. `pdftotext -layout` produces the referenced extraction.

Missing declarations, defaults, types and conflicting source claims remain
explicit. An editorial signature summary is not a literal API declaration.
Language badges describe vendor documentation, not Ferrule support or independently
verified backend applicability. These overlapping sets are not an additive parity
denominator.

The COM catalog has its own per-page hashes and explicitly provisional identities.
Its documented installation and visible-window requirements do not prove a working
vendor environment. Its scoped catalog-completeness flag does not establish
Enterprise 2026r2 inventory completeness. The canonical `inventory_complete` flag
remains false while release provenance, supporting classes and remaining source
contracts are unresolved.

The type-library name comparison does not bind the online catalog's signatures
or behavior to the installed release. It retains the different spellings of
`HighlightMyConnectionsRecursively` and `HighlightMyConnectionsRecursivly`,
additional declaration names, and dump-rendering gaps resolved by bounded raw
name-table reads. Event-source names are compared separately from interface
members. Signatures remain unqualified; name matches and accessor-record counts
are not additive capabilities or ABI proof.

The separate enum-value inventory retains exact declaration names, signed values,
record offsets and source hashes. Of 130 constants, 124 match the pinned catalog,
one differs (`eProjectTypeCount`: 18 in the type library, 16 online), and five are
type-library-only. The three `-1` sentinels use a referenced signed value, not a
literal interpretation of their encoded offset. `ENUMViewMode` values come from
bounded raw records because the pinned dump does not render that table. The
existing three-value observation in `artifact-observations.json` is preserved.
These declarations do not establish aliases, accepted runtime options, backend
applicability, or the online catalog's release binding. The canonical inventory
remains incomplete, and no execution evidence follows from this comparison.

The public supporting declarations are inventoried. `schema-wrappers.json`
records bounded source bundles for C#, Java and both C++ DOM options, including
actual schema names and build dependencies. The requested compiled C# DLLs,
complete API contracts and runtime behavior remain unqualified. Running the
generated Java Ant default target executes the mapping; it is not a source-only
inspection. The requests and provenance requirements are listed in `supporting.json`.
Undocumented supporting classes are explicitly implementation-private and
changeable in the manual; including them requires a bounded choice of generation
fixtures and settings, not an assumption that all possible schema-generated
classes form a finite catalog.

Run `cargo test -p mfd-conformance --test source_inventory` to validate finite
coverage, unique source identities and the separation from execution assessments.
