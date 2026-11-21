# YAGUI – Yet Another Git User Interface

A friendly desktop Git client for Windows (Rust, egui). Every Git command is available as a
button or dialog, and YAGUI always shows which command it runs and what it does.

## Features

- History with branch graph, commit details with inline diffs, file tree
- Local changes as a folder tree: stage, unstage, stash, discard or commit whole folders
- All Git commands in a searchable, explained command view and in Quick Launch (Ctrl+P)
- Repository Manager, workspaces, tab list, statistics and treemap
- Branch, tag, stash, remote, rebase (interactive), cherry-pick, bisect, worktrees, submodules,
  Git Flow and Git LFS
- SSH keys, HTTPS accounts (credential helper), preferences and per-repository settings
- Light and dark theme, English and German

## Build

```
cargo build --release
```

The program is `target/release/yagui.exe`. Git must be installed and on the PATH.

`yagui --screenshot shot.png [--repo <folder>]` writes a picture of the window and exits.
