# Declarative MFD function libraries — v3 source contract

Source-prepared, not compiler/SDK/runtime admitted. The earlier source-only
commits and their stronger pending oracles are retained; v3 keeps the v2 refusals for
the conversion/consumption cases whose correctness was not established.

The importer reads document-local declarations of this form:

```xml
<settings><libraries>
  <import path="../libraries/strings.mfd" saverelative="1"/>
</libraries></settings>
```

There is **no import alias**. Functions retain the exact `library` and `name`
attributes on their direct declaration components. Saved call `componentid`
values must match that definition's input/output component IDs, not similar
parameter names. IDs and ports are local to each document and definition.

## Authority and ownership

- A natural reference is relative to its declaring document and confined to
  the explicitly approved package root (the main document's directory by
  default). No basename search, sibling-project discovery, global application
  libraries, or case-insensitive fallback selects another file.
- A relocated root library requires `ResourceBindingKindV1::MappingLibrary`
  through `ImportOptions::with_resource_bindings_v1`. It uses the existing
  exact original-origin/reference selector, expected SHA-256, optional
  `with_origin_sha256`, dependency pins, and `prepare` snapshots. Never label
  library metadata as an XML schema or EDI configuration.
- Transitive imports are relative to the owning imported document and confined
  to the **same authorizing root**, not the main mapping's resources or an
  unrelated catalog. Pins on the root binding can seal these dependencies.
  Separate relocation bindings for transitive owners are not supported.
- Native rooted reads refuse links/reparse points and special files. Parsing
  consumes the immutable delivered buffer whose hash is recorded; it never
  hashes a pathname and asks an XML parser to reopen it.
- Each document owns its declarations and direct file imports. In v2 nested
  function calls are refused before expansion; a main-mapping declaration is
  never borrowed to supply their missing body or signature.
  Local declarations take precedence over imports of the exact same symbol.
  Conflicting declarations and ambiguous same-symbol imports are refused;
  no import-order precedence is guessed. Ambiguity still produces a diagnostic
  even if a local declaration subsequently shadows that symbol.
- Transitive symbol **re-export** is deliberately unsupported: import the
  defining document directly when calling its function from the main mapping.
  The available documentation establishes dependency trees, but not sufficient
  ownership rules for guessing re-export visibility or alias remapping.

The library's main mapping is never imported or evaluated. Its payload paths,
HTTP/database components and adjacent code modules are not opened. A missing
library remains an unresolved resource/required-producer diagnostic. Kind-19
calls in a mapping declaring MFD libraries do not fall through to custom-module
discovery or captured-output/name substitution.

## Current supported contract and explicit deferred capabilities

Imported definitions use the **existing scalar UDF reader and materializer**,
not a separate evaluator. Supported imported functions are regular, single-output,
first-class scalar helpers with named, explicitly typed mandatory inputs, no
connected defaults, and complete function-owned port wiring. They remain real
`UserFunctionCall` nodes, so the existing declared input **and output** adapters
and eager argument-consumption boundary are retained. A builtin `boolean` or
`string` operation is not substituted for those adapters.

The following capabilities remain pending and are now explicit refusals:

- optional/defaulted, inline and multi-output imported signatures;
- nested calls inside imported definitions, or calls to imported functions from
  embedded scalar/structured wrappers (even when an argument appears unused);
- imported calls used in root-function ABI mappings, failure rules/predicates,
  runtime filenames, forwarded expressions or unconsumed outputs;
- calls not consumed directly by ordinary XML scalar target entries below an
  explicit unqualified `document/@instanceroot` and nonempty schema reference.
  Only a direct content root or the existing exporter's namespace-verified
  `FileInstance/document` wrapper is recognized. Protocol/root sockets and
  other namespace/root encodings are deferred, not inferred from display names.
- call-site inline/context overrides, structured/qualified port metadata and
  unknown scalar call attributes beyond the saved kind19/name/library/UID and
  flat component-ID port contract.

The v3 consumer gate checks the **downstream role**, not just leaf geometry:

- `FileInstance` inputs selected by the XML reader are runtime filename ports,
  even below a content root and regardless of `ns`, `pos`, or entry type. Such
  collisions and legacy decimal-slot/`@` name aliases are refused. An unconnected
  verified `FileInstance/document` protocol wrapper remains permitted.
- Any direct `properties/@PassThrough="1"` marks an intermediate variable, not
  an ordinary final target. This owner is refused before any of its ports are
  admitted, including when its outputs reach a later ordinary target. No
  transitive consumer walk may erase that boundary. Other output-bearing XML
  consumer owners also remain outside the supported terminal profile.
- All raw declarations of a connected numeric input key enter the census;
  duplicate/normalized aliases cannot hide a protocol or second consumer owner.
  The gate shares the XML reader's actual first-content and filename-input
  selectors. Later content roots, transparent type-wrapper sockets and skipped
  decoration entries are not treated as ordinary scalar targets.

These are private typed consumer refusals carried through the existing import
warning path; they do not add a public error API or claim whole-import admission.
The four stronger pending positive oracles remain retained and unweakened. All
v3 fixtures are authored only, not compiled or run.

Mappings declaring MFD libraries do not use the specialized early root-view
return: imported calls must first pass the ordinary consumption census, even
when their output appears unused.

