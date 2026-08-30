import { mockIPC } from "@tauri-apps/api/mocks";
import { previewBrowserRename, type BrowserFileEntry } from "./browser-preview.ts";
import type { RenameRules } from "./rule-builder.ts";

const fixtures: BrowserFileEntry[] = [
  { path: "/fixtures/IMG_10.JPG", name: "IMG_10.JPG", modifiedMs: new Date(2026, 7, 25).getTime() },
  { path: "/fixtures/IMG_2.JPG", name: "IMG_2.JPG", modifiedMs: new Date(2026, 7, 26).getTime() },
  { path: "/fixtures/notes.txt", name: "notes.txt", modifiedMs: new Date(2026, 7, 27).getTime() },
];

mockIPC((command, payload = {}) => {
  const args = payload as Record<string, unknown>;
  if (command === "plugin:dialog|open") {
    const options = args.options as { directory?: boolean; title?: string } | undefined;
    if (!options?.directory) return fixtures.map((file) => file.path);
    return options.title === "Choose A Destination Folder" ? "/exports" : "/fixtures/source";
  }

  if (command === "collect_files") {
    const paths = args.paths as string[];
    const files = paths.includes("/fixtures/source")
      ? fixtures
      : fixtures.filter((file) => paths.includes(file.path));
    return {
      files: files.map((file) => ({ ...file, bytes: 1024 })),
      skippedHidden: 0,
      truncated: false,
      folderDepthLimited: false,
      unreadableFolders: 0,
    };
  }

  if (command === "preview_save_as") {
    const paths = args.paths as string[];
    const rules = args.rules as RenameRules;
    return previewBrowserRename(fixtures.filter((file) => paths.includes(file.path)), rules);
  }

  if (command === "save_as") {
    const destination = String(args.destination ?? "");
    if (!destination.startsWith("/")) throw new Error("Type An Absolute Destination Folder Path.");
    const paths = args.paths as string[];
    const rules = args.rules as RenameRules;
    const result = previewBrowserRename(fixtures.filter((file) => paths.includes(file.path)), rules);
    return { count: result.changed, logPath: destination };
  }

  return undefined;
}, { shouldMockEvents: true });
