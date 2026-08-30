import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";

const requestedQueueSize = Number(new URLSearchParams(window.location.search).get("large") ?? 0);
const fixturePaths = requestedQueueSize > 0
  ? Array.from({ length: requestedQueueSize }, (_, index) => `/fixtures/Image-${String(index + 1).padStart(4, "0")}.jpg`)
  : ["/fixtures/Alpha.jpg", "/fixtures/Beta.png", "/fixtures/Corrupt.jpg"];
let conversionRun = 0;
let cancellationRequested = false;
let saveAttempt = 0;
let collisionCheck = 0;

mockIPC(async (command, payload = {}) => {
  if (command === "plugin:dialog|open") {
    const options = (payload as { options?: { directory?: boolean } }).options;
    return options?.directory ? "/exports" : fixturePaths;
  }
  if (command === "collect_images") {
    return {
      paths: fixturePaths,
      ignored: 0,
      truncated: false,
      folderDepthLimited: false,
      unreadableFolders: 0,
    };
  }
  if (command === "probe_files") {
    if (requestedQueueSize > 0) {
      return fixturePaths.map((path, index) => ({
        kind: "ok",
        path,
        name: path.split("/").pop() ?? path,
        bytes: 4_096 + index,
        width: 1600,
        height: 1200,
        hasMetadata: index % 2 === 0,
      }));
    }
    return [
      { kind: "ok", path: fixturePaths[0], name: "Alpha.jpg", bytes: 4_096, width: 1600, height: 1200, hasMetadata: true },
      { kind: "ok", path: fixturePaths[1], name: "Beta.png", bytes: 8_192, width: 2048, height: 1536, hasMetadata: false },
      { kind: "failed", path: fixturePaths[2], error: "Corrupt Image" },
    ];
  }
  if (command === "scratch_dir") return `/tmp/kiln-e2e/run-${conversionRun + 1}`;
  if (command === "convert_batch") {
    conversionRun += 1;
    cancellationRequested = false;
    const jobs = (payload as { jobs?: Array<{ path: string }> }).jobs ?? [];
    window.setTimeout(async () => {
      for (const [index, job] of jobs.entries()) {
        if (conversionRun > 1 && cancellationRequested && index > 0) {
          await emit("conversion-progress", { status: "skipped", path: job.path });
        } else {
          await emit("conversion-progress", {
            status: "done",
            path: job.path,
            output: `/tmp/kiln-e2e/run-${conversionRun}/${index === 0 ? "Alpha.jpg" : "Beta.jpg"}`,
            bytes: index === 0 ? 2_048 : 3_072,
          });
        }
        if (conversionRun > 1) await new Promise((resolve) => window.setTimeout(resolve, 250));
      }
      await emit("conversion-finished", null);
    }, conversionRun > 1 ? 800 : 0);
    return null;
  }
  if (command === "cancel_batch") {
    cancellationRequested = true;
    return null;
  }
  if (command === "existing_targets") {
    collisionCheck += 1;
    const paths = (payload as { paths?: string[] }).paths ?? [];
    return collisionCheck >= 3 ? paths.slice(0, 1) : [];
  }
  if (command === "save_files") {
    saveAttempt += 1;
    const copies = (payload as { copies?: Array<{ from: string; to: string }> }).copies ?? [];
    return copies.map((copy, index) => saveAttempt === 1 && index === copies.length - 1
      ? { status: "failed", from: copy.from, error: "Destination Unavailable" }
      : { status: "ok", from: copy.from, path: copy.to });
  }
  if (command === "discard_files") return [];
  return null;
}, { shouldMockEvents: true });
