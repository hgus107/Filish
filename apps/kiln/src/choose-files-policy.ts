export type ChooseFilesRow = {
  state: string;
  output?: string;
};

export function chooseFilesEnabled(
  rows: Iterable<ChooseFilesRow>,
  running: boolean,
  pickerOpen: boolean,
): boolean {
  if (running || pickerOpen) return false;
  return ![...rows].some((row) => Boolean(row.output) && row.state !== "saved");
}

export function uniqueNewPaths(paths: Iterable<string>, existing: ReadonlySet<string>): string[] {
  const unique = new Set<string>();
  for (const path of paths) {
    if (!existing.has(path)) unique.add(path);
  }
  return [...unique];
}

export type SelectionNotice =
  | { kind: "none" }
  | { kind: "no-supported" }
  | { kind: "unsupported"; count: number }
  | { kind: "limit"; ignored: number }
  | { kind: "folder-depth" }
  | { kind: "unreadable-folders"; count: number };

export function selectionNotice(
  inputCount: number,
  supportedCount: number,
  newCount: number,
  ignored: number,
  truncated: boolean,
  folderDepthLimited = false,
  unreadableFolders = 0,
): SelectionNotice {
  if (truncated) return { kind: "limit", ignored };
  if (folderDepthLimited) return { kind: "folder-depth" };
  if (unreadableFolders > 0) return { kind: "unreadable-folders", count: unreadableFolders };
  if (newCount === 0 && supportedCount === 0 && inputCount > 0) {
    return { kind: "no-supported" };
  }
  if (ignored > 0) return { kind: "unsupported", count: ignored };
  return { kind: "none" };
}
