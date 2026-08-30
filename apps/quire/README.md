# Quire

Everything PDF & Free.

Website: [https://hgus107.github.io/Filish/?app=quire](https://hgus107.github.io/Filish/?app=quire)

Current release: **v0.1.1**

Create, extract, and manage PDFs on your laptop. No uploads, page limits, or watermarks.

## Why this exists

People paste tax returns, signed contracts, medical letters, and passports into free PDF websites every day, because they need a Word file turned into a PDF and that is where the search results point. The file is uploaded, converted on a server, and held there for some number of hours according to a privacy policy nobody reads.

None of that is necessary. Quire runs these conversions locally through macOS PDFKit, qpdf, Tesseract, Pandoc, and LibreOffice.

Quire is a plain interface over those tools, running locally, with nothing to sign up for.

## What it does

- Converts Word, ODT, RTF, Markdown, HTML, and plain text into PDF
- Creates a separate PDF for every source or one combined PDF in an order you choose
- Extracts each PDF to Markdown, plain text, or both
- Keeps headings, reading order, tables, captions, and figure references where the source allows it; image data is omitted from text exports
- Merges, splits, rotates, and compresses PDFs
- Converts multiple selected files or an entire folder in one pass
- Makes scanned PDFs searchable and selectable with OCR

## What it avoids

| The free PDF sites | Quire |
|---|---|
| Your contract is uploaded to a stranger's server | Nothing leaves the disk |
| 2 free tasks per hour, then wait or pay | No caps, no clock |
| 15 MB file limit | Whatever your disk holds |
| Watermark on the free tier | No watermark |
| Requires an account and an email address | No account |
| Needs an internet connection | Works on a plane |

## How to use

Download the signed, notarized Apple Silicon installer from the [latest release](https://github.com/hgus107/quire/releases/latest).

1. Choose a function and, when shown, an output type.
2. Import files, or drag files or a folder onto the window.
3. Reorder sources when creating one combined PDF.
4. Press Apply, review the result status, then press Save.

Originals are never modified or deleted.

## Tech stack

**Shell**

- [Tauri v2](https://tauri.app) — desktop shell, IPC bridge, installer bundler.
- Frontend is Vite + TypeScript, no framework.

**Backend**

- [Rust](https://www.rust-lang.org) — orchestrates the conversion tools, manages the queue, and owns every path the user's files travel along.
- [Pandoc](https://pandoc.org) — prepares Markdown documents for PDF conversion.
- [LibreOffice](https://www.libreoffice.org) in headless mode — converts Office and text document formats to PDF.
- macOS PDFKit, Core Graphics, and ImageIO — page counting, text extraction, OCR rendering, and three-level PDF compression without Poppler or Ghostscript.
- [`qpdf`](https://qpdf.sourceforge.io) — merging, splitting, page-range extraction, and rotation.
- [Tesseract](https://github.com/tesseract-ocr/tesseract) — OCR for scanned pages that carry no text layer.
- `serde` — typed messages across the Rust/TypeScript boundary.

**How a conversion actually runs**

Rust picks a route based on the input and requested output: document conversion uses LibreOffice or Pandoc, structural PDF work uses qpdf, and native PDF processing uses Apple frameworks. Work is staged in Quire's scratch directory and nothing is passed over a network socket.

**Distribution**

- `npm run package:mac` builds and verifies `Quire.app` plus an Apple Silicon `.dmg`, bundling qpdf, Tesseract, Pandoc, LibreOffice, OCR language data, and required native libraries.
- Local packages use ad-hoc signing. A public release requires `QUIRE_SIGNING_IDENTITY` plus `QUIRE_NOTARY_PROFILE`; the script then enables hardened runtime signing, notarizes the DMG, and staples the ticket.
- The current minimum is macOS 12 on Apple Silicon. Intel, Windows, and Linux packages are not currently built or verified by this repository.

## License

MIT.
