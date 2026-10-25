# Static EDIFACT document boundaries

C# static document CLI generation selects EDIFACT only from an explicit
`FormatOptions.edi_kind = Edifact` boundary; library callers select
`DocumentBoundaryOptions::Edifact`. A filename or schema does not select
the family. `EdifactBoundaryOptions::from_format_options` retains supported EDI
settings and refuses unrelated formats and unresolved configuration resources.
Every primary and named endpoint is admitted before artifacts are emitted.

The static adapter uses the same `FerruleEdi` runtime as primary document plans,
with strict envelope validation enabled for input and output. Complete UNB/UNZ
interchanges may contain direct UNH/UNT messages or UNG/UNE groups. Counts,
references and ownership are checked before parsing and after serialization.
UNA declares input service characters; output uses the native codec's canonical
syntax. Syntax versions before 4 preserve literal `*`; version 4 uses the
declared repetition character and typed repeating fields.

`ExecuteDocumentOutputsWithHost` and its byte variant parse inputs once, execute
the mapping once, and serialize primary then named targets in declaration order.
No output bundle is returned until every writer succeeds. Selected-target APIs
use the same complete input admission but construct only the selected target.
Host context supplies the same clock to mapping and envelope completion; the
adapter neither obtains its own clock nor accesses files or the network.

Boundary failures retain the codec's typed exception inside
`DocumentExecutionException` and identify the primary or named endpoint.
Malformed envelopes retain `FerruleEdiError.Envelope` and a zero-based segment
index from the shared validator.
Per-document codec limits and the static bundle limits (4096 documents and
256 MiB of UTF-8) apply before mapping. JSON and X12 retain their own codecs.
