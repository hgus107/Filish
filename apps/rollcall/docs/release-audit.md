# Rollcall 0.1.2 Release Audit

Audit date: 2026-08-27

## Release Environment

- Build host: Apple Silicon (`arm64`)
- Build OS: macOS 26.5.2
- Supported package: Apple Silicon, macOS 12.0 or newer
- App shell: Tauri 2
- Frontend: Vite 6, TypeScript, bundled Inter
- Backend: Rust 1.98

## Automated Results

- Frontend unit tests: 17 passed, 0 failed
- Rust unit and integration-style tests: 20 passed, 0 failed
- TypeScript and Vite production build: passed
- Rust release build: passed
- npm dependency audit: 0 known vulnerabilities
- DMG checksum verification: passed
- App bundle code-signature integrity: passed
- Mach-O architecture: arm64
- Mach-O minimum OS: macOS 12.0
- Linked native libraries: Apple system frameworks and system libraries only

## UI And Button Matrix

| Control Or Flow | Positive | Negative | Boundary Or Edge | Result |
|---|---|---|---|---|
| Choose Files | Adds multiple files | Duplicate files show a clear notice | Empty picker return is harmless | Passed |
| Choose Folder | Recursively adds visible files | Re-importing the same folder is blocked | Hidden items, depth 8, and 20,000-file limits are reported | Passed |
| Add Text | Beginning and end text preview correctly | Both fields empty keeps New Name blank | Extensions remain unchanged | Passed |
| Replace Text | Matching names alone receive previews | Empty Find keeps the function incomplete | Empty Replace With removes found text; Find is always treated as literal text | Passed |
| Change Case | Lowercase, uppercase, and title case work | No case keeps New Name blank | Switching functions clears stale previews immediately | Passed |
| Add Numbers | Beginning and end positions work | No position keeps New Name blank | Start clamps to 0–999,999; digits clamp to 1–3 | Passed |
| Add Date | Today and Date Modified work | No position keeps New Name blank | All three date formats and both positions work | Passed |
| Original Name Sort | Natural ascending sort works | No queue is harmless | Second click reverses direction | Passed |
| New Name Sort | Natural ascending sort works | Incomplete previews remain safe | Second click reverses direction without changing rename order | Passed |
| Row Selection | Click, drag, Shift, Command, and arrows work | Empty selection cannot remove | Selection stops at first and last rows | Passed |
| Remove Selected | Removes only highlighted rows | Disabled with no selection | Delete and Backspace perform the same action | Passed |
| Clear | Clears queue and selection | Disabled for empty queue | Clears duplicate-folder memory and preview errors | Passed |
| Save As | Opens the approved confirmation | Disabled until a valid preview exists | Counts only names that will change | Passed |
| Save To | Accepts typed absolute paths | Relative and missing paths return clear errors | `~` and `~/…` expand to the current home folder | Passed |
| Change Folder | Uses the native macOS folder picker | Picker cancellation is harmless | Browser preview cannot trigger filesystem permission prompts | Passed |
| Cancel | Closes without saving | Repeated use is harmless | Escape and backdrop also close dialogs | Passed |
| Yes | Writes renamed copies | Existing targets, invalid names, and missing sources block the batch | Partial copies are rolled back after a failure | Passed |

## Findings Fixed

1. Removed the unused Undo Last Batch UI, commands, logs, dependency, and rename-in-place implementation.
2. Changed the localhost version to a read-only preview so browser upload/edit permission warnings cannot be triggered.
3. Made Save To directly writable while retaining the native Change Folder option.
4. Fixed stale New Name values that briefly remained when switching to an incomplete function.
5. Made destination writes exclusive with `create_new`, preventing an overwrite race between validation and copying.
6. Added rollback of every output created before a batch copy failure.
7. Added absolute-path validation plus `~` expansion for typed destination paths.
8. Added case-insensitive and Unicode-normalized collision checks against destination contents.
9. Removed remote Google Fonts access and bundled Inter locally for offline use.
10. Added a restrictive production Content Security Policy.
11. Removed obsolete overlapping backend behavior so Save As is the only write operation.
12. Added a reproducible macOS app and DMG packaging script with verification and optional Developer ID notarization.
13. Replaced the outdated README with the tested Rollcall behavior and Kiln-style repository flow.
14. Added the Rollcall tab and deep link to the existing Kiln GitHub Pages website.
15. Reduced the macOS menu bar to Rollcall → About Rollcall / Quit Rollcall and View → Toggle Full Screen, matching Kiln exactly.
16. Removed Advanced Pattern Matching from the interface and both preview engines while preserving literal Find/Replace behavior.

## Distribution Status

The public macOS release is signed with `Developer ID Application: Harish Gupta (V7SF7XDP3N)`, uses hardened runtime and a secure timestamp, and was accepted by Apple's notarization service. The notarization ticket is stapled to the DMG. Independent `codesign`, `spctl`, `stapler`, and `hdiutil` checks pass; Gatekeeper reports both the app and DMG as `Notarized Developer ID`.

The reproducible signed-release command is:

```sh
ROLLCALL_SIGNING_IDENTITY="Developer ID Application: Harish Gupta (V7SF7XDP3N)" \
ROLLCALL_NOTARY_PROFILE="RollcallNotary" \
npm run package:mac
```

Latest notarization submission: `ac0331f7-8673-4840-8f78-e3fa23b54b01` (Accepted, August 27, 2026).