These are not declared impossible. Their typed value/error/eager/lazy boundaries
need real implementation and independently admitted proof. The v1 default and
transitive-call positive tests are retained with explicit pending-capability
markers, alongside active v2 refusal controls. The normal embedded ABI and
non-imported lowering remain in place. Ordinary scalar recursion now also
returns a bounded resource diagnostic instead of bypassing expansion/depth limits.
New pending Bool-default/If and nested Int-identity/comparison oracles also retain
the desired typed values and malformed-input errors. They are not passing v2
admission tests and must not be deleted or weakened to make a test run green.

All body components are checked, including unused ones. Unknown/custom,
structured, resource-reading, recursive, runtime-clock/random/delay and
origin-sensitive filepath bodies are refused rather than stubbed, pruned into
partial support, or passed to schema/module recognizers. This also refuses
unproved requiredness/cardinality/context metadata, XML namespaces on function
metadata, reserved built-in/node-function/`webservice`/`wsdl` ownership, and unknown library
formats (`.mff`, DLL, XSLT, etc.) in this declared-MFD ingestion path. Ordinary
embedded UDF lowering remains in place.

Reserved kind-19 owners in mappings declaring MFD libraries are refused **before**
builtin, schema or HTTP dispatch. No HTTP operation is used to resolve a function.

Supported scalar operations may still fail at runtime. Lazy branches *within* a
supported helper remain lazy, but connected regular-call arguments are not
discarded or relocated below a condition. Resource-free is not total. Portable projects
contain owned existing recipes/helper graphs, not live external callbacks.
Existing export support applies to those graphs; unsupported graph shapes keep
the existing precise export refusals rather than emitting an opaque import.

## Budgets and private evidence

One closure is limited to 32 files, 8 MiB per file, 32 MiB aggregate delivered
bytes (repeated deliveries count), 16 document levels, 128 import edges, 512
declarations and 131,072 XML nodes. A shared sticky quota of 65,536 construction
work units and 32 MiB of decoded/copied text covers metadata, every ordinary
recursive expression build, argument slots, literal/name clones, and helper-graph
construction **before** allocation. This is not a promised final graph size:
metadata reservations consume quota too. The 256-level ceiling is checked during
ordinary recursion as well as cloning/substitution; synthetic adapters receive
conservative depth/work charges. Expanded definitions are moved, not cloned
before a post-hoc size check. No cap was raised. The native rooted-path budgets also apply.
Qualified symbols are bounded to 256 UTF-8 bytes each and scalar literals to
1,024 bytes before expansion; scalar pin positions are below **64** and unique
per direction, matching the existing reader without its invalid-position fallback.
Parameter default/output ports support exactly one position-0 slot. Requiredness
uses that same slot, never "any connected source". Connected defaults are still
refused for the separate typed-adaptation reason. Ordinary scalar producers use
only primary output0 (filter outputs0/1); extra conditional/filter slots and
constant trigger/context inputs that this reader would not consume are refused.
This is a reader-consumption limit, not a replacement builtin table for older
MFD versions. Legacy connection vocabularies, value-map body
metadata and extra graph/context attributes are not in this initial subset.
DTD/entities, file import cycles, definition cycles and digest mismatches are
refused. Imported closure acquisition is staged before registration.

An optional `ResourceReadSetV1` records actual consumed raw byte hashes for both
natural and bound MFD libraries. `mapping_library_definitions()` additionally
retains the exact declaring document/hash/reference, resolved document/hash,
qualified function identity, full saved declaration XML (including refused
signature/default/port metadata), and any refusal. Accepted v2 functions are
leaf functions, so their function-dependency closure is empty. Refused nested
references remain in the raw declaration, not an invented admitted closure.
This evidence is host-private, bounded, and never serialized into a Project.
Unpinned transitive bytes are recorded, **not claimed pre-authorized by hash**.
The readset is append-only evidence, not an admission ledger: delivered bytes or
`refusal: None` for a leaf definition do not establish a successful whole import,
supported call role, executable Project, export, or Rust/C# equivalence.

## Format rationale

Official documentation distinguishes MFD UDF libraries from custom executable
libraries, states that local library paths are relative to the declaring MFD,
describes declared library names, local-before-import precedence, and nested
library-import trees:

- [Manage Function Libraries](https://www.altova.com/manual/Mapforce/mapforceenterprise/mff_libraries_manage.html)
- [Local and Global Libraries](https://www.altova.com/manual/Mapforce/mapforceenterprise/mff_libraries_local_vs_global.html)
- [Relative Library Paths](https://www.altova.com/manual/Mapforce/mapforceenterprise/mff_libraries_relative_path.html)
- [UDF Basics](https://www.altova.com/manual/Mapforce/mapforceenterprise/mf_func_udf_basics.html)
- [mfd-filepath](https://www.altova.com/manual/Mapforce/mapforceenterprise/mff_lib_core_path_mfd-filepath.html)

These rules do not supply any absent owner's definitions or establish the
identity of a historical/similarly named library.
Current builtin-name tables or a current `SourceRootBuiltin.mff` are not blanket
authority for older MFD versions. No `.mff` body or similarly named definition
is substituted for a missing declared coeval MFD library.
