import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { renameButtonLabel } from "./presentation.ts";
import { duplicateTitle, sortByName, type SortColumn, type SortDirection } from "./queue-presentation.ts";
import { adjacentPath, pathsInRange } from "./queue-selection.ts";
import { buildRenameRules, type RenameRules } from "./rule-builder.ts";

type FileEntry = {
  path: string;
  name: string;
  bytes: number;
  modifiedMs?: number;
};

type CollectedFiles = {
  files: FileEntry[];
  skippedHidden: number;
  truncated: boolean;
  folderDepthLimited: boolean;
  unreadableFolders: number;
};

type PreviewItem = {
  source: string;
  target: string;
  originalName: string;
  proposedName: string;
  status: "ready" | "unchanged" | "conflict" | "invalid" | "missing";
  message: string;
  matched: boolean;
};

type RenamePreview = {
  items: PreviewItem[];
  ready: boolean;
  changed: number;
  conflicts: number;
  invalid: number;
  unchanged: number;
};

type BatchResult = {
  count: number;
  logPath: string;
};

const element = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const renameFunctionSelect = element<HTMLSelectElement>("rename-function");
const prefixInput = element<HTMLInputElement>("prefix");
const suffixInput = element<HTMLInputElement>("suffix");
const findInput = element<HTMLInputElement>("find");
const replaceInput = element<HTMLInputElement>("replace");
const caseStyleInput = element<HTMLSelectElement>("case-style");
const counterStartInput = element<HTMLInputElement>("counter-start");
const counterPaddingInput = element<HTMLSelectElement>("counter-padding");
const numberPositionInput = element<HTMLSelectElement>("number-position");
const datePositionInput = element<HTMLSelectElement>("date-position");
const dateSourceInput = element<HTMLSelectElement>("date-source");
const dateFormatInput = element<HTMLSelectElement>("date-format");
const customDateField = element<HTMLElement>("custom-date-field");
const customDateInput = element<HTMLInputElement>("custom-date");
const functionPanels = new Map<string, HTMLElement>([
  ["add-text", element<HTMLElement>("add-text-settings")],
  ["replace-text", element<HTMLElement>("replace-text-settings")],
  ["change-case", element<HTMLElement>("change-case-settings")],
  ["add-numbers", element<HTMLElement>("add-numbers-settings")],
  ["add-date", element<HTMLElement>("add-date-settings")],
]);
const rowsBody = element<HTMLTableSectionElement>("rows");
const emptyState = element<HTMLDivElement>("empty");
const ruleError = element<HTMLElement>("rule-error");
const dropZone = element<HTMLElement>("drop");
const chooseFilesButton = element<HTMLButtonElement>("choose-files");
const chooseFolderButton = element<HTMLButtonElement>("choose-folder");
const clearButton = element<HTMLButtonElement>("clear");
const removeSelectedButton = element<HTMLButtonElement>("remove-selected");
const renameButton = element<HTMLButtonElement>("rename");
const statusLabelElement = element<HTMLElement>("status-label");
const backdrop = element<HTMLDivElement>("backdrop");
const notice = element<HTMLElement>("notice");
const noticeTitle = element<HTMLElement>("notice-title");
const noticeBody = element<HTMLElement>("notice-body");
const noticeClose = element<HTMLButtonElement>("notice-close");
const noticeOk = element<HTMLButtonElement>("notice-ok");
const confirmDialog = element<HTMLElement>("confirm");
const confirmBody = element<HTMLElement>("confirm-body");
const confirmCancel = element<HTMLButtonElement>("confirm-cancel");
const confirmApply = element<HTMLButtonElement>("confirm-apply");
const destinationInput = element<HTMLInputElement>("destination");
const changeFolderButton = element<HTMLButtonElement>("change-folder");
const sortButtons = Array.from(document.querySelectorAll<HTMLButtonElement>("[data-sort]"));
const tableWrap = document.querySelector<HTMLElement>(".table-wrap")!;

