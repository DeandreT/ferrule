# Explicit filename document fixture

This fixture is independently authored synthetic content. `DOC` contains one
`ABC` segment with two string fields, mapped to the complete JSON object
`{"label":"blue widget","units":"7"}`. It does not encode an order, customer,
external service or reference-application mapping.

`InputFile` and `OutputFile` connect to the EDI source and JSON target filename
inputs. Instance and parameter previews are decoys, not runtime defaults. The
mapping's business fields remain separate from both filename graph roots.

`Host.cs.txt` exercises a standalone generated .NET 10 Release consumer through
`ExecutePrimaryEdiDocumentBytes` and `EvaluateOutputFilename`. `Host.rs.txt` uses
the equivalent generated Rust APIs; the CLI integration test materializes it
beside the generated crate. Both check complete output, filename identity,
pre-host filename refusals, direct-selection cardinality and malformed input.
Hosts perform no output publication.

The companion case adds an existing object JSON string parser to the business
mapping. Its `FileInstance` input carries JSON content, not a filename. The
filename importer delegates that component to the existing parser admission
rather than counting it as another document endpoint.
