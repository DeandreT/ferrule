#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutClass {
    Wide,
    Compact,
    Narrow,
}

impl LayoutClass {
    pub fn from_width(width: f32) -> Self {
        if width >= 1_100.0 {
            Self::Wide
        } else if width >= 768.0 {
            Self::Compact
        } else {
            Self::Narrow
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WorkspacePane {
    Source,
    #[default]
    Canvas,
    Inspector,
}

impl WorkspacePane {
    pub const ALL: [Self; 3] = [Self::Source, Self::Canvas, Self::Inspector];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Source => "Source",
            Self::Canvas => "Canvas",
            Self::Inspector => "Inspector",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SideDock {
    Source,
    #[default]
    Inspector,
}

impl SideDock {
    pub const ALL: [Self; 2] = [Self::Source, Self::Inspector];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Source => "Source",
            Self::Inspector => "Inspector",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkspaceVisibility {
    pub source_dock: bool,
    pub inspector_dock: bool,
    pub center: WorkspacePane,
}

impl WorkspaceVisibility {
    pub fn resolve(
        class: LayoutClass,
        show_source: bool,
        show_inspector: bool,
        compact_dock_open: bool,
        compact_dock: SideDock,
        narrow_pane: WorkspacePane,
    ) -> Self {
        match class {
            LayoutClass::Wide => Self {
                source_dock: show_source,
                inspector_dock: show_inspector,
                center: WorkspacePane::Canvas,
            },
            LayoutClass::Compact => Self {
                source_dock: compact_dock_open && compact_dock == SideDock::Source,
                inspector_dock: compact_dock_open && compact_dock == SideDock::Inspector,
                center: WorkspacePane::Canvas,
            },
            LayoutClass::Narrow => Self {
                source_dock: false,
                inspector_dock: false,
                center: narrow_pane,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_breakpoints_preserve_a_useful_canvas() {
        assert_eq!(LayoutClass::from_width(1_100.0), LayoutClass::Wide);
        assert_eq!(LayoutClass::from_width(900.0), LayoutClass::Compact);
        assert_eq!(LayoutClass::from_width(767.0), LayoutClass::Narrow);
    }

    #[test]
    fn compact_layout_opens_only_the_selected_dock() {
        let visibility = WorkspaceVisibility::resolve(
            LayoutClass::Compact,
            true,
            true,
            true,
            SideDock::Source,
            WorkspacePane::Inspector,
        );
        assert!(visibility.source_dock);
        assert!(!visibility.inspector_dock);
        assert_eq!(visibility.center, WorkspacePane::Canvas);
    }

    #[test]
    fn narrow_layout_uses_the_selected_pane_as_its_whole_workspace() {
        let visibility = WorkspaceVisibility::resolve(
            LayoutClass::Narrow,
            true,
            true,
            true,
            SideDock::Inspector,
            WorkspacePane::Source,
        );
        assert!(!visibility.source_dock);
        assert!(!visibility.inspector_dock);
        assert_eq!(visibility.center, WorkspacePane::Source);
    }
}
