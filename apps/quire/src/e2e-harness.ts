import { mockIPC } from "@tauri-apps/api/mocks";

type MockRequest = {
  runId: string;
  taskFunction: string;
  outputType: string;
  paths: string[];
  detail: string;
};

const cancelledRuns = new Set<string>();
const modes = new URLSearchParams(window.location.search);
let filePickerCalls = 0;

function hasMode(name: string): boolean {
  return modes.has(name);
}

const fixtures = [
  { path: "/fixtures/First.pdf", name: "First.pdf", bytes: 4096, extension: "pdf" },
  { path: "/fixtures/Second.pdf", name: "Second.pdf", bytes: 6144, extension: "pdf" },
  { path: "/fixtures/Notes.docx", name: "Notes.docx", bytes: 2048, extension: "docx" },
];

function mockOutputs(request: MockRequest) {
  const sources = fixtures.filter((file) => request.paths.includes(file.path));
  if (request.taskFunction === "extract") {
    const extensions = request.outputType === "both" ? ["md", "txt"] : [request.outputType === "markdown" ? "md" : "txt"];
    return sources.flatMap((source) => extensions.map((extension) => ({
      source: source.path,
      path: `/quire/run/${source.name.replace(/\.[^.]+$/, "")}.${extension}`,
      name: `${source.name.replace(/\.[^.]+$/, "")}.${extension}`,
      bytes: 1200,
    })));
  }
  if (request.taskFunction === "split") {
    const names = request.outputType === "ranges" ? ["pages-1-2", "pages-3-4"] : ["page-1", "page-2"];
    return names.map((name) => ({ source: sources[0]?.path ?? "", path: `/quire/run/${name}.pdf`, name: `${name}.pdf`, bytes: 1500 }));
  }
  if (request.taskFunction === "merge" || (request.taskFunction === "to-pdf" && request.outputType === "combined")) {
    return [{ source: "", path: "/quire/run/Combined Documents.pdf", name: "Combined Documents.pdf", bytes: 5000 }];
  }
  return sources.map((source) => ({
    source: source.path,
    path: `/quire/run/${source.name.replace(/\.[^.]+$/, "")}.pdf`,
    name: `${source.name.replace(/\.[^.]+$/, "")}.pdf`,
    bytes: 1800,
  }));
}

mockIPC((command, payload = {}) => {
  const args = payload as Record<string, unknown>;
  if (command === "plugin:dialog|open") {
    const options = args.options as { directory?: boolean; title?: string } | undefined;
    if (options?.directory && hasMode("directory-error")) throw new Error("Folder Picker Failed.");
    if (!options?.directory && hasMode("picker-error")) throw new Error("File Picker Failed.");
    if (hasMode("picker-cancel")) return null;
    if (options?.directory) return "/exports";
    if (hasMode("single-file")) return fixtures[0]!.path;
    if (hasMode("incremental")) {
      filePickerCalls += 1;
      return filePickerCalls === 1 ? fixtures[0]!.path : fixtures.slice(1).map((file) => file.path);
    }
    return fixtures.map((file) => file.path);
  }
  if (command === "collect_files") {
    if (hasMode("collect-error")) throw new Error("Collection Failed.");
    const paths = args.paths as string[];
    if (hasMode("empty-collection")) {
      return { files: [], ignored: 0, truncated: false, folderDepthLimited: false, unreadableFolders: 0 };
    }
    return {
      files: fixtures.filter((file) => paths.includes(file.path)),
      ignored: hasMode("collection-warnings") ? 2 : 0,
      truncated: hasMode("collection-warnings"),
      folderDepthLimited: hasMode("collection-warnings"),
      unreadableFolders: hasMode("collection-warnings") ? 1 : 0,
    };
  }
  if (command === "run_task") {
    const request = args.request as MockRequest;
    return new Promise((resolve, reject) => {
      window.setTimeout(() => {
        if (cancelledRuns.delete(request.runId)) reject("Conversion Cancelled.");
        else if (hasMode("multi-fail") && request.paths.includes("/fixtures/First.pdf")) reject("First.pdf Failed Deliberately.");
        else if (hasMode("multi-fail") && request.paths.includes("/fixtures/Second.pdf")) reject("Second.pdf Failed Deliberately.");
        else if (hasMode("mixed") && request.paths.includes("/fixtures/Second.pdf")) {
          reject("Second.pdf Could Not Be Converted.");
        }
        else resolve({ outputs: mockOutputs(request), warnings: [] });
      }, 1000);
    });
  }
  if (command === "save_outputs") {
    if (hasMode("save-error")) throw new Error("Destination Became Unavailable.");
    const destination = String(args.destination ?? "");
    if (!destination.startsWith("/")) throw new Error("Type an Absolute Destination Folder Path.");
    const paths = args.paths as string[];
    const overwrite = Boolean(args.overwrite);
    if ((destination.endsWith("Existing.pdf") || destination === "/existing-folder") && !overwrite) {
      const existing = destination.endsWith("Existing.pdf")
        ? destination
        : `${destination}/${paths[0]?.split("/").pop() ?? "output.pdf"}`;
      throw new Error(`FILE_EXISTS::${existing}`);
    }
    return {
      saved: paths.length === 1 && /\.[A-Za-z0-9]+$/.test(destination)
        ? [destination]
        : paths.map((path) => `${destination}/${path.split("/").pop()}`),
    };
  }
  if (command === "discard_outputs") {
    if (hasMode("discard-error")) throw new Error("Discard Failed.");
    return [];
  }
  if (command === "cancel_task") {
    if (hasMode("cancel-error")) throw new Error("Cancel Failed.");
    cancelledRuns.add(String(args.runId ?? ""));
    return null;
  }
  return undefined;
}, { shouldMockEvents: true });