const ruleControls: Array<HTMLInputElement | HTMLSelectElement> = [
  renameFunctionSelect,
  prefixInput,
  suffixInput,
  findInput,
  replaceInput,
  caseStyleInput,
  counterStartInput,
  counterPaddingInput,
  numberPositionInput,
  datePositionInput,
  dateSourceInput,
  dateFormatInput,
  customDateInput,
];

let files: FileEntry[] = [];
let preview: RenamePreview | null = null;
let previewTimer = 0;
let previewRequest = 0;
let busy = false;
let pickerOpen = false;
let sortColumn: SortColumn | null = null;
let sortDirection: SortDirection = "ascending";
const selectedPaths = new Set<string>();
let selectionAnchor: string | null = null;
let selectionFocus: string | null = null;
let draggingRows = false;
const importedFolders = new Set<string>();
const isTauri = "__TAURI_INTERNALS__" in window;

function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Something Unexpected Happened.";
}

function currentRules(): RenameRules {
  const start = Number.parseInt(counterStartInput.value, 10);
  const padding = Number.parseInt(counterPaddingInput.value, 10);
  return buildRenameRules(renameFunctionSelect.value, {
    prefix: prefixInput.value,
    suffix: suffixInput.value,
    find: findInput.value,
    replace: replaceInput.value,
    caseStyle: caseStyleInput.value,
    counterStart: Number.isFinite(start) ? Math.max(0, Math.min(start, 999_999)) : 1,
    counterPadding: Number.isFinite(padding) ? Math.max(1, Math.min(padding, 3)) : 3,
    numberPosition: numberPositionInput.value,
    datePosition: datePositionInput.value,
    dateSource: dateSourceInput.value,
    dateFormat: dateFormatInput.value,
    customDate: customDateInput.value,
  });
}

function hasCompleteRule(): boolean {
  switch (renameFunctionSelect.value) {
    case "add-text":
      return prefixInput.value !== "" || suffixInput.value !== "";
    case "replace-text":
      return findInput.value !== "";
    case "change-case":
      return caseStyleInput.value !== "";
    case "add-numbers":
      return numberPositionInput.value !== "";
    case "add-date":
      return datePositionInput.value !== ""
        && dateSourceInput.value !== ""
        && dateFormatInput.value !== ""
        && (dateSourceInput.value !== "custom" || customDateInput.value !== "");
    default:
      return false;
  }
}

function syncFunctionPanel(): void {
  for (const [name, panel] of functionPanels) {
    panel.hidden = name !== renameFunctionSelect.value;
  }
}

function syncCustomDateField(): void {
  customDateField.hidden = dateSourceInput.value !== "custom";
}

function syncBackdrop(): void {
  backdrop.hidden = notice.hidden && confirmDialog.hidden;
}

function closeNotice(): void {
  notice.hidden = true;
  syncBackdrop();
}

function showNotice(title: string, body: string): void {
  confirmDialog.hidden = true;
  noticeTitle.textContent = title;
  noticeBody.textContent = body;
  notice.hidden = false;
  syncBackdrop();
  noticeOk.focus();
}

function closeConfirm(): void {
  confirmDialog.hidden = true;
  syncBackdrop();
}

function statusLabel(item: PreviewItem): string {
  if (item.status === "unchanged" && renameFunctionSelect.value === "replace-text" && !item.matched) return "No Match";
  switch (item.status) {
    case "ready":
      return "Ready";
    case "unchanged":
      return "Unchanged";
    case "conflict":
      return "Conflict";
    case "invalid":
      return "Invalid";
    case "missing":
      return "Missing";
  }
}

function sortedItems(items: PreviewItem[]): PreviewItem[] {
  return sortByName(items, sortColumn, sortDirection);
}

