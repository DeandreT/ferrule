# Single-writer completion and sealing protocol

This is a byte-preserving copy/freeze operation, not code or model execution.
No source correction is made. All101 v1 matching entries and the one approved
parent-preserved source input must pass their independent digest and direct
byte-equality checks before sealing. The original baseline, reference and34
repository source-binding checks must pass. All prior-input hash and selected
metadata guards must pass, including the preserved ancestry copies.

All authored documents and generated filesystem receipts are completed first.
QUIESCENT-COMPLETION.txt declares that boundary; CONTENT-PATHS.txt records the
complete content pathset, including itself. Only then are the two new top-level
manifest files opened for exclusive creation. No other writer is authorized.

Remove all file and directory write bits in v2 while the sealer holds only the
two manifest descriptors. Hash the now-quiescent content through the manifest
descriptor, close it, write its detached digest through the second descriptor,
and close that descriptor. The seal-completion boundary is the closing of the
last manifest descriptor. Source files and all other content remain unchanged
throughout this phase. The top-level manifests exclude only themselves, not
nested ancestry manifests.

After that boundary there must be no file or metadata write, chmod, new receipt,
note, edit or reseal. Read-only verification of the complete manifest, detached
digest, content pathset, inherited101, new input and permission bits may return
to session output only. Parent owns independent verification and further scope.
Any later correction requires a separately authorized fresh successor.
