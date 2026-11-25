# To do

## macOS

- [ ] **Try YAGUI on a real Mac.** The release workflow only builds the macOS version and runs
  `--version` and `--write-icon`. The window, the Git integration, the first-start approval and
  the in-app update have never been checked on a Mac.
- [ ] **The in-app update breaks the app's signature.** On macOS the update replaces
  `YAGUI.app/Contents/MacOS/yagui` with the bare program from `yagui-<v>-macos-universal.tar.gz`
  (`src/updater.rs`, `install`), so the ad-hoc signature of the app no longer matches, and
  `yagui.old` stays inside the app until the next start. Ship the signed app in the update
  archive and replace the whole app, or sign it again after the update
  (`codesign --force --deep -s -`).
- [ ] **Shortcuts use Control instead of Command.** The shortcuts are defined with
  `Modifiers::CTRL` (only commit uses ⌘Enter), so on a Mac they need ⌃ instead of ⌘, and the
  menus and the Shortcuts window show "Ctrl+…". Use `Modifiers::COMMAND` (⌘ on macOS, Ctrl
  elsewhere) and show ⌘ on macOS.
- [ ] **Git from Homebrew is not found when YAGUI is started from Finder or the Dock.** Such apps
  only get `/usr/bin:/bin:/usr/sbin:/sbin`, so YAGUI always runs Apple's Git (which needs the
  Command Line Tools) and never finds `git-lfs` or `git-flow` from Homebrew. Add
  `/opt/homebrew/bin` and `/usr/local/bin` to `PATH` for Git calls on macOS.
- [ ] **Smaller points:** "Open in" finds no editors on macOS (it only looks in Windows folders);
  the Accounts dialog describes Windows credential helpers.
- [ ] **Docs:** say in the README and the release notes that YAGUI also runs straight from the
  disk image or from any folder, that Git needs the Command Line Tools (`xcode-select --install`),
  and that "Open Anyway" is offered for about an hour after the blocked start and asks for the
  login password.