function renderSortHeaders(): void {
  for (const button of sortButtons) {
    const column = button.dataset.sort as SortColumn;
    const active = column === sortColumn;
    const header = button.closest("th");
    const icon = button.querySelector<HTMLElement>(".sort-icon");
    header?.setAttribute("aria-sort", active ? sortDirection : "none");
    if (icon) icon.textContent = active ? (sortDirection === "ascending" ? "↑" : "↓") : "⇅";
  }
}

function renderRows(): void {
  rowsBody.replaceChildren();
  const items = sortedItems(preview?.items ?? []);
  emptyState.hidden = files.length > 0;

  if (items.length === 0 && files.length > 0) {
    const fragment = document.createDocumentFragment();
    for (const file of files) {
      const row = document.createElement("tr");
      row.dataset.path = file.path;
      row.setAttribute("aria-selected", String(selectedPaths.has(file.path)));
      row.classList.toggle("selected", selectedPaths.has(file.path));
      const original = document.createElement("td");
      original.className = "filename-cell";
      original.textContent = file.name;
      original.title = file.path;
      const arrow = document.createElement("td");
      arrow.className = "row-arrow";
      arrow.textContent = "";
      const proposed = document.createElement("td");
      proposed.className = "filename-cell proposed-name";
      proposed.textContent = "";
      const state = document.createElement("td");
      const remove = createRowRemoveButton(file.path, file.name);
      row.append(original, arrow, proposed, state, remove);
      fragment.append(row);
    }
    rowsBody.append(fragment);
    return;
  }

  const fragment = document.createDocumentFragment();
  for (const item of items) {
    const row = document.createElement("tr");
    row.dataset.path = item.source;
    row.setAttribute("aria-selected", String(selectedPaths.has(item.source)));
    row.classList.toggle("selected", selectedPaths.has(item.source));
    const original = document.createElement("td");
    original.className = "filename-cell";
    original.textContent = item.originalName;
    original.title = item.source;

    const arrow = document.createElement("td");
    arrow.className = "row-arrow";
    arrow.textContent = "→";

    const proposed = document.createElement("td");
    proposed.className = "filename-cell proposed-name";
    const hideNonMatch = renameFunctionSelect.value === "replace-text" && !item.matched;
    proposed.textContent = hideNonMatch ? "" : item.proposedName;
    proposed.title = item.target || item.message;

    const state = document.createElement("td");
    const pill = document.createElement("span");
    pill.className = `status-pill status-${item.status}`;
    pill.textContent = statusLabel(item);
    pill.title = item.message;
    state.append(pill);
    const remove = createRowRemoveButton(item.source, item.originalName);
    row.append(original, arrow, proposed, state, remove);
    fragment.append(row);
  }
  rowsBody.append(fragment);
}

function renderControls(): void {
  const changed = preview?.changed ?? 0;
  clearButton.disabled = busy || files.length === 0;
  removeSelectedButton.disabled = busy || selectedPaths.size === 0;
  removeSelectedButton.textContent = `Remove Selected (${selectedPaths.size.toLocaleString()})`;
  renameButton.disabled = busy || preview?.ready !== true || !isTauri;
  renameButton.textContent = renameButtonLabel(changed, busy);
  confirmApply.disabled = busy || destinationInput.value === "";
  changeFolderButton.disabled = busy;
  chooseFilesButton.disabled = busy || pickerOpen || !isTauri;
  chooseFolderButton.disabled = busy || pickerOpen || !isTauri;
  for (const control of ruleControls) control.disabled = busy;

}

function render(): void {
  renderRows();
  renderSortHeaders();
  renderControls();
}

function visiblePaths(): string[] {
  if (preview) return sortedItems(preview.items).map((item) => item.source);
  return files.map((file) => file.path);
}

function selectRange(from: string, to: string, additive: boolean): void {
  const paths = visiblePaths();
  const range = pathsInRange(paths, from, to);
  if (range.length === 0) return;
  if (!additive) selectedPaths.clear();
  range.forEach((path) => selectedPaths.add(path));
  selectionFocus = to;
}

