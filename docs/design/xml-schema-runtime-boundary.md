# XML schema and instance boundaries

`format-xml` keeps separate XSD/DTD compiler and XML instance modules behind its
existing public facade. Shared schema admission belongs to `declaration`, not
to either compiler or instance implementation. The compiler and codecs use the
same traversal for XML roles, occurrence metadata, defaults, expanded names and
namespace ambiguity. Its error order is preserved across these callers.

Occurrence, optional/required use and nil interpretation still derive from the
IR's effective XML view. Alternative and declaration data remain in `ir`;
`declaration` owns the common interpretation used by XSD import/export and instance
reading/writing. The public `XmlFormatError` and existing codec/compiler APIs are
unchanged.

No additional compiler crate is introduced. Runtime-only consumers include
`codegen-runtime`; authoring and host consumers such as MFD and CLI use both
schema compilation and instance I/O. The compiler and codecs currently share
`ir`, `roxmltree`, `quick-xml` and the public error contract, so another package
would not remove an external parser dependency without further API work.
The module boundary removes the compiler's dependency on private instance
helpers without adding indirection or claiming a measured build-speed benefit.

Run the `format-xml` suite to exercise compiler and codec behavior together.
`tests/shared_schema_contract.rs` checks matching rejection order and namespace
diagnostics through public XSD export and XML input/output entry points; existing
effective-declaration and roundtrip suites cover import, nil and alternatives.
