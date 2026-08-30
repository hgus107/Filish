export type TaskFunction = "to-pdf" | "extract" | "merge" | "split" | "rotate" | "compress" | "ocr";

export type OutputChoice = {
  value: string;
  label: string;
};

const choices: Record<TaskFunction, OutputChoice[]> = {
  "to-pdf": [
    { value: "combined", label: "One PDF" },
    { value: "separate", label: "Separate PDFs" },
  ],
  extract: [
    { value: "markdown", label: "Markdown" },
    { value: "text", label: "Text" },
    { value: "both", label: "Markdown and Text" },
  ],
  merge: [{ value: "merged", label: "One Combined PDF" }],
  split: [
    { value: "individual", label: "Individual Pages" },
    { value: "ranges", label: "Page Ranges" },
  ],
  rotate: [
    { value: "clockwise", label: "90° Clockwise" },
    { value: "counterclockwise", label: "90° Counterclockwise" },
    { value: "half-turn", label: "180°" },
  ],
  compress: [
    { value: "screen", label: "Smallest" },
    { value: "standard", label: "Standard" },
    { value: "print", label: "Largest" },
  ],
  ocr: [{ value: "searchable", label: "Searchable PDF" }],
};

const pdfOnly = new Set<TaskFunction>(["extract", "merge", "split", "rotate", "compress", "ocr"]);

const completedStatus: Record<TaskFunction, string> = {
  compress: "Compressed",
  "to-pdf": "Converted",
  extract: "Extracted",
  ocr: "Searchable",
  merge: "Merged",
  rotate: "Rotated",
  split: "Done",
};

const failureAction: Record<TaskFunction, string> = {
  compress: "Compressed",
  "to-pdf": "Converted",
  extract: "Extracted",
  ocr: "Made Searchable",
  merge: "Merged",
  rotate: "Rotated",
  split: "Split",
};

const failureTitle: Record<TaskFunction, string> = {
  compress: "Compression Failed.",
  "to-pdf": "PDF Conversion Failed.",
  extract: "Extraction Failed.",
  ocr: "OCR Failed.",
  merge: "Merge Failed.",
  rotate: "Rotation Failed.",
  split: "Split Failed.",
};

export function outputChoices(task: TaskFunction): OutputChoice[] {
  return choices[task];
}

export function completedStatusLabel(task: TaskFunction): string {
  return completedStatus[task];
}

export function taskFailureFallback(task: TaskFunction): string {
  return failureTitle[task];
}

export function taskFailureSummary(task: TaskFunction, count: number): string {
  return `${count} Files Could Not Be ${failureAction[task]}.`;
}

export function acceptsExtension(task: TaskFunction, extension: string): boolean {
  const normalized = extension.toLowerCase().replace(/^\./, "");
  if (pdfOnly.has(task)) return normalized === "pdf";
  return ["doc", "docx", "odt", "rtf", "md", "markdown", "html", "htm", "txt"].includes(normalized);
}

export function conversionTargets<T extends { path: string; state: string }>(
  task: TaskFunction,
  compatible: T[],
  selectedPaths: Set<string>,
): T[] {
  const ready = compatible.filter((item) => item.state === "ready");
  const targeted = selectedPaths.size === 0
    ? ready
    : ready.filter((item) => selectedPaths.has(item.path));
  if (task === "split") return targeted.length === 1 ? targeted : [];
  if (task === "merge") return targeted.length >= 2 ? targeted : [];
  return targeted;
}

export function conversionIssue<T extends { path: string; state: string }>(
  task: TaskFunction,
  compatible: T[],
  selectedPaths: Set<string>,
): string {
  if (task === "to-pdf" && compatible.length === 0) {
    return "Add Non-PDF Files to Convert.";
  }
  const ready = compatible.filter((item) => item.state === "ready");
  const targeted = selectedPaths.size === 0
    ? ready
    : ready.filter((item) => selectedPaths.has(item.path));
  if (task === "split" && targeted.length !== 1) {
    return "Only One PDF Can Be Split at a Time. Please Select One PDF.";
  }
  if (task === "merge" && targeted.length < 2) {
    return "Select at least two ready PDFs to merge.";
  }
  if (targeted.length === 0) {
    return selectedPaths.size > 0
      ? "Select at least one ready file supported by this function."
      : "No ready files in the queue are supported by this function.";
  }
  return "";
}

export function outputLabel(task: TaskFunction, outputType: string): string {
  return choices[task].find((choice) => choice.value === outputType)?.label ?? "Choose a Type";
}

export function extensionLabel(extension: string): string {
  const normalized = extension.toLowerCase().replace(/^\./, "");
  if (normalized === "markdown") return "MD";
  return normalized.toUpperCase().slice(0, 4) || "FILE";
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 KB";
  const units = ["B", "KB", "MB", "GB"];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** index;
  const digits = index === 0 || value >= 10 || Number.isInteger(value) ? 0 : 1;
  return `${value.toFixed(digits)} ${units[index]}`;
}