function removeSelected(): void {
  if (busy || selectedPaths.size === 0) return;
  files = files.filter((file) => !selectedPaths.has(file.path));
  selectedPaths.clear();
  selectionAnchor = null;
  selectionFocus = null;
  importedFolders.clear();
  void refreshPreview();
}

function removePath(path: string): void {
  if (busy) return;
  files = files.filter((file) => file.path !== path);
  selectedPaths.delete(path);
  if (selectionAnchor === path) selectionAnchor = null;
  if (selectionFocus === path) selectionFocus = null;
  importedFolders.clear();
  void refreshPreview();
}

function createRowRemoveButton(path: string, name: string): HTMLTableCellElement {
  const cell = document.createElement("td");
  cell.className = "remove-cell";
  const button = document.createElement("button");
  button.className = "row-remove-button";
  button.type = "button";
  button.textContent = "×";
  button.dataset.removePath = path;
  button.setAttribute("aria-label", `Remove ${name} From Queue`);
  button.title = `Remove ${name}`;
  button.disabled = busy;
  cell.append(button);
  return cell;
}

async function refreshPreview(): Promise<void> {
  window.clearTimeout(previewTimer);
  const request = ++previewRequest;
  ruleError.textContent = "";

  if (files.length === 0) {
    preview = null;
    render();
    return;
  }

  if (!hasCompleteRule()) {
    preview = null;
    render();
    return;
  }

  try {
    const result = await invoke<RenamePreview>("preview_save_as", {
      paths: files.map((file) => file.path),
      rules: currentRules(),
    });
    if (request !== previewRequest) return;
    preview = result;
  } catch (error) {
    if (request !== previewRequest) return;
    preview = null;
    ruleError.textContent = errorMessage(error);
  }
  render();
}

function schedulePreview(): void {
  window.clearTimeout(previewTimer);
  if (!hasCompleteRule()) {
    previewRequest += 1;
    preview = null;
    ruleError.textContent = "";
    render();
    return;
  }
  previewTimer = window.setTimeout(() => void refreshPreview(), 110);
}

function collectionNotice(result: CollectedFiles, added: number, duplicateCount = 0): void {
  const notes: string[] = [];
  if (duplicateCount > 0) {
    notes.push(`${duplicateCount.toLocaleString()} File${duplicateCount === 1 ? " Is" : "s Are"} Already In Queue.`);
  }
  if (result.truncated) notes.push("The 20,000-File Safety Limit Was Reached.");
  if (result.folderDepthLimited) notes.push("Folders Deeper Than Eight Levels Were Skipped.");
  if (result.unreadableFolders > 0) {
    notes.push(`${result.unreadableFolders} Unreadable Folder${result.unreadableFolders === 1 ? " Was" : "s Were"} Skipped.`);
  }
  if (result.skippedHidden > 0) {
    notes.push(`${result.skippedHidden} Hidden Item${result.skippedHidden === 1 ? " Was" : "s Were"} Skipped.`);
  }
  if (added === 0 && notes.length === 0) notes.push("No New Files Were Found In That Selection.");
  if (notes.length > 0) {
    const addedTitle = `${added.toLocaleString()} File${added === 1 ? "" : "s"} Added To Queue`;
    showNotice(added > 0 ? addedTitle : "Nothing Added", notes.join(" "));
  }
}

