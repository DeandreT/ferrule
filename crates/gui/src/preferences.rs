//! Versioned editor preferences stored by eframe, separate from mapping files.

use serde::{Deserialize, Serialize};

use crate::appearance::EditorAppearance;
use crate::theme::ThemeState;

const STORAGE_KEY: &str = "ferrule.editor_preferences";
// Additive appearance fields (including canvas density) migrate through serde
// defaults without discarding the user's existing theme and wire settings.
const CURRENT_VERSION: u32 = 1;
/// Most recently opened or saved projects kept for the start view.
pub const MAX_RECENT_PROJECTS: usize = 6;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorPreferences {
    version: u32,
    pub theme: ThemeState,
    pub appearance: EditorAppearance,
    pub show_minimap: bool,
    pub mfd_package_manifest: Option<String>,
    /// Project paths, most recent first.
    pub recent_projects: Vec<String>,
}

impl EditorPreferences {
    pub fn new(
        theme: ThemeState,
        appearance: EditorAppearance,
        show_minimap: bool,
        mfd_package_manifest: Option<String>,
    ) -> Self {
        Self {
            version: CURRENT_VERSION,
            theme,
            appearance,
            show_minimap,
            mfd_package_manifest,
            recent_projects: Vec::new(),
        }
    }

    pub fn with_recent_projects(mut self, recent_projects: Vec<String>) -> Self {
        self.recent_projects = recent_projects;
        self
    }
}

/// Moves `path` to the front of `recent`, dropping duplicates and the
/// oldest entries beyond [`MAX_RECENT_PROJECTS`].
pub fn remember_recent_project(recent: &mut Vec<String>, path: String) {
    recent.retain(|existing| *existing != path);
    recent.insert(0, path);
    recent.truncate(MAX_RECENT_PROJECTS);
}

impl Default for EditorPreferences {
    fn default() -> Self {
        Self::new(
            ThemeState::default(),
            EditorAppearance::default(),
            true,
            None,
        )
    }
}

pub fn load(storage: Option<&dyn eframe::Storage>) -> EditorPreferences {
    let Some(document) = storage.and_then(|storage| storage.get_string(STORAGE_KEY)) else {
        return EditorPreferences::default();
    };
    serde_json::from_str::<EditorPreferences>(&document)
        .ok()
        .filter(|preferences| preferences.version == CURRENT_VERSION)
        .unwrap_or_default()
}

pub fn store(storage: &mut dyn eframe::Storage, preferences: &EditorPreferences) {
    if let Ok(document) = serde_json::to_string(&preferences) {
        storage.set_string(STORAGE_KEY, document);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use eframe::Storage as _;

    use super::*;
    use crate::appearance::{AppearancePreset, CanvasDensity, WireColorMode, WireGeometry};
    use crate::theme::ThemePreference;

    #[derive(Default)]
    struct MemoryStorage(BTreeMap<String, String>);

    impl eframe::Storage for MemoryStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }

        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_string(), value);
        }

        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }

        fn flush(&mut self) {}
    }

    #[test]
    fn preferences_roundtrip_theme_and_wire_settings() {
        let mut preferences = EditorPreferences::new(
            ThemeState {
                preference: ThemePreference::Light,
                ..Default::default()
            },
            EditorAppearance::preset(AppearancePreset::Light),
            false,
            Some("/trusted/ferrule-package.json".to_string()),
        );
        let mut wire = *preferences.appearance.wire();
        wire.set_geometry(WireGeometry::Straight)
            .expect("straight wire geometry is valid");
        wire.set_color_mode(WireColorMode::UniquePerWire);
        preferences.appearance.set_wire(wire);
        preferences.appearance.set_density(CanvasDensity::Dense);

        let mut storage = MemoryStorage::default();
        store(&mut storage, &preferences);

        assert_eq!(load(Some(&storage)), preferences);
        assert_eq!(
            load(Some(&storage)).mfd_package_manifest.as_deref(),
            Some("/trusted/ferrule-package.json")
        );
    }

    #[test]
    fn missing_malformed_and_future_preferences_fall_back_atomically() {
        let mut storage = MemoryStorage::default();
        assert_eq!(load(Some(&storage)), EditorPreferences::default());

        storage.set_string(STORAGE_KEY, "not json".to_string());
        assert_eq!(load(Some(&storage)), EditorPreferences::default());

        storage.set_string(
            STORAGE_KEY,
            r#"{"version":99,"theme":{"preference":"light"}}"#.to_string(),
        );
        assert_eq!(load(Some(&storage)), EditorPreferences::default());
    }

    #[test]
    fn version_one_preferences_without_a_minimap_field_enable_it_by_default() {
        let mut storage = MemoryStorage::default();
        storage.set_string(STORAGE_KEY, r#"{"version":1}"#.to_string());

        assert!(load(Some(&storage)).show_minimap);
    }

    #[test]
    fn old_preferences_default_density_without_resetting_other_choices() {
        let mut storage = MemoryStorage::default();
        for appearance in [
            r#"{"wire":{"width":3.5}}"#,
            r#"{"density":"future_preset","wire":{"width":3.5}}"#,
        ] {
            storage.set_string(
                STORAGE_KEY,
                format!(
                    r#"{{"version":1,"theme":{{"preference":"light"}},"appearance":{appearance},"show_minimap":false,"mfd_package_manifest":"authored-package.json"}}"#,
                ),
            );
            let preferences = load(Some(&storage));
            assert_eq!(preferences.appearance.density(), CanvasDensity::Compact);
            assert_eq!(preferences.appearance.wire().width(), 3.5);
            assert_eq!(preferences.theme.preference, ThemePreference::Light);
            assert!(!preferences.show_minimap);
            assert_eq!(
                preferences.mfd_package_manifest.as_deref(),
                Some("authored-package.json")
            );
        }
    }

    #[test]
    fn recent_projects_roundtrip_most_recent_first_without_duplicates() {
        let mut recent = Vec::new();
        for index in 0..=MAX_RECENT_PROJECTS {
            remember_recent_project(&mut recent, format!("p{index}.json"));
        }
        remember_recent_project(&mut recent, "p3.json".to_string());
        assert_eq!(recent.len(), MAX_RECENT_PROJECTS);
        assert_eq!(recent[0], "p3.json");
        assert_eq!(recent[1], format!("p{MAX_RECENT_PROJECTS}.json"));
        assert!(!recent.contains(&"p0.json".to_string()));
        assert_eq!(recent.iter().filter(|path| *path == "p3.json").count(), 1);

        let preferences = EditorPreferences::default().with_recent_projects(recent.clone());
        let mut storage = MemoryStorage::default();
        store(&mut storage, &preferences);
        assert_eq!(load(Some(&storage)).recent_projects, recent);

        storage.set_string(STORAGE_KEY, r#"{"version":1}"#.to_string());
        assert!(load(Some(&storage)).recent_projects.is_empty());
    }

    #[test]
    fn every_density_persists_independently_of_document_data() {
        let mut storage = MemoryStorage::default();
        for density in CanvasDensity::ALL {
            let mut preferences = EditorPreferences::default();
            preferences.appearance.set_density(density);
            store(&mut storage, &preferences);
            assert_eq!(load(Some(&storage)), preferences);
        }
    }
}
