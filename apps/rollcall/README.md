# Rollcall

New names first. Original files stay untouched.

Website: [https://hgus107.github.io/Filish/?app=rollcall](https://hgus107.github.io/Filish/?app=rollcall)

Current release: **v0.1.2**

## Why this exists

Renaming a few files is easy. Renaming hundreds consistently is where Finder becomes slow, error-prone, and difficult to review.

Rollcall turns that job into a visible queue. Rename all your files in one go with five different naming patterns. It is fast, interactive, private, safe, and free. Processing stays on the Mac and the selected originals are never modified.

## What it does

- Adds text at the beginning, at the end, or both
- Finds and replaces text, including removal when Replace With is empty
- Changes filename text to lowercase, uppercase, or title case
- Adds sequential numbers at the beginning or end with `1`, `01`, or `001` formatting
- Adds today's date, the file-modified date, or a custom date at the beginning or end in three formats
- Shows Original Name, New Name, and Status before Save As is enabled
- Sorts by Original Name or New Name in either direction
- Supports mouse, Shift, Command, arrow-key, and Delete-key queue selection
- Imports individual files or folders while rejecting duplicate selections
- Collects up to 20,000 visible files per selection, skips hidden items, and limits recursive folder depth to eight levels
- Writes renamed copies to a chosen or typed destination folder; originals remain untouched
- Rejects duplicate, occupied, invalid, cross-platform-incompatible, and over-255-byte filenames

## What it avoids

| Manual or web renaming | Rollcall |
|---|---|
| Rename files one at a time | Preview a complete batch |
| Discover collisions halfway through | Block conflicts before saving |
| Upload filenames or files to a service | All processing stays local |
| Risk changing the originals | Save renamed copies to another folder |
| Learn patterns for basic tasks | Pick a plain-language function |
| Guess which files changed | Read Original Name, New Name, and Status together |

## How to use

Download the Apple Silicon installer from the [latest release](https://github.com/hgus107/rollcall/releases/latest).

1. Add individual files or choose a folder.
2. Choose Add Text, Replace Text, Change Case, Add Numbers, or Add Date.
3. Review every populated New Name and resolve any Status warning.
4. Select Save As, then choose or type an absolute destination folder path.
5. Confirm Yes. Rollcall writes renamed copies and leaves every original unchanged.

The localhost browser preview is intentionally read-only. File and folder actions are available in the Rollcall desktop app, which avoids browser upload/edit permission warnings.

## Tech stack

**Shell**

- [Tauri v2](https://tauri.app) — native macOS window, system file pickers, IPC bridge, and installer bundling
- Vite plus TypeScript — framework-free interface and typed presentation logic
- Inter — bundled locally so the interface does not contact a font service

**Backend**

- [Rust](https://www.rust-lang.org) — folder collection, validation, previews, and file copying
- `regex` — internal validation of supported filename tokens
- `chrono` — Today and Date Modified formatting
- `unicode-normalization` — case-insensitive, Unicode-normalized collision checks
- `walkdir` — bounded recursive folder collection
- `serde` — typed messages across the Rust/TypeScript boundary

**How Save As runs**

The frontend sends selected paths and one rename rule to Rust. Rust validates the rule and every proposed filename, detects duplicate output names, verifies the destination, and checks existing names without overwriting them. It then creates each output exclusively, copies bytes and source permissions, syncs the result, and removes any partial outputs if the batch fails. The original paths are never renamed or deleted.

**Distribution**

- `npm run package:mac` builds and verifies `Rollcall.app` plus an Apple Silicon `.dmg`.
- Local packages use ad-hoc signing. A public release requires `ROLLCALL_SIGNING_IDENTITY` plus `ROLLCALL_NOTARY_PROFILE`; the script then enables hardened runtime signing, notarizes the DMG, and staples the ticket.
- The current minimum is macOS 12 on Apple Silicon. Intel, Windows, and Linux packages are not currently built or verified by this repository.

## License

MIT.