async function addPaths(paths: string[], directory = false): Promise<void> {
  if (paths.length === 0 || busy) return;
  if (directory && paths.every((path) => importedFolders.has(path))) {
    showNotice("Folder Already Imported", "Choose A Different Folder Or Clear The Current Queue First.");
    return;
  }
  busy = true;
  renderControls();
  try {
    const collected = await invoke<CollectedFiles>("collect_files", { paths });
    const known = new Set(files.map((file) => file.path));
    const additions = collected.files.filter((file) => !known.has(file.path));
    files = [...files, ...additions];
    if (directory) paths.forEach((path) => importedFolders.add(path));
    const duplicateCount = collected.files.length - additions.length;
    if (duplicateCount > 0 && additions.length === 0 && !directory) {
      showNotice(
        duplicateTitle(duplicateCount, false),
        "Choose Different Files Or Clear The Current Queue First.",
      );
    } else {
      collectionNotice(collected, additions.length, duplicateCount);
    }
    await refreshPreview();
  } catch (error) {
    showNotice("Could Not Read That Selection", errorMessage(error));
  } finally {
    busy = false;
    render();
  }
}

async function choosePaths(directory: boolean): Promise<void> {
  if (pickerOpen || busy) return;
  if (!isTauri) return;
  pickerOpen = true;
  renderControls();
  try {
    const chosen = await open({
      directory,
      multiple: !directory,
      title: directory ? "Choose A Folder To Import" : "Choose Files To Import",
    });
    const paths = Array.isArray(chosen) ? chosen : typeof chosen === "string" ? [chosen] : [];
    await addPaths(paths, directory);
  } catch (error) {
    showNotice("Could Not Open The Picker", errorMessage(error));
  } finally {
    pickerOpen = false;
    renderControls();
  }
}

async function applySaveAs(): Promise<void> {
  if (busy || preview?.ready !== true) return;
  if (destinationInput.value === "") return;
  closeConfirm();
  busy = true;
  render();
  try {
    const destination = destinationInput.value;
    const result = await invoke<BatchResult>("save_as", {
      paths: files.map((file) => file.path),
      rules: currentRules(),
      destination,
    });
    files = [];
    preview = null;
    selectedPaths.clear();
    selectionAnchor = null;
    selectionFocus = null;
    importedFolders.clear();
    showNotice(
      `${result.count.toLocaleString()} File${result.count === 1 ? "" : "s"} Saved`,
      "Original Files Were Not Changed.",
    );
  } catch (error) {
    showNotice("Files Were Not Saved", errorMessage(error));
    await refreshPreview();
  } finally {
    busy = false;
    render();
  }
}

for (const control of ruleControls) {
  control.addEventListener("input", schedulePreview);
  control.addEventListener("change", schedulePreview);
}

for (const button of sortButtons) {
  button.addEventListener("click", () => {
    const column = button.dataset.sort as SortColumn;
    if (sortColumn === column) {
      sortDirection = sortDirection === "ascending" ? "descending" : "ascending";
    } else {
      sortColumn = column;
      sortDirection = "ascending";
    }
    render();
  });
}

rowsBody.addEventListener("pointerdown", (event) => {
  if ((event.target as HTMLElement).closest("button")) return;
  const row = (event.target as HTMLElement).closest<HTMLTableRowElement>("tr[data-path]");
  const path = row?.dataset.path;
  if (!path || event.button !== 0) return;
  event.preventDefault();
  tableWrap.focus();
  if (event.metaKey || event.ctrlKey) {
    if (selectedPaths.has(path)) selectedPaths.delete(path);
    else selectedPaths.add(path);
    selectionAnchor = path;
    selectionFocus = path;
    draggingRows = false;
  } else if (event.shiftKey && selectionAnchor) {
    selectRange(selectionAnchor, path, false);
    draggingRows = false;
  } else {
    selectedPaths.clear();
    selectedPaths.add(path);
    selectionAnchor = path;
    selectionFocus = path;
    draggingRows = true;
  }
  render();
});

rowsBody.addEventListener("click", (event) => {
  const button = (event.target as HTMLElement).closest<HTMLButtonElement>("button[data-remove-path]");
  if (!button?.dataset.removePath) return;
  removePath(button.dataset.removePath);
});

rowsBody.addEventListener("pointerover", (event) => {
  if (!draggingRows || (event.buttons & 1) === 0 || !selectionAnchor) return;
  const row = (event.target as HTMLElement).closest<HTMLTableRowElement>("tr[data-path]");
  const path = row?.dataset.path;
  if (!path) return;
  selectRange(selectionAnchor, path, false);
  render();
});

