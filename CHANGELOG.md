# Changelog 

All notable changes to YAGUI. Versions follow [Semantic Versioning](https://semver.org/):
MAJOR.MINOR.PATCH. The release workflow takes the notes for a version from this file.

## [Unreleased]

### Added
- macOS: "Open in" lists the editors and IDEs installed in Applications (Visual Studio Code, Cursor, Zed, Sublime Text, Xcode, JetBrains IDEs and more)
- Linux: "Open in" lists the editors whose launcher is on PATH (code, codium, subl, zed, JetBrains Toolbox scripts and more)
- macOS: ⌘R refreshes (F5 needs the Fn key on most Mac keyboards)

### Changed
- macOS: keyboard shortcuts use the Command key, and menus, tooltips and the Shortcuts window show ⌘, ⇧ and ⌥. Show HEAD is ⇧⌘H (⌘H hides the app); switching tabs stays on ⌃Tab
- Accounts: the credential helpers fit the system (macOS Keychain on macOS), and the dialog shows the helper in effect (also one set for the whole system, like Apple's Keychain helper or Git for Windows' Credential Manager). Choosing a helper now replaces those instead of being asked after them, and "none" really switches helpers off; its description says that YAGUI then cannot sign in over HTTPS, and "Save sign-in" waits for a helper

### Fixed
- macOS: Git, Git LFS and Git Flow from Homebrew or MacPorts are found when YAGUI is started from Finder or the Dock
- macOS: after an update from within the app, YAGUI.app gets the new version in its Info.plist and is signed again (it was left with a broken signature); an app updated by YAGUI 1.1.0 or older is signed again at its first start
- Ctrl+Shift+T (restore closed tab), Ctrl+Shift+Tab (previous tab), Ctrl+Alt+T (console) and Ctrl+Alt+O (file explorer) did what Ctrl+T, Ctrl+Tab and Ctrl+O do
- Ctrl+Q closed YAGUI at once, also while typing "@" with Ctrl+Alt+Q on German keyboards; quit with File > Exit (on macOS ⌘Q)
- The Pull menu entry showed Ctrl+Shift+P, which opens Find a command

## [1.1.0] - 2026-11-24

### Added
- Side-by-side diff: old lines on the left, new lines on the right, gaps hatched
- Diff toolbar at the top right of every diff (left to right): ignore whitespace, show invisible characters (· space, → tab, ¶ Windows line end), word wrap (in the single-column view), fewer and more lines around each change, the entire file, side by side. The choices are remembered
- "Reading repository…" in the status bar while a big repository is being read

### Changed
- Much smoother on slow computers: Git now runs in the background for the history, the local changes, commit details, diffs, path/content searches and jumping to a branch or tag, so the window no longer stops while it waits. Reading no longer rewrites Git's index, which used to make the folder watch reload again and again even when nothing was done. Tabs load when they are first shown; a replaced load stops its Git process
- Long diffs and very long lines stay fast: only the visible part is drawn
- Diffs of files opened with "Expand All" load a few at a time instead of all at once
- The local changes show what is really in the files: no text conversion (for example of PDF or Office files), so staging a hunk never writes converted text
- `YAGUI_PERF_LOG=<file>` writes a timing log (Git calls, commands, slow frames, graphics adapter) to track down stutter; Help > About shows the graphics adapter and warns when the computer draws without graphics acceleration

### Fixed
- Running the tests (`cargo test`) no longer removes the Start menu and desktop shortcuts (Linux: menu entry, icon and `yagui` command) of an installed YAGUI
- Update check for a private repository without a GitHub sign-in: it now says that GitHub gives no access (and to sign in under File > Accounts) instead of "No release has been published yet"
- Updates when YAGUI was installed from the .deb or sits in a folder you may not write to: the update dialog points to the release page instead of failing with "Permission denied"
- Staging a hunk of a file that isn't UTF-8 (for example Windows-1252 with umlauts) could replace its special characters with "�" in the commit; such hunks are no longer offered (stage the whole file)
- Discard Hunk could rewrite all line endings of a file whose line endings don't match core.autocrlf or .gitattributes (for example mixed CRLF and LF); it is no longer offered for such files
- A user's `apply.whitespace=fix` no longer changes the spaces of a staged or discarded hunk
- Hunk buttons wait while a command changes the repository, so a quick second click can't apply a hunk twice or in the wrong place
- Discard Hunk on a submodule's commit change did nothing; hunk buttons are no longer shown there
- Cancelling a command in the Activity window now stops it completely on Windows (Git for Windows' `git` starts the real git, which kept running)
- A renamed file added with "intent to add" (`git add -N`) showed up as a stray extra entry, or closed YAGUI when the old name had an umlaut
- Clicking an annotated tag in the sidebar now jumps to its commit; a tag and a branch with the same name are told apart
- The diff of a file whose name contains `[`, `]`, `*` or `?` no longer brings in other files that the name matches as a pattern
- Closing or moving a tab to the left of the active one no longer switches to another tab

## [1.0.0] - 2026-11-21

First release of YAGUI – Yet Another Git User Interface.

### History and changes
- Commit history with branch graph, labels for branches, tags and stashes, search by message, author, SHA, path or changed content
- Commit details with inline diffs ("Expand All"), changed files and the full file tree as folders
- Local changes as a folder tree or list: stage, unstage, stash, discard or commit single files, whole folders or single hunks
- Interactive rebase editor (reorder, reword, edit, squash, fixup, drop) with quick actions on any commit

### Every Git command, explained
- All Git commands as dialogs with options, shown as the exact command line before running
- A searchable "Git Commands" view and Quick Launch (Ctrl+P) for repositories and commands
- Activity log with the output of every command

### Repositories
- Repository Manager: recent repositories, repositories found in your source code folders, summary and statistics
- Workspaces, tab list, Treemap, Performance Benchmark
- Branches, tags, remotes, stashes, submodules, worktrees, Git Flow, Git LFS, bisect, patches
- Per-repository settings: user, main branch, no-fast-forward, commit template, Signed-off-by, issue tracker links, custom commands

### Sign-in and integration
- SSH keys (create, add to the agent, test), HTTPS accounts through Git's credential helper
- Open in console, file explorer or an editor; external merge and diff tools

### App
- English and German, light and dark theme, zoom
- Installer and uninstaller for the current user (no administrator rights), automatic update check
- Windows, Linux and macOS (Apple silicon and Intel)
