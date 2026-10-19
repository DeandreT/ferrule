# First literal milestone — ordinary Starting, UNRUN

These five UTF-8/LF files were authored before all #443 model operations.
No reader, serializer, lowerer, emitter or evaluator has been invoked on them.
`legacy-project.json` is the authored complete pretty+LF saved document and
serialization oracle, grounded in Project/ScopeRef/SchemaNode serialization at
merged f30f6ca1. Its exact re-encoding must later match; a discrepancy is a
refused oracle requiring independent source review, never automatic rewriting.

The only partition declaration is ordinary `source:[Rows]` plus
`group_starting_with:2`. Node2 is a framed legacy Bool Start read. No owned
field, owned selector, permission extension or new wire field appears.
The members use inherited legacy `source:[]`. Nodes1/3 are framed legacy Label
and Position(Rows). The literal five Starts are F,F,T,T,F, with complete expected
members [one,two], [three], [four,five], ordinals1,2,3. All field order, scalar
tags and Unknown Group XML origins are preserved in the separate tree files.

This is a literal input/output and serialization milestone, not emitted artifact
or execution evidence. The parent independently owns exact merged-base
historical generation/reconciliation; no old #442 baseline is reused as #443.