window.addEventListener("pointerup", () => {
  draggingRows = false;
});

tableWrap.addEventListener("keydown", (event) => {
  if ((event.key === "Delete" || event.key === "Backspace") && selectedPaths.size > 0) {
    event.preventDefault();
    removeSelected();
    return;
  }
  const paths = visiblePaths();
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "a" && paths.length > 0) {
    event.preventDefault();
    selectedPaths.clear();
    paths.forEach((path) => selectedPaths.add(path));
    selectionAnchor = paths[0];
    selectionFocus = paths[paths.length - 1] ?? null;
    render();
    return;
  }
  if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
  if (paths.length === 0) return;
  event.preventDefault();
  const delta = event.key === "ArrowDown" ? 1 : -1;
  const next = adjacentPath(paths, selectionFocus, delta);
  if (next === null) return;
  if ((event.shiftKey || event.metaKey || event.ctrlKey) && selectionAnchor) {
    selectRange(selectionAnchor, next, false);
  } else {
    selectedPaths.clear();
    selectedPaths.add(next);
    selectionAnchor = next;
    selectionFocus = next;
  }
  render();
});

renameFunctionSelect.addEventListener("change", () => {
  syncFunctionPanel();
  schedulePreview();
});
dateSourceInput.addEventListener("change", syncCustomDateField);

chooseFilesButton.addEventListener("click", () => void choosePaths(false));
chooseFolderButton.addEventListener("click", () => void choosePaths(true));
removeSelectedButton.addEventListener("click", removeSelected);
clearButton.addEventListener("click", () => {
  files = [];
  preview = null;
  selectedPaths.clear();
  selectionAnchor = null;
  selectionFocus = null;
  importedFolders.clear();
  ruleError.textContent = "";
  render();
});
renameButton.addEventListener("click", () => {
  if (preview?.ready !== true) return;
  destinationInput.value = "";
  confirmBody.textContent = `Would You Like To Rename ${preview.changed.toLocaleString()} Selected File${preview.changed === 1 ? "" : "s"}?`;
  notice.hidden = true;
  confirmDialog.hidden = false;
  syncBackdrop();
  renderControls();
  changeFolderButton.focus();
});
changeFolderButton.addEventListener("click", async () => {
  if (!isTauri) return;
  try {
    const chosen = await open({
      directory: true,
      multiple: false,
      title: "Choose A Destination Folder",
    });
    if (typeof chosen === "string") destinationInput.value = chosen;
    renderControls();
  } catch (error) {
    showNotice("Could Not Open The Picker", errorMessage(error));
  }
});
confirmCancel.addEventListener("click", closeConfirm);
confirmApply.addEventListener("click", () => void applySaveAs());
destinationInput.addEventListener("input", renderControls);
noticeClose.addEventListener("click", closeNotice);
noticeOk.addEventListener("click", closeNotice);
backdrop.addEventListener("click", () => {
  closeNotice();
  closeConfirm();
});

window.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
  closeNotice();
  closeConfirm();
});

if (isTauri) {
  void listen<{ paths: string[] }>("tauri://drag-drop", ({ payload }) => {
    dropZone.classList.remove("over");
    void addPaths(payload.paths);
  });
  void listen("tauri://drag-enter", () => {
    if (!busy) dropZone.classList.add("over");
  });
  void listen("tauri://drag-leave", () => dropZone.classList.remove("over"));

}

if (!isTauri) {
  statusLabelElement.textContent = "Preview Only";
  chooseFilesButton.title = "Available In The Rollcall Desktop App";
  chooseFolderButton.title = "Available In The Rollcall Desktop App";
  renameButton.title = "Available In The Rollcall Desktop App";
}

syncFunctionPanel();
syncCustomDateField();
render();
