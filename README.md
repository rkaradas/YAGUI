# YAGUI – Yet Another Git User Interface

A friendly desktop Git client for Windows, macOS and Linux (Rust, egui). Every Git command is
available as a button or dialog, and YAGUI always shows which command it runs and what it does.

## Features

- History with branch graph, commit details with inline diffs, file tree
- Local changes as a folder tree: stage, unstage, stash, discard or commit whole folders
- All Git commands in a searchable, explained command view and in Quick Launch (Ctrl+P)
- Repository Manager, workspaces, tab list, statistics and treemap
- Branch, tag, stash, remote, rebase (interactive), cherry-pick, bisect, worktrees, submodules,
  Git Flow and Git LFS
- SSH keys, HTTPS accounts (credential helper), preferences and per-repository settings
- Light and dark theme, English and German

## Install

Download the file for your computer from the
[releases](https://github.com/rkaradas/YAGUI/releases):

| System | File | How |
|---|---|---|
| Windows | `YAGUI-Setup-<version>-windows-x64.exe` | Run it and click Install (no administrator rights). Uninstall via Settings > Apps or Help > Uninstall YAGUI. |
| Windows (portable) | `yagui-<version>-windows-x64.exe` | Run it from anywhere. |
| macOS | `YAGUI-<version>-macos-universal.dmg` | Drag YAGUI into Applications. Not notarized: macOS blocks the first start; then click "Open Anyway" in System Settings > Privacy & Security (macOS 14 and older: right-click > Open). |
| Debian/Ubuntu | `yagui_<version>_amd64.deb` | `sudo apt install ./yagui_<version>_amd64.deb` |
| Other Linux | `yagui-<version>-linux-x64.tar.gz` | Unpack, then `./yagui --install` (adds a menu entry and the `yagui` command). |

Git must be installed. Command line options: `yagui --help`.

## Updates

YAGUI looks for a new release once a day (Help > Check for Updates; can be turned off there).
"Download and install" replaces the program and restarts it. For a private repository, sign in
to GitHub under File > Accounts so YAGUI may read the releases.

## Versions and releases

The version is in `Cargo.toml` (Semantic Versioning); `yagui --version` and Help > About show it
with the commit it was built from. Changes are listed in `CHANGELOG.md`.

To publish a release:

1. Set the new version in `Cargo.toml`, add its section to `CHANGELOG.md`, commit.
2. Create the tag `v<version>` (e.g. `v1.2.0`) and upload the branch and the tag to GitHub.
3. GitHub Actions (`.github/workflows/release.yml`) builds Windows, Linux and macOS and posts
   the release with all files and checksums. Until all three builds have passed, the release
   stays a draft; after a failure, "Re-run failed jobs" continues (after moving the tag, start a
   new run instead). A published release is never changed; to build a version again, tag a new
   one. If the tag push starts no run, start it by hand: Actions > Release > Run workflow, and
   under "Use workflow from" pick the tag.

## Build

```
cargo build --release
```

The program is `target/release/yagui` (`yagui.exe` on Windows). Git must be installed and on the
PATH. On Linux the GTK 3 development files are needed (`libgtk-3-dev`).

`yagui --screenshot shot.png [--repo <folder>]` writes a picture of the window and exits.
