# To do

## macOS

- [ ] **Try YAGUI on a real Mac.** The release workflow builds the macOS version, runs
  `--version` and `--write-icon`, and draws the window once (step "Window test (macOS)", picture
  kept as the artifact "yagui-macos-window"). Still to check by hand:
  - first start from the disk image and from Applications, with "Open Anyway";
  - Git: with only the Command Line Tools, and with Git, Git LFS and Git Flow from Homebrew
    started from Finder;
  - shortcuts: ⌘T, ⌘W, ⌘O, ⌘P, ⇧⌘T, ⌃Tab / ⌃⇧Tab, ⇧⌘H, ⌥⌘T, ⌥⌘O, ⌘R, ⌘, and zoom
    (⌘+ / ⌘- / ⌘0), also on a German keyboard; the symbols ⌘ ⇧ ⌥ ⌃ show in the menus;
  - "Open in" with the installed editors; Accounts shows the macOS Keychain and switching to
    another helper works;
  - an update from within the app: afterwards Finder shows the new version and
    `codesign --verify --strict YAGUI.app` passes; whether macOS asks again for folder access;
  - an app updated from 1.1.0: at its first start the new version signs it again.
- [x] The in-app update broke the app's signature: YAGUI.app is now signed again after an update
  and gets the new version in its Info.plist; an app updated by 1.1.0 is repaired at its first
  start.
- [x] Shortcuts used Control instead of Command.
- [x] Git from Homebrew was not found when YAGUI was started from Finder or the Dock.
- [x] "Open in" found no editors on macOS; the Accounts dialog described Windows credential
  helpers.
- [x] Docs: README and release notes say that YAGUI also runs from the disk image or any folder,
  that Git needs the Command Line Tools (or Homebrew), and how "Open Anyway" works.

### Later

- [ ] **A stable signature.** YAGUI is signed ad hoc, so macOS treats every update as a new app:
  it asks again for access to folders such as Documents, and the first start needs "Open
  Anyway". A Developer ID certificate with notarization (paid Apple Developer Program) solves
  both; a self-signed certificate kept in the CI secrets would at least keep the permissions.
  Both only help together with "Update the whole app": today the in-app update signs YAGUI.app
  again ad hoc on the user's Mac (refresh_app in src/updater.rs), which would replace the CI
  signature at the first update.
- [ ] **Command Line Tools missing.** Without them, YAGUI's first Git call brings up Apple's
  install dialog without warning. Check `xcode-select -p` first and explain what to install.
- [ ] **Update the whole app.** Ship the signed YAGUI.app as the update archive and swap the whole
  app instead of the program inside it, without signing it again. Needs care: the updaters
  already out there (1.1.0 and this one) copy only the program out of the archive.
- [ ] **Names of the system's tools.** On macOS and Linux the labels still say "File Explorer" /
  "Datei-Explorer" and "Console" / "Konsole" (Finder, Terminal, the file manager there), and
  Preferences offers the Windows terminals (Windows Terminal, PowerShell, Git Bash), which
  "Open in Console" ignores there.

## Linux

- [x] "Open in" found no editors on Linux either: it now looks for `code`, `codium`, `subl`, `zed`
  and other launchers on PATH and in JetBrains Toolbox's scripts folder.
