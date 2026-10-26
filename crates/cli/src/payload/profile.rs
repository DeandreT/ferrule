use std::path::Path;

/// The opt-in profile takes format identity from the declaration, never from an
/// uploaded filename or a host's output destination override. An extensionless
/// identity permits explicit format options but cannot guess a missing format.
#[derive(Clone, Copy, Default)]
pub(super) enum BoundaryProfile<'a> {
    #[default]
    Compatibility,
    BoundedTabular {
        declared_path: Option<&'a str>,
    },
}

impl<'a> BoundaryProfile<'a> {
    pub(super) fn new(bounded: bool, declared_path: Option<&'a str>) -> Self {
        if bounded {
            Self::BoundedTabular { declared_path }
        } else {
            Self::Compatibility
        }
    }

    pub(super) fn identity(self, logical_path: &'a Path) -> &'a Path {
        match self {
            Self::Compatibility => logical_path,
            Self::BoundedTabular { declared_path } => {
                declared_path.map_or(Path::new("declared-payload-boundary"), Path::new)
            }
        }
    }

    pub(super) fn bounded(self) -> bool {
        matches!(self, Self::BoundedTabular { .. })
    }
}
