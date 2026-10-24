//! Best-effort repair of Windows-origin casing in static local input paths.

use std::path::Path;

pub(crate) fn resolve_static_input(mapping_path: &Path, stored: &str) -> String {
    if excluded(stored) {
        return stored.to_string();
    }
    let portable = stored.replace('\\', "/");
    let relative = Path::new(&portable);
    if relative.is_absolute() || windows_drive_path(&portable) {
        return stored.to_string();
    }
    let base = mapping_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if base.join(relative).exists() {
        return stored.to_string();
    }

    resolve_components(base, &portable).unwrap_or_else(|| stored.to_string())
}

fn excluded(stored: &str) -> bool {
    stored.trim().is_empty() || stored.contains("://") || stored.contains(['*', '?', '[', ']'])
}

fn windows_drive_path(portable: &str) -> bool {
    let bytes = portable.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn resolve_components(base: &Path, portable: &str) -> Option<String> {
    let mut current = base.to_path_buf();
    let mut resolved = Vec::new();
    for component in portable.split('/') {
        match component {
            "" => return None,
            "." | ".." => {
                current.push(component);
                resolved.push(component.to_string());
            }
            expected => {
                let direct = current.join(expected);
                if direct.exists() {
                    current = direct;
                    resolved.push(expected.to_string());
                    continue;
                }
                let matched = unique_case_insensitive_sibling(&current, expected)?;
                resolved.push(matched.clone());
                current.push(matched);
            }
        }
    }
    current.exists().then(|| resolved.join("/"))
}

fn unique_case_insensitive_sibling(parent: &Path, expected: &str) -> Option<String> {
    let mut matched = None;
    for entry in std::fs::read_dir(parent).ok()? {
        let name = entry.ok()?.file_name().into_string().ok()?;
        if !name.eq_ignore_ascii_case(expected) {
            continue;
        }
        if matched.is_some() {
            return None;
        }
        matched = Some(name);
    }
    matched
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Result<Self, std::io::Error> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "ferrule-mfd-instance-path-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path)?;
            Ok(Self(path))
        }

        fn mapping(&self) -> PathBuf {
            self.0.join("Design").join("mapping.mfd")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn repairs_each_uniquely_matched_component() -> Result<(), Box<dyn Error>> {
        let temp = TempDir::new()?;
        let mapping = temp.mapping();
        let base = mapping.parent().ok_or("mapping path has no parent")?;
        let data = base.join("Data");
        std::fs::create_dir_all(&data)?;
        let input = data.join("Orders-Custom.EDI");
        std::fs::write(&input, b"UNB")?;

        let stored = "data/orders-custom.edi";
        let resolved = resolve_static_input(&mapping, stored);
        if base.join(stored).is_file() {
            // Native case-insensitive lookup already resolves the stored path.
            assert_eq!(resolved, stored);
        } else {
            assert_eq!(resolved, "Data/Orders-Custom.EDI");
        }
        assert_eq!(
            std::fs::canonicalize(base.join(&resolved))?,
            std::fs::canonicalize(input)?
        );
        Ok(())
    }

    #[test]
    fn preserves_parent_components_and_returns_a_portable_relative_path()
    -> Result<(), Box<dyn Error>> {
        let temp = TempDir::new()?;
        let mapping = temp.mapping();
        let base = mapping.parent().ok_or("mapping path has no parent")?;
        std::fs::create_dir_all(base)?;
        let data = temp.0.join("Data");
        std::fs::create_dir_all(&data)?;
        let input = data.join("Orders.EDI");
        std::fs::write(&input, b"UNB")?;

        let stored = "..\\data\\orders.edi";
        let resolved = resolve_static_input(&mapping, stored);
        if base.join(stored.replace('\\', "/")).is_file() {
            assert_eq!(resolved, stored);
        } else {
            assert_eq!(resolved, "../Data/Orders.EDI");
        }
        assert_eq!(
            std::fs::canonicalize(base.join(resolved.replace('\\', "/")))?,
            std::fs::canonicalize(input)?
        );
        Ok(())
    }

    #[test]
    fn leaves_exact_and_missing_paths_unchanged() -> Result<(), Box<dyn Error>> {
        let temp = TempDir::new()?;
        let mapping = temp.mapping();
        let data = mapping
            .parent()
            .ok_or("mapping path has no parent")?
            .join("Data");
        std::fs::create_dir_all(&data)?;
        std::fs::write(data.join("Orders.EDI"), b"UNB")?;

        assert_eq!(
            resolve_static_input(&mapping, "Data\\Orders.EDI"),
            "Data\\Orders.EDI"
        );
        assert_eq!(
            resolve_static_input(&mapping, "data/missing.edi"),
            "data/missing.edi"
        );
        Ok(())
    }

    #[test]
    fn leaves_urls_absolute_paths_and_wildcards_unchanged() -> Result<(), Box<dyn Error>> {
        let temp = TempDir::new()?;
        let mapping = temp.mapping();
        for stored in [
            "https://example.test/Orders.EDI",
            "/var/data/orders.edi",
            r"C:\Data\Orders.EDI",
            "Data/*.edi",
            "Data/Orders?.edi",
            "Data/[Oo]rders.edi",
        ] {
            assert_eq!(resolve_static_input(&mapping, stored), stored);
        }
        Ok(())
    }

    #[test]
    fn does_not_search_outside_the_declared_relative_path() -> Result<(), Box<dyn Error>> {
        let temp = TempDir::new()?;
        let mapping = temp.mapping();
        let base = mapping.parent().ok_or("mapping path has no parent")?;
        std::fs::create_dir_all(base)?;
        let input = temp.0.join("Orders.EDI");
        std::fs::write(&input, b"UNB")?;

        assert_eq!(resolve_static_input(&mapping, "orders.edi"), "orders.edi");
        let resolved = resolve_static_input(&mapping, "../orders.edi");
        if base.join("../orders.edi").is_file() {
            assert_eq!(resolved, "../orders.edi");
        } else {
            assert_eq!(resolved, "../Orders.EDI");
        }
        assert_eq!(
            std::fs::canonicalize(base.join(&resolved))?,
            std::fs::canonicalize(input)?
        );
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rejects_ambiguous_case_insensitive_siblings() -> Result<(), Box<dyn Error>> {
        let temp = TempDir::new()?;
        let mapping = temp.mapping();
        let base = mapping.parent().ok_or("mapping path has no parent")?;
        std::fs::create_dir_all(base)?;
        std::fs::write(base.join("Orders.EDI"), b"UNB")?;
        std::fs::write(base.join("ORDERS.edi"), b"UNB")?;

        assert_eq!(resolve_static_input(&mapping, "orders.edi"), "orders.edi");
        Ok(())
    }

    #[test]
    fn import_repairs_primary_and_named_static_inputs_but_not_outputs() -> Result<(), Box<dyn Error>>
    {
        let temp = TempDir::new()?;
        let mapping = temp.mapping();
        let base = mapping.parent().ok_or("mapping path has no parent")?;
        std::fs::create_dir_all(base)?;
        std::fs::write(base.join("Primary.XML"), b"<Alpha/>")?;
        std::fs::write(base.join("Catalog.XML"), b"<Beta/>")?;
        std::fs::write(base.join("Result.XML"), b"<Target/>")?;
        std::fs::write(
            &mapping,
            r#"<mapping version="26" ferrule-primary-source="alpha">
  <component name="map"><structure><children>
    <component name="Alpha" uid="alpha" library="xml" kind="14"><data>
      <root><entry name="Alpha"><entry name="Value" outkey="10"/></entry></root>
      <document inputinstance="primary.xml" instanceroot="{}Alpha"/>
    </data></component>
    <component name="Beta" uid="beta" library="xml" kind="14"><data>
      <root><entry name="Beta"><entry name="Value" outkey="20"/></entry></root>
      <document inputinstance="catalog.xml" instanceroot="{}Beta"/>
    </data></component>
    <component name="Target" uid="target" library="xml" kind="14">
      <properties XSLTDefaultOutput="1"/><data>
        <root><entry name="Target"><entry name="Primary" inpkey="30"/><entry name="Named" inpkey="31"/></entry></root>
        <document outputinstance="result.xml" instanceroot="{}Target"/>
      </data>
    </component>
  </children><graph><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
    <vertex vertexkey="20"><edges><edge vertexkey="31"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#,
        )?;

        let imported = super::super::import(&mapping)?;
        let primary = if base.join("primary.xml").is_file() {
            "primary.xml"
        } else {
            "Primary.XML"
        };
        assert_eq!(imported.project.source_path.as_deref(), Some(primary));
        let [named] = imported.project.extra_sources.as_slice() else {
            return Err("fixture must import one named source".into());
        };
        let catalog = if base.join("catalog.xml").is_file() {
            "catalog.xml"
        } else {
            "Catalog.XML"
        };
        assert_eq!(named.path, catalog);
        assert_eq!(
            std::fs::canonicalize(base.join(primary))?,
            std::fs::canonicalize(base.join("Primary.XML"))?
        );
        assert_eq!(
            std::fs::canonicalize(base.join(&named.path))?,
            std::fs::canonicalize(base.join("Catalog.XML"))?
        );
        assert!(named.dynamic_path.is_none());
        assert_eq!(imported.project.target_path.as_deref(), Some("result.xml"));
        Ok(())
    }
}
