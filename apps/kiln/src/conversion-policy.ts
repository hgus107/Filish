import { parseDimensions, type Dimensions } from "./dimension.ts";

export type ConvertibleState = "queued" | "working" | "done" | "failed" | "cancelled" | "saved";

export type ConvertibleRow = {
  path: string;
  state: ConvertibleState;
  hasSourceInfo: boolean;
  targetDimension?: string;
  output?: string;
  reconvert?: boolean;
};

export type ResizeRequest =
  | { mode: "original" }
  | { mode: "exact"; width: number; height: number };

export type ConversionJob = {
  path: string;
  resize: ResizeRequest;
};

export type ConversionPlan =
  | { kind: "missing-format" }
  | { kind: "unsaved-results" }
  | { kind: "no-convertible-files" }
  | { kind: "invalid-dimension"; path: string }
  | { kind: "ready"; jobs: ConversionJob[] };

const RETRYABLE_STATES = new Set<ConvertibleState>(["queued", "failed", "cancelled"]);

export function rowResize(targetDimension?: string): ResizeRequest | null {
  const value = targetDimension?.trim() ?? "";
  if (value === "") return { mode: "original" };
  const dimensions: Dimensions | null = parseDimensions(value);
  if (!dimensions) return null;
  if (dimensions.width < 1024 || dimensions.width > 7680) return null;
  if (dimensions.height < 1024 || dimensions.height > 7680) return null;
  return { mode: "exact", width: dimensions.width, height: dimensions.height };
}

export function convertibleRows(rows: Iterable<ConvertibleRow>): ConvertibleRow[] {
  return [...rows].filter((row) =>
    row.hasSourceInfo && (RETRYABLE_STATES.has(row.state) || (Boolean(row.reconvert) && (row.state === "done" || row.state === "saved")))
  );
}

export function reconversionPaths(
  rows: Iterable<ConvertibleRow>,
  selectedPaths: ReadonlySet<string>,
): Set<string> {
  return new Set([...rows]
    .filter((row) => selectedPaths.has(row.path) && row.hasSourceInfo && (row.state === "done" || row.state === "saved"))
    .map((row) => row.path));
}

export function conversionPlan(rows: Iterable<ConvertibleRow>, format: string): ConversionPlan {
  const queue = [...rows];
  if (format === "") return { kind: "missing-format" };
  const requested = convertibleRows(queue).filter((row) => row.reconvert);
  if (requested.length === 0 && queue.some((row) => Boolean(row.output) && row.state !== "saved")) {
    return { kind: "unsaved-results" };
  }
  const candidates = requested.length > 0 ? requested : convertibleRows(queue);
  if (candidates.length === 0) return { kind: "no-convertible-files" };

  const jobs: ConversionJob[] = [];
  for (const row of candidates) {
    const resize = rowResize(row.targetDimension);
    if (!resize) return { kind: "invalid-dimension", path: row.path };
    jobs.push({ path: row.path, resize });
  }
  return { kind: "ready", jobs };
}

export function convertButtonEnabled(
  rows: Iterable<ConvertibleRow>,
  format: string,
  running: boolean,
  starting: boolean,
  cancellationRequested: boolean,
): boolean {
  if (starting) return false;
  if (running) return !cancellationRequested;
  return conversionPlan(rows, format).kind === "ready";
}

export function compressionQuality(value: number): number {
  if (!Number.isFinite(value)) return 80;
  return Math.max(40, Math.min(100, Math.round(value)));
}

export function progressPresentation(
  status: "done" | "failed" | "skipped",
  error?: string,
): { state: "done" | "failed" | "cancelled"; detail: string; error?: string } {
  if (status === "done") return { state: "done", detail: "Converted" };
  if (status === "failed") return { state: "failed", detail: "Failed", error: error || "Unknown Error" };
  return { state: "cancelled", detail: "Cancelled" };
}
