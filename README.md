# YoFile

Your file toolkit, in one app. Nothing leaves your Mac.

Website: [https://hgus107.github.io/YoFile/](https://hgus107.github.io/YoFile/)

Current release: **v0.1.1**

## Why this exists

Converting an image, renaming a folder of files, or turning a document into a PDF are all local jobs, yet the easy path for each one is a different ad-covered website that wants your file on its server. Three tasks, three uploads, three privacy policies nobody reads.

YoFile puts the three tools that already run locally into a single app. One download, one install, three tools, and nothing ever leaves the disk.

## What it does

- **Convert** — images between HEIC, AVIF, WebP, JPEG, PNG, and TIFF, in large local batches
- **Rename** — bulk-rename files with a live preview before anything is written
- **PDF** — convert documents to PDF, extract text, merge, split, rotate, compress, and OCR
- One window with three tools; each tool opens in its own focused workspace
- Everything runs on your Mac with no network code and no account

Each tool is the full, standalone app (Kiln, Rollcall, and Quire) bundled inside YoFile, so nothing is stripped down.

## What it avoids

| The web tools | YoFile |
|---|---|
| Your file is uploaded to a stranger's server | Nothing leaves the disk |
| A different site for every task | Convert, rename, and PDF in one app |
| File limits, queues, and watermarks | No caps, no queue, no watermark |
| Ads, popups, "upgrade to Pro" | None |
| Requires an account and an email address | No account |
| Needs an internet connection | Works offline |

## How to use

Download the signed, notarized Apple Silicon installer from the [latest release](https://github.com/hgus107/YoFile/releases/latest).

1. Open YoFile.
2. Pick a tool: Convert, Rename, or PDF.
3. The tool opens in its own window; do the job and save.

Originals are never modified or deleted.

## Tech stack

**Shell**
- [Tauri v2](https://tauri.app) — the YoFile launcher window and the three bundled tool apps. Uses the operating system's own webview, so the shell ships tiny.
- Frontend is Vite + TypeScript, no framework.

**Tools bundled**
- [Kiln](https://github.com/hgus107/kiln) — image conversion (libvips).
- [Rollcall](https://github.com/hgus107/rollcall) — bulk rename with preview.
- [Quire](https://github.com/hgus107/quire) — PDF creation, extraction, and processing (qpdf, Tesseract, Pandoc, LibreOffice, and Apple frameworks).

**How it works**

YoFile is a small launcher. Each tool is its own complete, Developer ID signed macOS app, bundled inside `YoFile.app/Contents/Resources/apps`. Choosing a tool opens that app. No tool code is rewritten; the launcher just brings them together into one install.

**Distribution**
- `npm run package:mac` builds `YoFile.app`, embeds the three prebuilt tool apps, signs, builds a `.dmg`, and names it for the machine architecture.
- Local development packages use ad-hoc signing. A public release requires `YOFILE_SIGNING_IDENTITY` plus `YOFILE_NOTARY_PROFILE`; the packaging script then enables hardened runtime, notarizes, and staples the ticket.
- The current minimum is macOS 12 on Apple Silicon. Intel, Windows, and Linux packages are not currently built or verified by this repository.

## License

MIT. Use it, modify it, repost it. All free.
