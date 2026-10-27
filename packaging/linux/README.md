# Linux desktop metadata

These files describe the existing native GUI executable; they do not build,
install, or package the application.

- Application ID: `io.github.DeandreT.Ferrule`
- Desktop executable: `ferrule-gui` (the GUI binary declared by
  `crates/gui/Cargo.toml`)
- No file associations or command-line launch arguments are declared because
  the GUI executable does not define a file-opening CLI contract.
- The SVG application mark is based on the Lucide Workflow icon design from
  [lucide-icons](https://github.com/lucide-icons/lucide), distributed under
  the ISC license. Lucide license: <https://github.com/lucide-icons/lucide/blob/main/LICENSE>.

The icon is installed under the same application ID used by the desktop entry
and AppStream component.
