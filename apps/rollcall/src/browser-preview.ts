import type { RenameRules } from "./rule-builder.ts";

export type BrowserFileEntry = {
  path: string;
  name: string;
  modifiedMs?: number;
};

export type BrowserPreviewItem = {
  source: string;
  target: string;
  originalName: string;
  proposedName: string;
  status: "ready" | "unchanged" | "conflict" | "invalid" | "missing";
  message: string;
  matched: boolean;
};

export type BrowserRenamePreview = {
  items: BrowserPreviewItem[];
  ready: boolean;
  changed: number;
  conflicts: number;
  invalid: number;
  unchanged: number;
};

function splitName(name: string): { stem: string; extension: string } {
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || dot === name.length - 1) return { stem: name, extension: "" };
  return { stem: name.slice(0, dot), extension: name.slice(dot + 1) };
}

function applyCase(value: string, style: string): string {
  switch (style) {
    case "keep":
      return value;
    case "lower":
      return value.toLocaleLowerCase();
    case "upper":
      return value.toLocaleUpperCase();
    case "title":
      return value
        .toLocaleLowerCase()
        .replace(/(^|[ _-])(\p{L})/gu, (_match, separator: string, letter: string) => `${separator}${letter.toLocaleUpperCase()}`);
    default:
      throw new Error("Choose A Supported Letter Case.");
  }
}

function validateFilename(name: string): string | null {
  if (name === "" || name === "." || name === "..") return "A Filename Cannot Be Empty, '.' Or '..'.";
  if (new TextEncoder().encode(name).length > 255) return "The New Filename Is Longer Than 255 Bytes.";
  if (/[\u0000-\u001f/\\<>:"|?*]/u.test(name)) return "The New Filename Contains An Unsupported Character.";
  if (/[ .]$/u.test(name)) return "A Filename Cannot End With A Space Or Period.";
  const stem = name.split(".", 1)[0].toLocaleUpperCase();
  if (/^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])$/u.test(stem)) return "That Filename Is Reserved By Windows.";
  return null;
}

function formattedDate(file: BrowserFileEntry, rules: RenameRules): string {
  if (rules.dateSource === "custom") {
    const [year, month, day] = rules.customDate.split("-");
    if (rules.dateFormat === "mm-dd-yyyy") return `${month}-${day}-${year}`;
    if (rules.dateFormat === "dd-mm-yyyy") return `${day}-${month}-${year}`;
    return `${year}-${month}-${day}`;
  }
  const date = rules.dateSource === "modified" && file.modifiedMs !== undefined ? new Date(file.modifiedMs) : new Date();
  const year = String(date.getFullYear()).padStart(4, "0");
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  if (rules.dateFormat === "mm-dd-yyyy") return `${month}-${day}-${year}`;
  if (rules.dateFormat === "dd-mm-yyyy") return `${day}-${month}-${year}`;
  return `${year}-${month}-${day}`;
}

function matchesFind(file: BrowserFileEntry, rules: RenameRules): boolean {
  if (rules.find === "") return true;
  const { stem } = splitName(file.name);
  return stem.includes(rules.find);
}

function nameFor(file: BrowserFileEntry, index: number, rules: RenameRules): string {
  const { stem, extension } = splitName(file.name);
  let changedStem = stem;
  if (rules.find !== "") {
    changedStem = changedStem.split(rules.find).join(rules.replace);
  }
  changedStem = applyCase(changedStem, rules.caseStyle);
  const count = String(Math.min(Number.MAX_SAFE_INTEGER, rules.counterStart + index)).padStart(rules.counterPadding, "0");
  const date = formattedDate(file, rules);
  const unknownToken = (rules.template.match(/\{[^{}]+\}/gu) ?? []).find(
    (token) => !["{name}", "{n}", "{ext}", "{date}"].includes(token),
  );
  if (unknownToken) {
    throw new Error(`Unknown Pattern Token: ${unknownToken}`);
  }
  let output = rules.template.replace(/\{(name|n|ext|date)\}/gu, (_token, name: string) => {
    if (name === "name") return changedStem;
    if (name === "n") return count;
    if (name === "date") return date;
    return extension;
  });
  if (!rules.template.includes("{ext}") && extension !== "") output += `.${extension}`;
  return output;
}

function parentOf(path: string): string {
  const normalized = path.replace(/\\/gu, "/");
  const slash = normalized.lastIndexOf("/");
  return slash < 0 ? "" : normalized.slice(0, slash);
}

function targetPath(path: string, proposedName: string): string {
  const parent = parentOf(path);
  return parent === "" ? proposedName : `${parent}/${proposedName}`;
}

function collisionKey(path: string): string {
  return path.normalize("NFC").toLocaleLowerCase();
}

export function previewBrowserRename(files: BrowserFileEntry[], rules: RenameRules): BrowserRenamePreview {
  const items: BrowserPreviewItem[] = files.map((file, index) => {
    const proposedName = nameFor(file, index, rules);
    const matched = matchesFind(file, rules);
    const target = targetPath(file.path, proposedName);
    const invalid = validateFilename(proposedName);
    if (invalid) {
      return { source: file.path, target, originalName: file.name, proposedName, status: "invalid", message: invalid, matched };
    }
    if (proposedName === file.name) {
      return {
        source: file.path,
        target,
        originalName: file.name,
        proposedName,
        status: "unchanged",
        message: "This Name Is Unchanged.",
        matched,
      };
    }
    return {
      source: file.path,
      target,
      originalName: file.name,
      proposedName,
      status: "ready",
      message: "Ready To Save.",
      matched,
    };
  });

  const counts = new Map<string, number>();
  for (const item of items) {
    if (item.status !== "ready") continue;
    const key = collisionKey(item.proposedName);
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  for (const item of items) {
    if (item.status === "ready" && (counts.get(collisionKey(item.proposedName)) ?? 0) > 1) {
      item.status = "conflict";
      item.message = "More Than One File Would Receive This Name.";
    }
  }

  const changed = items.filter((item) => item.status === "ready").length;
  const conflicts = items.filter((item) => item.status === "conflict").length;
  const invalid = items.filter((item) => item.status === "invalid").length;
  const unchanged = items.filter((item) => item.status === "unchanged").length;
  return {
    items,
    ready: changed > 0 && conflicts === 0 && invalid === 0,
    changed,
    conflicts,
    invalid,
    unchanged,
  };
}
