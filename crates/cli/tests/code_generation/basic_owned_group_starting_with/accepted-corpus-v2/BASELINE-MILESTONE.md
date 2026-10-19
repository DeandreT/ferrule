# Ordinary Starting literal baseline — authored, UNRUN

The five baseline files in `postimages/conformance/basic-owned-group-starting-with/`
are frozen before the functional corpus is authored. They define a genuine
ordinary saved Project, with existing `group_starting_with`, framed legacy
SourceFields and no owned selectors or expressions. The entire schemas, three
nodes and stored source spelling are explicit. The Project's pretty+LF bytes
are authored from the inspected serde field/omission rules, not emitted or
round-tripped by a running model. Future old-reader serialization must match
these frozen bytes; a mismatch is retained and reviewed, never auto-updated.

Independent input: Bool F,F,T,T,F, labels one,two,three,four,five. Complete literal
output: [one,two], [three], [four,five], ordinal Int1/2/3. The new true member
belongs to its new group. Both ordinary JSON and full lossless typed trees are
authored; every Group has Unknown XML origin and all fields are ordered.

Prospective ordinary execution inventory: All and Primary, two calls per engine
only after authorization. No parsing/lowering/emission/evaluation or producer
was invoked to make these files. Historical generation remains parent-owned;
no artifact manifest exists yet and no old baseline is being rebased.
