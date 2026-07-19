//! Versioned editor preferences stored by eframe, separate from mapping files.

use serde::{Deserialize, Serialize};

use crate::appearance::EditorAppearance;
use crate::theme::ThemeState;

const STORAGE_KEY: &str = "ferrule.editor_preferences";
const CURRENT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorPreferences {
    version: u32,
    pub theme: ThemeState,
    pub appearance: EditorAppearance,
}

impl EditorPreferences {
    pub const fn new(theme: ThemeState, appearance: EditorAppearance) -> Self {
        Self {
            version: CURRENT_VERSION,
            theme,
            appearance,
        }
    }
}

impl Default for EditorPreferences {
    fn default() -> Self {
        Self::new(ThemeState::default(), EditorAppearance::default())
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

pub fn store(storage: &mut dyn eframe::Storage, preferences: EditorPreferences) {
    if let Ok(document) = serde_json::to_string(&preferences) {
        storage.set_string(STORAGE_KEY, document);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use eframe::Storage as _;

    use super::*;
    use crate::appearance::{AppearancePreset, WireColorMode, WireGeometry};
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
        );
        let mut wire = *preferences.appearance.wire();
        wire.set_geometry(WireGeometry::Straight)
            .expect("straight wire geometry is valid");
        wire.set_color_mode(WireColorMode::UniquePerWire);
        preferences.appearance.set_wire(wire);

        let mut storage = MemoryStorage::default();
        store(&mut storage, preferences);

        assert_eq!(load(Some(&storage)), preferences);
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
}
