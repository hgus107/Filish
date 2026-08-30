import type { TaskFunction } from "./task-options.ts";

export type ImportReadiness = {
  task: TaskFunction | null;
  outputType: string;
  pageRanges: string;
  rotatePages: string;
  pickerOpen: boolean;
  importing: boolean;
};

export type ImportCandidate = {
  path: string;
  name: string;
};

export type RotatePageMode = "all" | "single" | "range";

export function allowedImportExtensions(task: TaskFunction | null): string[] {
  if (!task) {
    return ["doc", "docx", "html", "htm", "md", "markdown", "odt", "pdf", "rtf", "txt"];
  }
  if (task === "to-pdf") return ["doc", "docx", "html", "htm", "md", "markdown", "odt", "rtf", "txt"];
  return ["pdf"];
}

export function validPageSelection(value: string, allowAll = false): boolean {
  const normalized = value.trim();
  if (allowAll && normalized.toLowerCase() === "all") return true;
  if (!normalized) return false;

  const intervals: Array<[number, number]> = [];
  for (const rawToken of normalized.split(",")) {
    const match = rawToken.trim().match(/^([1-9]\d*)(?:\s*-\s*([1-9]\d*))?$/);
    if (!match) return false;
    const start = Number(match[1]);
    const end = Number(match[2] ?? match[1]);
    if (!Number.isSafeInteger(start) || !Number.isSafeInteger(end) || end < start) return false;
    intervals.push([start, end]);
  }

  intervals.sort((left, right) => left[0] - right[0] || left[1] - right[1]);
  return intervals.every((interval, index) => index === 0 || interval[0] > intervals[index - 1]![1]);
}

export function validRotatePageSelection(mode: RotatePageMode, value: string): boolean {
  const normalized = value.trim();
  if (mode === "all") return true;
  if (mode === "single") return /^([1-9]\d*)$/.test(normalized) && Number.isSafeInteger(Number(normalized));
  const match = normalized.match(/^([1-9]\d*)\s*-\s*([1-9]\d*)$/);
  if (!match) return false;
  const start = Number(match[1]);
  const end = Number(match[2]);
  return Number.isSafeInteger(start) && Number.isSafeInteger(end) && end >= start;
}

export function validImportDetails(
  task: TaskFunction | null,
  outputType: string,
  pageRanges: string,
  rotatePages: string,
  rotatePageMode: RotatePageMode = "all",
): boolean {
  if (task === "split" && outputType === "ranges") return validPageSelection(pageRanges);
  if (task === "rotate") return validRotatePageSelection(rotatePageMode, rotatePages);
  return true;
}

export function importIsReady(state: ImportReadiness): boolean {
  return !state.pickerOpen && !state.importing;
}

export function duplicateNotice(names: string[]): string {
  if (names.length === 1) return `${names[0]} is already in the queue and was not added again.`;
  return "One or more files are already in the queue and were not added.";
}

export function partitionImportCandidates<T extends ImportCandidate>(files: T[], existingPaths: Iterable<string>): {
  fresh: T[];
  duplicateNames: string[];
} {
  const seen = new Set(existingPaths);
  const fresh: T[] = [];
  const duplicateNames: string[] = [];
  for (const file of files) {
    if (seen.has(file.path)) duplicateNames.push(file.name);
    else {
      seen.add(file.path);
      fresh.push(file);
    }
  }
  return { fresh, duplicateNames };
}

export function clearIsEnabled(queueLength: number): boolean {
  return queueLength > 0;
}

export function removeSelectedIsEnabled(selectionCount: number): boolean {
  return selectionCount > 0;
}
