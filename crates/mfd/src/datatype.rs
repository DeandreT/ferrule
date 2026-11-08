use ir::ScalarType;

/// The `.mfd` datatype name for a scalar type, as written in component and
/// function declarations.
pub(crate) const fn mfd_type_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "decimal",
        ScalarType::Bool => "boolean",
    }
}
