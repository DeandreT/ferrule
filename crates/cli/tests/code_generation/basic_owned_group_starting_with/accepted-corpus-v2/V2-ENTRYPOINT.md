# #443 source freeze v2 — start here

This is the single parent-authorized successor freeze of the existing authored
corpus, not a second independently authored corpus. It is source-only and UNRUN.
Parent accepted the custody explanation, **not the v1 seal or semantic acceptance**.

**v1 original old_profiles body unavailable; successor not custody repair.**

## Exact source boundary

- All101 matching entries from v1's original102-entry manifest are carried at
  their original relative paths with byte-for-byte equality. No inherited source,
  document, literal, catalogue, diagnostic, count, selector or path was corrected
  or renamed for v2.
- The sole new payload input is
  `source/conformance/basic-owned-group-starting-with/old_profiles.rs`, copied
  byte-for-byte from the parent's preserved
  `starting443-corpus-custody-failure-v1/observed-old_profiles.rs`.
  Its SHA256 is
  `bfb6cc637778716270b4003eedb1c320c6abaeb7cf8c19f22136bebaa7ee89fd`.
  The parent explicitly allowed the existing `#[derive(Debug)]` for complete
  returned-product evidence and inspected NamedInputAdmissionPlan Debug support.
- That input is the preserved post-seal body, **not a recovered v1 original**.
  The v1 manifest's expected digest
  `2da2b9b76058d41e2f84ca4455be33484208adf9cfd6f2c0ab18405093d7b9b7`
  remains historical evidence, not a digest claimed for the v2 source.

`source/` and `postimages/` contain the full carried source; nothing is represented
only by a delta or reconstructed from an expected hash. All inherited v1 names,
instructions, receipts and paths in README.md and the other copied documents are
historical and deliberately unedited. This entrypoint identifies v2's authority
and custody boundary; it does not silently revise those documents.

The earlier source inventory remains an authored UNRUN inventory. Semantic review
continues with reviewer `ses_eb0065320ffedAh1tUMBvrEWrb`; no reviewer actor or other
session was invoked for this freeze. Any later requested correction needs separate
parent direction and a fresh successor, never a post-seal edit to this root.

## Ancestry, not a repaired predecessor

`ancestry/v1/` retains the original v1 manifest and detached digest unchanged.
`ancestry/custody-failure-v1/` retains the parent's eight body/identity/metadata/
failure/baseline receipts, with their original bundle seal alongside it.
`ancestry/reconciliation-v1/` retains the accepted custody explanation unchanged.
See ancestry/README.md for the source mapping.

The original v1 manifest digest remains
`0554c9e4fddb8342a51b2ebf73bbbd74ca119d5274d260163e11a7556a2d3b06`.
Its failed body binding remains a failure. Do not use an ancestry manifest as the
v2 top-level manifest, infer a pre-seal body from the changed copy, delete the
derive, or describe current consistency as restored original custody.

## Byte/hash verification only

Fresh receipts are under verification/:

- INHERITED-101.sha256 and INHERITED-101-BYTE-EQUALITY.tsv: independently retained
  v1 expected rows and direct cmp equality for all101 carried entries.
- NEW-SOURCE-INPUT.sha256 and NEW-SOURCE-INPUT.txt: the one explicitly approved
  parent-preserved input and direct copy equality.
- PAYLOAD-102.sha256 and PAYLOAD-102-CHECK.txt: all102 carried payload paths,
  with the one new body identity rather than the unavailable original.
- V1/V2-LITERAL-BASELINE-CHECK.txt: all six original baseline entries, unchanged
  in both roots. LITERAL-INTEGRATION-COPY-EQUALITY.tsv covers the ten literal
  integration copies. No actual result was copied into an expectation.
- REFERENCE-SOURCE-CHECK.txt: all20 retained reference-source rows.
- INSPECTED-SOURCE-BINDINGS-CHECK.txt and
  ADDITIONAL-INSPECTED-SOURCE-BINDINGS-CHECK.txt: all21+13 original repository
  source bindings, read-only against the bound checkout, plus its identity receipt.
- PARENT-FAILURE-BUNDLE-CHECK.txt, V1-MANIFEST-IDENTITY-CHECK.txt and the ancestry
  copy-equality receipt: unaltered predecessor evidence and copy identity.
- PRIOR-INPUTS-BEFORE/AFTER.stat and the prior-input hash checks: bounded guards
  for all114 predecessor/custody/reconciliation input files. Atime is intentionally
  excluded; no prior input's bytes, mode, mtime or ctime is intentionally changed.
- QUIESCENT-COMPLETION.txt and CONTENT-PATHS.txt: the last pre-seal completion
  declaration and exact authored content pathset. No top-level manifest exists
  until that content is complete.

These are filesystem/custody checks only. No compiler, formatter, project parser,
code check/test, lowerer, emission, evaluation, mapping/model/consumer or network
operation was authorized or invoked. Repository and prior packet paths are read
only. Parent reports ordinary historical emission completed elsewhere; those
actuals are not imported here and no literal or expected output is changed.

## Final seal and write boundary

The top-level PACKET.sha256 is created only after authored content and verification
receipts are complete. It covers every regular file recursively except exactly
the two top-level seal files; ancestry manifests are included. PACKET.sha256.sha256
binds that new manifest. The inherited v1 manifest is not reused as a v2 seal.

All new files and directories have write permission bits removed before the final
manifest contents are written through the two sealer-only open descriptors. Those
descriptors are then closed. There are no payload changes during sealing and no
file or metadata writes after seal completion, including convenience fixes or
new receipts. Subsequent verification emits only to the session tool receipt.
This is permission-bit write denial, not a claim of an irreversible privileged
filesystem immutable flag. Parent will verify the final bytes and permissions
independently. No source acceptance or execution qualification is implied.
