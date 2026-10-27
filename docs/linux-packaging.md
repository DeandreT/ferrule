# Linux desktop metadata

Ferrule provides desktop-entry and AppStream metadata for the existing native
GUI executable. These files are metadata only: they do not produce a binary
release, install system packages, register file associations, or change the
desktop environment.

## Build and validate

From the repository root, build the GUI using the pinned toolchain:

```sh
cargo build --locked --release -p gui
```

Validate the checked-in metadata with the standard desktop and AppStream
validators:

```sh
desktop-file-validate packaging/linux/io.github.DeandreT.Ferrule.desktop
appstreamcli validate --pedantic packaging/linux/io.github.DeandreT.Ferrule.metainfo.xml
```

The desktop entry uses `ferrule-gui`, the executable name declared by the GUI
crate. It intentionally declares no `MimeType`, desktop actions, or command
line arguments: the GUI does not currently expose a file-opening CLI contract.
The command-line tool is a separate executable.

## Manual user-local staging

The following commands install files only in the current user's home directory.
They do not use a package manager, modify system files, alter shell profiles, or
change default applications. Run them from the repository root after building:

```sh
install -Dm755 target/release/ferrule-gui "$HOME/.local/bin/ferrule-gui"
install -Dm644 packaging/linux/io.github.DeandreT.Ferrule.svg "$HOME/.local/share/icons/hicolor/scalable/apps/io.github.DeandreT.Ferrule.svg"
install -Dm644 packaging/linux/io.github.DeandreT.Ferrule.metainfo.xml "$HOME/.local/share/metainfo/io.github.DeandreT.Ferrule.metainfo.xml"
desktop_entry=$(mktemp)
trap 'rm -f "$desktop_entry"' EXIT
sed "s|^Exec=ferrule-gui$|Exec=$HOME/.local/bin/ferrule-gui|" \
  packaging/linux/io.github.DeandreT.Ferrule.desktop > "$desktop_entry"
install -Dm644 "$desktop_entry" "$HOME/.local/share/applications/io.github.DeandreT.Ferrule.desktop"
desktop-file-validate "$HOME/.local/share/applications/io.github.DeandreT.Ferrule.desktop"
rm "$desktop_entry"
trap - EXIT
```

The staged desktop entry uses the absolute path to the user-local executable so
launching it does not depend on the desktop session's `PATH`. The temporary
file is uniquely created and removed after installation. To uninstall this
manual staging, remove only the four listed files from `~/.local/bin`,
`~/.local/share/icons/hicolor/scalable/apps`,
`~/.local/share/metainfo`, and `~/.local/share/applications` respectively.
