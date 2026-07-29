use ir::{ScalarType, ScalarTypeSet};

/// The complete scalar domain of one schema leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarDomain {
    Single(ScalarType),
    Union(ScalarTypeSet),
}

impl ScalarDomain {
    pub fn label(self) -> String {
        match self {
            Self::Single(ty) => scalar_type_label(ty).to_string(),
            Self::Union(types) => types
                .iter()
                .map(scalar_type_label)
                .collect::<Vec<_>>()
                .join(" | "),
        }
    }

    /// Returns true when every possible source value is accepted by this
    /// target domain. Integer-to-float widening is the only implicit coercion.
    pub fn accepts_all_from(self, source: Self) -> bool {
        match source {
            Self::Single(ty) => self.accepts_source_type(ty),
            Self::Union(types) => types.iter().all(|ty| self.accepts_source_type(ty)),
        }
    }

    fn accepts_source_type(self, source: ScalarType) -> bool {
        self.contains(source) || source == ScalarType::Int && self.contains(ScalarType::Float)
    }

    fn contains(self, ty: ScalarType) -> bool {
        match self {
            Self::Single(single) => single == ty,
            Self::Union(types) => types.contains(ty),
        }
    }
}

pub fn scalar_type_label(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "String",
        ScalarType::Int => "Int",
        ScalarType::Float => "Float",
        ScalarType::Bool => "Bool",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn union(types: impl IntoIterator<Item = ScalarType>) -> ScalarDomain {
        let Some(types) = ScalarTypeSet::new(types) else {
            panic!("test union must contain distinct scalar types");
        };
        ScalarDomain::Union(types)
    }

    #[test]
    fn labels_follow_the_canonical_scalar_set_order() {
        assert_eq!(
            union([ScalarType::Bool, ScalarType::Int, ScalarType::String]).label(),
            "String | Int | Bool"
        );
    }

    #[test]
    fn compatibility_requires_the_complete_source_domain() {
        let string_int = union([ScalarType::String, ScalarType::Int]);
        let string_float = union([ScalarType::String, ScalarType::Float]);
        let all_numeric = union([ScalarType::Int, ScalarType::Float]);

        assert!(string_int.accepts_all_from(ScalarDomain::Single(ScalarType::String)));
        assert!(string_float.accepts_all_from(string_int));
        assert!(!string_int.accepts_all_from(string_float));
        assert!(ScalarDomain::Single(ScalarType::Float).accepts_all_from(all_numeric));
        assert!(!ScalarDomain::Single(ScalarType::Int).accepts_all_from(all_numeric));
    }
}
