import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { dimensionsAreValid, formatDimensionInput } from "./dimension.ts";
import { filenameTimestamp } from "./timestamp.ts";
import { formatSizePair } from "./queue-size.ts";
import { savedCountLabel } from "./queue-summary.ts";
import { clearPolicy } from "./clear-policy.ts";
import { chooseFilesEnabled, selectionNotice, uniqueNewPaths } from "./choose-files-policy.ts";
import { removePolicy, removeSelectionPolicy } from "./remove-policy.ts";
import {
  activeAfterRemovals,
  arrowSelection,
  clickSelection,
  isSelectAllShortcut,
  keyboardDeleteAllowed,
  selectAll,
  type QueueSelection,
} from "./selection-policy.ts";
import {
  compressionQuality,
  conversionPlan,
  convertButtonEnabled,
  progressPresentation,
  reconversionPaths,
  type ConvertibleRow,
  type ConversionJob,
} from "./conversion-policy.ts";
import {
  baseName,
  buildSaveCopies,
  extensionOf,
  parentFolder,
  saveEnabled,
  scopedSavePaths,
  stemOf,
} from "./save-policy.ts";

type SourceInfo = {
  path: string;
  name: string;
  bytes: number;
  width: number;
  height: number;
  hasMetadata: boolean;
};

type Probe =
  | ({ kind: "ok" } & SourceInfo)
  | { kind: "failed"; path: string; error: string };

type CollectedImages = {
  paths: string[];
  ignored: number;
  truncated: boolean;
  folderDepthLimited: boolean;
  unreadableFolders: number;
};

type Progress =
  | { status: "done"; path: string; output: string; bytes: number }
  | { status: "failed"; path: string; error: string }
  | { status: "skipped"; path: string };

type Row = {
  info: SourceInfo | null;
  path: string;
  state: "queued" | "working" | "done" | "failed" | "cancelled" | "saved";
  detail: string;
  targetDimension?: string;
  error?: string;
  // Where the converted result currently sits (a scratch path) until saved.
  output?: string;
  outputBytes?: number;
  savedPath?: string;
  reconvert?: boolean;
};

const EXTENSIONS = ["heic", "heif", "avif", "webp", "jpg", "jpeg", "jfif", "png", "tif", "tiff"];
const IS_MAC = /Mac|iPhone|iPad|iPod/.test(navigator.platform || navigator.userAgent);

const rows = new Map<string, Row>();
const rowEls = new Map<string, HTMLTableRowElement>();
let selectedOrig = "";
const selectedPaths = new Set<string>();
let selectionAnchor = "";
let selectionExplicit = false;
let running = false;
let conversionStarting = false;
let cancellationRequested = false;
const activeConversionPaths = new Set<string>();
let saving = false;
let returnFocusToDimension = false;
let pickerOpen = false;
let dropProcessing = false;
const removingPaths = new Set<string>();
const reconversionBackups = new Map<string, Row>();
const reconversionFailures = new Set<string>();

const element = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const dropZone = element<HTMLElement>("drop");
const rowsBody = element<HTMLTableSectionElement>("rows");
const notice = element<HTMLDivElement>("notice");
const noticeTitle = element<HTMLParagraphElement>("notice-title");
const noticeBody = element<HTMLDivElement>("notice-body");
const backdrop = element<HTMLDivElement>("backdrop");

// The backdrop is visible whenever any popup is open, blocking the UI behind it.
function syncBackdrop() {
  backdrop.hidden = !document.querySelector(".toast:not([hidden])");
}
const saveDialog = element<HTMLDivElement>("save-dialog");
const saveTitle = element<HTMLParagraphElement>("save-title");
const saveNameField = element<HTMLLabelElement>("save-name-field");
const saveNameInput = element<HTMLInputElement>("save-name");
const saveExt = element<HTMLSpanElement>("save-ext");
const saveLocLabel = element<HTMLParagraphElement>("save-loc-label");
const saveLocation = element<HTMLInputElement>("save-location");
const saveConfirm = element<HTMLButtonElement>("save-confirm");
const saveClose = element<HTMLButtonElement>("save-close");
const saveCancel = element<HTMLButtonElement>("save-cancel");
const saveChange = element<HTMLButtonElement>("save-change");
const confirmDialog = element<HTMLDivElement>("confirm");
const confirmTitle = element<HTMLParagraphElement>("confirm-title");
const confirmLine = element<HTMLParagraphElement>("confirm-line");
const discardDialog = element<HTMLDivElement>("discard-dialog");
const discardCancel = element<HTMLButtonElement>("discard-cancel");
const discardConfirm = element<HTMLButtonElement>("discard-confirm");
const discardRowDialog = element<HTMLDivElement>("discard-row-dialog");
const discardRowCancel = element<HTMLButtonElement>("discard-row-cancel");
const discardRowConfirm = element<HTMLButtonElement>("discard-row-confirm");
const discardSelectedDialog = element<HTMLDivElement>("discard-selected-dialog");
const discardSelectedCancel = element<HTMLButtonElement>("discard-selected-cancel");
const discardSelectedConfirm = element<HTMLButtonElement>("discard-selected-confirm");
const emptyNote = element<HTMLParagraphElement>("empty");
const summary = element<HTMLSpanElement>("summary");
const formatSelect = element<HTMLSelectElement>("format");
const qualityField = element<HTMLLabelElement>("quality-field");
const qualityInput = element<HTMLInputElement>("quality");
const qualityValue = element<HTMLOutputElement>("quality-value");
const origDim = element<HTMLSelectElement>("orig-dim");
const newPixel = element<HTMLInputElement>("new-pixel");
const keepMetadata = element<HTMLInputElement>("keep-metadata");
const timestamp = element<HTMLInputElement>("timestamp");
const convertButton = element<HTMLButtonElement>("convert");
const saveButton = element<HTMLButtonElement>("save");
const clearButton = element<HTMLButtonElement>("clear");
const removeSelectedButton = element<HTMLButtonElement>("remove-selected");
const browseButton = element<HTMLButtonElement>("browse");

function updateSummary() {
  const total = rows.size;
  const done = [...rows.values()].filter((row) => row.state === "done").length;
  const saved = [...rows.values()].filter((row) => row.state === "saved").length;
  const failed = [...rows.values()].filter((row) => row.state === "failed").length;
  const cancelled = [...rows.values()].filter((row) => row.state === "cancelled").length;

  if (total === 0) {
    summary.textContent = "";
  } else if (running) {
    const completed = [...activeConversionPaths].filter((path) => {
      const state = rows.get(path)?.state;
      return state === "done" || state === "saved" || state === "failed" || state === "cancelled";
    }).length;
    summary.textContent = `Converting — ${completed} Of ${activeConversionPaths.size}`;
  } else if (saved) {
    summary.textContent = savedCountLabel(saved);
  } else if (done || failed || cancelled) {
    summary.textContent = [
      done ? `${done} CONVERTED` : "",
      failed ? `${failed} FAILED` : "",
      cancelled ? `${cancelled} CANCELLED` : "",
    ].filter(Boolean).join(", ");
  } else {
    summary.textContent = `${total} File${total === 1 ? "" : "s"} Queued`;
  }
}

// Updates a single row in place — the detail text, its state class, and the
// summary — without rebuilding the whole table. Used for progress events, so a
// large batch does not re-render N rows on every one of N completions.
function updateRow(path: string) {
  const row = rows.get(path);
  const tr = rowEls.get(path);
  if (!row || !tr) return;
  tr.className = rowClass(row);
  const sizeCell = tr.children[1] as HTMLElement | undefined;
  if (sizeCell) sizeCell.textContent = formatSizePair(row.info?.bytes ?? 0, row.outputBytes);
  const detailCell = tr.children[2] as HTMLElement | undefined;
  if (detailCell) detailCell.textContent = row.detail;
  populateOrigDim();
  updateSummary();
}

// Fills the Pixel (original) dropdown with each queued file's real dimensions,
// keeping the current selection if that file is still present.
function populateOrigDim() {
  origDim.replaceChildren();

  const placeholder = document.createElement("option");
  placeholder.value = "";
  placeholder.disabled = true;
  placeholder.textContent = "—";
  origDim.append(placeholder);

  for (const row of rows.values()) {
    if (!row.info) continue;
    const option = document.createElement("option");
    option.value = row.path;
    option.textContent = `${row.info.width} × ${row.info.height}`;
    origDim.append(option);
  }

  origDim.value = rows.has(selectedOrig) ? selectedOrig : "";
  if (origDim.value === "") placeholder.selected = true;
}

function applySelection(selection: QueueSelection) {
  selectedPaths.clear();
  for (const path of selection.selected) {
    if (rows.has(path)) selectedPaths.add(path);
  }
  selectedOrig = rows.has(selection.active) ? selection.active : "";
  selectionAnchor = rows.has(selection.anchor) ? selection.anchor : selectedOrig;
}

function clearReconversionRequests() {
  for (const [path, row] of rows) {
    if (row.reconvert) rows.set(path, { ...row, reconvert: false });
  }
}

function applyUserSelection(selection: QueueSelection) {
  clearReconversionRequests();
  selectionExplicit = true;
  applySelection(selection);
}

function requestSelectedReconversion() {
  if (!selectionExplicit) return;
  const requested = reconversionPaths([...rows.values()].map(conversionRow), selectedPaths);
  for (const [path, row] of rows) {
    const reconvert = requested.has(path);
    if (Boolean(row.reconvert) !== reconvert) rows.set(path, { ...row, reconvert });
  }
  updateConvertControl();
}

function rowClass(row: Row): string {
  return [row.state, selectedPaths.has(row.path) ? "selected" : "", row.path === selectedOrig ? "active" : ""]
    .filter(Boolean)
    .join(" ");
}

function conversionRow(row: Row): ConvertibleRow {
  return {
    path: row.path,
    state: row.state,
    hasSourceInfo: row.info !== null,
    targetDimension: row.targetDimension,
    output: row.output,
    reconvert: row.reconvert,
  };
}

function conversionBusy(): boolean {
  return running || conversionStarting;
}

function interactionBusy(): boolean {
  return conversionBusy() || saving;
}

function updateConvertControl() {
  convertButton.disabled = saving || !convertButtonEnabled(
    [...rows.values()].map(conversionRow),
    formatSelect.value,
    running,
    conversionStarting,
    cancellationRequested,
  );
  convertButton.textContent = conversionStarting
    ? "Starting"
    : running
      ? cancellationRequested ? "Cancelling" : "Cancel"
      : "Convert";
  convertButton.classList.toggle("cancel", running);
}

function render() {
  const busy = interactionBusy();
  for (const path of [...selectedPaths]) {
    if (!rows.has(path)) selectedPaths.delete(path);
  }
  rowsBody.replaceChildren();
  rowEls.clear();

  for (const row of rows.values()) {
    const tr = document.createElement("tr");
    tr.className = rowClass(row);
    rowEls.set(row.path, tr);

    const name = document.createElement("td");
    name.textContent = row.info?.name ?? row.path.split("/").pop() ?? row.path;
    name.title = row.path;

    const size = document.createElement("td");
    size.textContent = formatSizePair(row.info?.bytes ?? 0, row.outputBytes);

    const detail = document.createElement("td");
    detail.textContent = row.detail;
    detail.title = row.error ?? "";
    detail.className = "detail";

    const remove = document.createElement("td");
    remove.className = "remove-cell";
    const removeButton = document.createElement("button");
    removeButton.type = "button";
    removeButton.className = "remove";
    removeButton.textContent = "×";
    removeButton.title = "Remove from the queue";
    removeButton.setAttribute("aria-label", `Remove ${row.info?.name ?? "file"}`);
    removeButton.disabled = !removePolicy(row, busy).enabled || removingPaths.has(row.path);
    removeButton.addEventListener("click", (event) => {
      event.stopPropagation();
      void removeQueueRow(row.path);
    });
    remove.append(removeButton);

    tr.addEventListener("click", (event) => {
      if (busy || removingPaths.has(row.path)) return;
      if (event.shiftKey) event.preventDefault();
      applyUserSelection(
        clickSelection(
          [...rows.keys()], selectedOrig, selectionAnchor, row.path, event.shiftKey, selectedPaths,
        ),
      );
      render();
    });

    tr.append(name, size, detail, remove);
    rowsBody.append(tr);
  }

  const total = rows.size;
  emptyNote.hidden = total > 0;
  updateConvertControl();
  clearButton.disabled = !clearPolicy(rows.values(), busy).enabled;
  removeSelectedButton.hidden = selectedPaths.size < 2;
  removeSelectedButton.textContent = `Remove Selected (${selectedPaths.size})`;
  removeSelectedButton.disabled = busy || [...selectedPaths].some((path) => removingPaths.has(path));
  saveButton.disabled = !saveEnabled(
    [...rows.values()].map((row) => ({ path: row.path, state: row.state, outputPath: row.output })),
    busy,
    selectedPaths,
    selectionExplicit,
  );
  saveButton.textContent = saving ? "Saving" : "Save";
  const canChoose = chooseFilesEnabled(rows.values(), busy, pickerOpen || dropProcessing);
  browseButton.disabled = !canChoose;
  dropZone.classList.toggle("disabled", !canChoose);
  dropZone.setAttribute("aria-disabled", String(!canChoose));
  if (!canChoose) dropZone.classList.remove("over");

  formatSelect.disabled = busy;
  origDim.disabled = busy;
  newPixel.disabled = busy;
  qualityInput.disabled = busy;
  keepMetadata.disabled = busy;
  timestamp.disabled = busy;
  const saveDialogBusy = saving || saveFolderPickerOpen;
  saveClose.disabled = saveDialogBusy;
  saveCancel.disabled = saveDialogBusy;
  saveConfirm.disabled = saveDialogBusy || pendingFolder.trim() === "";
  saveChange.disabled = saveDialogBusy;
  saveNameInput.disabled = saveDialogBusy;
  saveLocation.disabled = saveDialogBusy;

  // An empty queue resets the To box to the placeholder.
  if (total === 0) {
    selectedOrig = "";
    selectedPaths.clear();
    selectionAnchor = "";
    selectionExplicit = false;
    formatSelect.value = "";
    newPixel.value = "";
    qualityField.hidden = false;
  }

  populateOrigDim();
  if (total > 0) newPixel.value = rows.get(selectedOrig)?.targetDimension ?? "";
  updateSummary();
}

// One popup for every message: a centred Title-Case heading, an optional body
// (a left-aligned list of filenames, or one centred line), and a close button.
// It never dismisses itself — the user closes it.
function popup(title: string, body: { names?: string[]; line?: string } = {}) {
  noticeTitle.textContent = title;
  noticeBody.replaceChildren();

  if (body.names && body.names.length > 0) {
    const list = document.createElement("div");
    list.className = "toast-list";
    for (const name of body.names) {
      const span = document.createElement("span");
      span.textContent = name;
      list.append(span);
    }
    noticeBody.append(list);
  } else if (body.line) {
    const line = document.createElement("p");
    line.className = "toast-line";
    line.textContent = body.line;
    noticeBody.append(line);
  }

  recenter(notice);
  notice.hidden = false;
  syncBackdrop();
}

function closePopup() {
  notice.hidden = true;
  syncBackdrop();
  if (returnFocusToDimension) {
    returnFocusToDimension = false;
    newPixel.focus();
    newPixel.setSelectionRange(newPixel.value.length, newPixel.value.length);
  }
}

// Lets a popup be dragged anywhere on screen by its title bar. Dragging switches
// the box from centred positioning to explicit coordinates.
function makeDraggable(box: HTMLElement) {
  let dragging = false;
  let startX = 0;
  let startY = 0;
  let originX = 0;
  let originY = 0;

  box.style.cursor = "move";
  box.addEventListener("mousedown", (event) => {
    // Anywhere in the box drags it — except the buttons and the name field.
    const target = event.target as HTMLElement;
    if (target.closest("button, input, select, textarea, a")) return;
    dragging = true;
    const rect = box.getBoundingClientRect();
    box.style.left = `${rect.left}px`;
    box.style.top = `${rect.top}px`;
    box.style.transform = "none";
    startX = event.clientX;
    startY = event.clientY;
    originX = rect.left;
    originY = rect.top;
    event.preventDefault();
  });
  window.addEventListener("mousemove", (event) => {
    if (!dragging) return;
    let left = originX + event.clientX - startX;
    let top = originY + event.clientY - startY;
    // Keep the whole box within the window.
    const maxLeft = Math.max(0, window.innerWidth - box.offsetWidth);
    const maxTop = Math.max(0, window.innerHeight - box.offsetHeight);
    left = Math.max(0, Math.min(left, maxLeft));
    top = Math.max(0, Math.min(top, maxTop));
    box.style.left = `${left}px`;
    box.style.top = `${top}px`;
  });
  window.addEventListener("mouseup", () => {
    dragging = false;
  });
}

// Clears any dragged position so the popup opens centred again.
function recenter(box: HTMLElement) {
  box.style.left = "";
  box.style.top = "";
  box.style.transform = "";
}


async function addPaths(dropped: string[]) {
  let collected: CollectedImages;
  try {
    // Rust walks any folders and drops anything that is not a supported image.
    collected = await invoke<CollectedImages>("collect_images", { paths: dropped });
  } catch {
    popup("Could Not Read Selected Files");
    return;
  }

  const wanted = uniqueNewPaths(collected.paths, new Set(rows.keys()));
  const notice = selectionNotice(
    dropped.length,
    collected.paths.length,
    wanted.length,
    collected.ignored,
    collected.truncated,
    collected.folderDepthLimited,
    collected.unreadableFolders,
  );

  if (wanted.length === 0) {
    // Duplicate selections leave the current queue untouched without an alert.
    if (notice.kind === "limit") {
      popup("20,000 File Limit Reached");
    } else if (notice.kind === "folder-depth") {
      popup("Folder Depth Limit Reached");
    } else if (notice.kind === "unreadable-folders") {
      popup("Some Folders Could Not Be Read", {
        line: `${notice.count} Folder${notice.count === 1 ? "" : "s"} Ignored`,
      });
    } else if (notice.kind === "no-supported") {
      popup("No Supported Images Found");
    } else if (notice.kind === "unsupported") {
      popup("Unsupported Files Ignored", {
        line: `${notice.count} File${notice.count === 1 ? "" : "s"} Ignored`,
      });
    }
    return;
  }

  if (notice.kind === "limit") {
    popup("20,000 File Limit Reached", {
      line: notice.ignored > 0 ? `${notice.ignored} Unsupported File${notice.ignored === 1 ? "" : "s"} Ignored` : undefined,
    });
  } else if (notice.kind === "folder-depth") {
    popup("Folder Depth Limit Reached");
  } else if (notice.kind === "unreadable-folders") {
    popup("Some Folders Could Not Be Read", {
      line: `${notice.count} Folder${notice.count === 1 ? "" : "s"} Ignored`,
    });
  } else if (notice.kind === "unsupported") {
    popup("Unsupported Files Ignored", {
      line: `${notice.count} File${notice.count === 1 ? "" : "s"} Ignored`,
    });
  }

  for (const path of wanted) {
    rows.set(path, { info: null, path, state: "queued", detail: "Reading" });
  }
  if (selectedOrig === "") {
    selectedOrig = wanted[0];
    selectionAnchor = wanted[0];
    selectedPaths.add(wanted[0]);
  }
  render();

  let probes: Probe[];
  try {
    probes = await invoke<Probe[]>("probe_files", { paths: wanted });
  } catch {
    for (const path of wanted) {
      const row = rows.get(path);
      if (row) rows.set(path, { ...row, state: "failed", detail: "Failed", error: "Could Not Read File" });
    }
    render();
    return;
  }
  for (const probe of probes) {
    const existing = rows.get(probe.path);
    // The row may have been removed while probing; never recreate it afterward.
    if (!existing) continue;
    if (probe.kind === "ok") {
      rows.set(probe.path, { ...existing, info: probe, state: "queued", detail: "Ready" });
    } else {
      rows.set(probe.path, {
        ...existing,
        info: null,
        state: "failed",
        detail: "Failed",
        error: probe.error,
      });
    }
  }
  render();
}

/// Colons and slashes are illegal or displayed wrongly in filenames, so the
/// stamp is digits only: 20260824-131205, which also sorts correctly.
function stamp(): string {
  return filenameTimestamp(new Date());
}

function conversionSettings(folder: string) {
  return {
    format: formatSelect.value,
    quality: compressionQuality(Number(qualityInput.value)),
    // Each job overrides this with its own Original or Exact resize request.
    resize: { mode: "original" },
    keepMetadata: keepMetadata.checked,
    destination: folder,
    // One stamp for the whole run, so every file from a batch carries the same one.
    suffix: timestamp.checked ? stamp() : null,
  };
}

async function convert() {
  if (saving) return;
  if (running) {
    if (cancellationRequested) return;
    cancellationRequested = true;
    render();
    try {
      await invoke("cancel_batch");
    } catch {
      cancellationRequested = false;
      popup("Conversion Could Not Be Cancelled");
      render();
    }
    return;
  }
  if (conversionStarting) return;

  const plan = conversionPlan([...rows.values()].map(conversionRow), formatSelect.value);
  if (plan.kind === "missing-format") {
    popup("Select A Type");
    return;
  }
  if (plan.kind === "unsaved-results") {
    popup("Save Or Discard Converted Files First");
    return;
  }
  if (plan.kind === "no-convertible-files") {
    popup("No Files Ready To Convert");
    return;
  }
  if (plan.kind === "invalid-dimension") {
    applySelection(clickSelection([...rows.keys()], selectedOrig, selectionAnchor, plan.path, false));
    returnFocusToDimension = true;
    popup("Min Is 1024 px · Max Is 7680 px");
    render();
    return;
  }

  const jobs: ConversionJob[] = plan.jobs;
  const previousRows = new Map(jobs.map((job) => [job.path, { ...rows.get(job.path)! }]));
  reconversionBackups.clear();
  reconversionFailures.clear();
  conversionStarting = true;
  activeConversionPaths.clear();
  for (const job of jobs) activeConversionPaths.add(job.path);
  render();

  // Step one: convert every file in the queue into a scratch directory. Results
  // show in the table; the user then presses Save (step two) to write them out.
  let scratch: string;
  try {
    scratch = await invoke<string>("scratch_dir");
  } catch {
    conversionStarting = false;
    activeConversionPaths.clear();
    popup("Conversion Could Not Start");
    render();
    return;
  }

  for (const job of jobs) {
    const row = rows.get(job.path);
    if (!row) continue;
    if (row.reconvert) reconversionBackups.set(job.path, { ...row });
    rows.set(job.path, {
      ...row,
      state: "working",
      detail: "Converting",
      error: undefined,
      output: undefined,
      outputBytes: undefined,
      savedPath: undefined,
      reconvert: false,
    });
  }

  running = true;
  conversionStarting = false;
  cancellationRequested = false;
  render();

  try {
    await invoke("convert_batch", {
      jobs,
      settings: conversionSettings(scratch),
    });
  } catch {
    running = false;
    cancellationRequested = false;
    activeConversionPaths.clear();
    reconversionBackups.clear();
    reconversionFailures.clear();
    for (const [path, row] of previousRows) rows.set(path, row);
    popup("Conversion Could Not Start");
    render();
  }
}

let pendingFolder = "";
let saveFolderPickerOpen = false;

// Converted plus previously saved rows retain their protected scratch result,
// allowing Save to write another copy into a different target folder.
function savableRows(): Row[] {
  const all = [...rows.values()];
  const paths = new Set(scopedSavePaths(
    all.map((row) => ({ path: row.path, state: row.state, outputPath: row.output })),
    selectedPaths,
    selectionExplicit,
  ));
  return all.filter((row) => paths.has(row.path));
}

function saveablePolicyRows(ready: Row[]) {
  return ready.map((row) => ({ outputPath: row.output! }));
}

function updateSaveDestinationDisplay() {
  if (document.activeElement !== saveLocation) saveLocation.value = pendingFolder;
  saveConfirm.disabled = saving || saveFolderPickerOpen || pendingFolder.trim() === "";
}

// Step two. A single result shows an editable name; a batch shows just the
// target folder. Its path can be typed directly; Choose Folder opens the native
// folder picker.
function openSaveDialog() {
  if (interactionBusy()) return;
  const ready = savableRows();
  if (ready.length === 0) {
    popup("Nothing To Save");
    return;
  }

  pendingFolder = parentFolder(ready[0].path);
  const single = ready.length === 1;
  const ext = extensionOf(ready[0].output!);

  if (single) {
    saveTitle.textContent = "Save Image";
    saveNameField.hidden = false;
    saveNameInput.value = stemOf(baseName(ready[0].output!));
    saveExt.textContent = ext ? `.${ext}` : "";
    saveConfirm.textContent = "Save";
  } else {
    saveTitle.textContent = `Save ${ready.length} Images`;
    saveNameField.hidden = true;
    saveConfirm.textContent = "Save All";
  }

  saveLocLabel.textContent = "Save To";
  updateSaveDestinationDisplay();
  recenter(saveDialog);
  saveDialog.hidden = false;
  syncBackdrop();
}

function closeSaveDialog() {
  if (saving || saveFolderPickerOpen) return;
  saveDialog.hidden = true;
  syncBackdrop();
}

function confirmDiscardAll(): Promise<boolean> {
  recenter(discardDialog);
  discardDialog.hidden = false;
  syncBackdrop();

  return new Promise((resolve) => {
    const finish = (confirmed: boolean) => {
      discardDialog.hidden = true;
      syncBackdrop();
      discardCancel.removeEventListener("click", onCancel);
      discardConfirm.removeEventListener("click", onConfirm);
      resolve(confirmed);
    };
    const onCancel = () => finish(false);
    const onConfirm = () => finish(true);
    discardCancel.addEventListener("click", onCancel);
    discardConfirm.addEventListener("click", onConfirm);
  });
}

function confirmDiscardRow(): Promise<boolean> {
  recenter(discardRowDialog);
  discardRowDialog.hidden = false;
  syncBackdrop();

  return new Promise((resolve) => {
    const finish = (confirmed: boolean) => {
      discardRowDialog.hidden = true;
      syncBackdrop();
      discardRowCancel.removeEventListener("click", onCancel);
      discardRowConfirm.removeEventListener("click", onConfirm);
      resolve(confirmed);
    };
    const onCancel = () => finish(false);
    const onConfirm = () => finish(true);
    discardRowCancel.addEventListener("click", onCancel);
    discardRowConfirm.addEventListener("click", onConfirm);
  });
}

function confirmDiscardSelected(): Promise<boolean> {
  recenter(discardSelectedDialog);
  discardSelectedDialog.hidden = false;
  syncBackdrop();

  return new Promise((resolve) => {
    const finish = (confirmed: boolean) => {
      discardSelectedDialog.hidden = true;
      syncBackdrop();
      discardSelectedCancel.removeEventListener("click", onCancel);
      discardSelectedConfirm.removeEventListener("click", onConfirm);
      resolve(confirmed);
    };
    const onCancel = () => finish(false);
    const onConfirm = () => finish(true);
    discardSelectedCancel.addEventListener("click", onCancel);
    discardSelectedConfirm.addEventListener("click", onConfirm);
  });
}

async function removeQueueRows(requestedPaths: string[], bulk: boolean) {
  if (interactionBusy()) return;
  const targets = requestedPaths
    .map((path) => rows.get(path))
    .filter((row): row is Row => Boolean(row) && !removingPaths.has(row!.path));
  if (targets.length === 0) return;

  const policies = targets.map((row) => [row, removePolicy(row, interactionBusy())] as const);
  const selectionPolicy = removeSelectionPolicy(targets, interactionBusy());
  if (!selectionPolicy.enabled) return;
  for (const row of targets) removingPaths.add(row.path);
  render();

  const requiresConfirmation = selectionPolicy.requiresDiscardConfirmation;
  const confirmed = !requiresConfirmation || (await (bulk ? confirmDiscardSelected() : confirmDiscardRow()));
  if (!confirmed) {
    for (const row of targets) removingPaths.delete(row.path);
    render();
    return;
  }

  const cleanupRows = policies.filter(([, policy]) => policy.requiresScratchCleanup).map(([row]) => row);
  const failedOutputs = new Set<string>();
  if (cleanupRows.length > 0) {
    try {
      const failures = await invoke<string[]>("discard_files", { paths: cleanupRows.map((row) => row.output!) });
      for (const output of failures) failedOutputs.add(output);
    } catch {
      for (const row of cleanupRows) failedOutputs.add(row.output!);
    }
  }

  const originalPaths = [...rows.keys()];
  const previousActive = selectedOrig;
  const previousAnchor = selectionAnchor;
  const removed = new Set(
    targets.filter((row) => !row.output || !failedOutputs.has(row.output)).map((row) => row.path),
  );
  selectedOrig = activeAfterRemovals(originalPaths, selectedOrig, removed);
  for (const path of removed) {
    rows.delete(path);
    selectedPaths.delete(path);
  }
  for (const row of targets) removingPaths.delete(row.path);
  if (selectedOrig && (selectedPaths.size === 0 || removed.has(previousActive))) selectedPaths.add(selectedOrig);
  selectionAnchor = removed.has(previousAnchor) ? selectedOrig : previousAnchor;
  render();

  if (failedOutputs.size > 0) {
    popup(failedOutputs.size === 1
      ? "Temporary Converted File Could Not Be Removed"
      : "Some Temporary Files Could Not Be Removed");
  }
}

async function removeQueueRow(path: string) {
  await removeQueueRows([path], false);
}

function resetClearedQueue() {
  rows.clear();
  selectedOrig = "";
  selectedPaths.clear();
  selectionAnchor = "";
  selectionExplicit = false;
  reconversionBackups.clear();
  reconversionFailures.clear();
  formatSelect.value = "";
  newPixel.value = "";
  qualityField.hidden = false;
  render();
}

// Asks whether to replace existing files, keep both, or cancel. Resolves once a
// button is pressed.
function confirmOverwrite(count: number): Promise<"replace" | "keep" | "cancel"> {
  const oneCollision = count === 1;
  confirmTitle.textContent = oneCollision ? "File Already Exists" : "Files Already Exist";
  confirmLine.textContent = oneCollision
    ? "A File With That Name Already Exists In This Folder."
    : `${count} File${count === 1 ? "" : "s"} With Those Names Already Exist In This Folder.`;
  recenter(confirmDialog);
  confirmDialog.hidden = false;
  syncBackdrop();

  return new Promise((resolve) => {
    const finish = (choice: "replace" | "keep" | "cancel") => {
      confirmDialog.hidden = true;
      syncBackdrop();
      replaceBtn.removeEventListener("click", onReplace);
      keepBtn.removeEventListener("click", onKeep);
      cancelBtn.removeEventListener("click", onCancel);
      closeBtn.removeEventListener("click", onCancel);
      resolve(choice);
    };
    const onReplace = () => finish("replace");
    const onKeep = () => finish("keep");
    const onCancel = () => finish("cancel");
    const replaceBtn = element<HTMLButtonElement>("confirm-replace");
    const keepBtn = element<HTMLButtonElement>("confirm-keep");
    const cancelBtn = element<HTMLButtonElement>("confirm-cancel");
    const closeBtn = element<HTMLButtonElement>("confirm-close");
    replaceBtn.addEventListener("click", onReplace);
    keepBtn.addEventListener("click", onKeep);
    cancelBtn.addEventListener("click", onCancel);
    closeBtn.addEventListener("click", onCancel);
  });
}

type SavedResult =
  | { status: "ok"; from: string; path: string }
  | { status: "failed"; from: string; error: string };

// Copies each converted result from scratch to the chosen destination.
async function saveResults() {
  if (saving) return;
  const ready = savableRows();
  if (ready.length === 0) return;
  pendingFolder = saveLocation.value.trim();
  if (pendingFolder === "") {
    popup("Choose A Folder");
    return;
  }
  const single = ready.length === 1;

  const copies = buildSaveCopies(
    saveablePolicyRows(ready),
    pendingFolder,
    saveNameInput.value,
  );
  if (single) saveNameInput.value = stemOf(baseName(copies[0].to));

  saveDialog.hidden = true;
  saving = true;
  syncBackdrop();
  render();

  // If any target already exists, ask before writing.
  let existing: string[];
  try {
    existing = await invoke<string[]>("existing_targets", { paths: copies.map((copy) => copy.to) });
  } catch {
    saving = false;
    popup("Destination Could Not Be Checked");
    render();
    return;
  }
  let overwrite = false;
  if (existing.length > 0) {
    const choice = await confirmOverwrite(existing.length);
    if (choice === "cancel") {
      saving = false;
      render();
      recenter(saveDialog);
      saveDialog.hidden = false;
      syncBackdrop();
      return;
    }
    overwrite = choice === "replace";
  }

  let results: SavedResult[];
  try {
    results = await invoke<SavedResult[]>("save_files", { copies, overwrite });
  } catch {
    saving = false;
    popup("Files Could Not Be Saved");
    render();
    return;
  }

  const byFrom = new Map(results.map((result) => [result.from, result]));
  for (const row of ready) {
    const result = byFrom.get(row.output!);
    if (result?.status === "ok") {
      // Keep final row states visible until the user deliberately removes or
      // clears them. The destination file is never affected by those actions.
      rows.set(row.path, {
        ...row,
        state: "saved",
        detail: "Saved",
        error: undefined,
        savedPath: result.path,
      });
    } else if (result?.status === "failed") {
      rows.set(row.path, { ...row, state: "failed", detail: "Failed", error: result.error });
    } else {
      rows.set(row.path, { ...row, state: "failed", detail: "Failed", error: "No Save Result Returned" });
    }
  }
  saving = false;
  syncBackdrop();
  render();
}

listen<Progress>("conversion-progress", ({ payload }) => {
  const row = rows.get(payload.path);
  if (!row) return;

  const presentation = progressPresentation(
    payload.status,
    payload.status === "failed" ? payload.error : undefined,
  );
  if (payload.status === "done") {
    const previous = reconversionBackups.get(payload.path);
    rows.set(payload.path, {
      ...row,
      ...presentation,
      output: payload.output,
      outputBytes: payload.bytes,
    });
    reconversionBackups.delete(payload.path);
    if (previous?.output && previous.output !== payload.output) {
      void invoke<string[]>("discard_files", { paths: [previous.output] })
        .then((failures) => {
          if (failures.length > 0) popup("Previous Temporary File Could Not Be Removed");
        })
        .catch(() => popup("Previous Temporary File Could Not Be Removed"));
    }
  } else {
    const previous = reconversionBackups.get(payload.path);
    if (previous) {
      rows.set(payload.path, { ...previous, reconvert: false, error: presentation.error });
      reconversionBackups.delete(payload.path);
      if (payload.status === "failed") reconversionFailures.add(previous.info?.name ?? previous.path);
    } else {
      rows.set(payload.path, { ...row, ...presentation });
    }
  }
  updateRow(payload.path);
});

listen("conversion-finished", () => {
  running = false;
  conversionStarting = false;
  cancellationRequested = false;
  for (const [path, row] of rows) {
    if (row.state === "working") {
      const previous = reconversionBackups.get(path);
      if (previous) {
        rows.set(path, { ...previous, reconvert: false, error: "Reconversion Did Not Finish" });
        reconversionFailures.add(previous.info?.name ?? previous.path);
      } else {
        rows.set(path, { ...row, state: "failed", detail: "Failed", error: "Conversion Did Not Finish" });
      }
    }
  }
  activeConversionPaths.clear();
  reconversionBackups.clear();
  selectedOrig = "";
  selectedPaths.clear();
  selectionAnchor = "";
  selectionExplicit = false;
  render();
  if (reconversionFailures.size > 0) {
    popup("Some Files Could Not Be Reconverted", { names: [...reconversionFailures] });
    reconversionFailures.clear();
  }
});

listen<{ paths: string[] }>("tauri://drag-drop", ({ payload }) => {
  dropZone.classList.remove("over");
  if (!chooseFilesEnabled(rows.values(), interactionBusy(), pickerOpen || dropProcessing)) return;
  dropProcessing = true;
  render();
  void addPaths(payload.paths).finally(() => {
    dropProcessing = false;
    render();
  });
});
listen("tauri://drag-enter", () => {
  if (chooseFilesEnabled(rows.values(), interactionBusy(), pickerOpen || dropProcessing)) dropZone.classList.add("over");
});
listen("tauri://drag-leave", () => dropZone.classList.remove("over"));

browseButton.addEventListener("click", async () => {
  if (!chooseFilesEnabled(rows.values(), interactionBusy(), pickerOpen)) return;
  pickerOpen = true;
  render();
  try {
    const chosen = await open({
      multiple: true,
      filters: [{ name: "Images", extensions: EXTENSIONS }],
    });
    const selected = Array.isArray(chosen) ? chosen : typeof chosen === "string" ? [chosen] : [];
    if (selected.length > 0) await addPaths(selected);
  } catch {
    popup("Could Not Open File Picker");
  } finally {
    pickerOpen = false;
    render();
  }
});

formatSelect.addEventListener("change", () => {
  // Lossless formats do not use a lossy compression-quality setting.
  qualityField.hidden = formatSelect.value === "png" || formatSelect.value === "tiff";
  requestSelectedReconversion();
  updateConvertControl();
});

qualityInput.addEventListener("input", () => {
  qualityValue.textContent = qualityInput.value;
  requestSelectedReconversion();
});

// Picking a file's original dimension highlights that row in the queue.
origDim.addEventListener("change", () => {
  applyUserSelection({
    selected: origDim.value ? [origDim.value] : [],
    active: origDim.value,
    anchor: origDim.value,
  });
  render();
});

// Eight digits are displayed as "XXXX * YYYY". Formatting happens while the
// user types; validation waits until focus leaves the field.
newPixel.addEventListener("input", () => {
  newPixel.value = formatDimensionInput(newPixel.value);
  newPixel.setSelectionRange(newPixel.value.length, newPixel.value.length);
  const targets = selectedPaths.size > 0 ? selectedPaths : new Set([selectedOrig]);
  for (const path of targets) {
    const row = rows.get(path);
    if (row) rows.set(path, { ...row, targetDimension: newPixel.value });
  }
  requestSelectedReconversion();
  updateConvertControl();
});

newPixel.addEventListener("keydown", (event) => {
  if (
    event.key === "Backspace" &&
    newPixel.selectionStart === newPixel.value.length &&
    newPixel.selectionEnd === newPixel.value.length &&
    /^\d{4} \* $/.test(newPixel.value)
  ) {
    event.preventDefault();
    newPixel.value = newPixel.value.slice(0, 3);
    newPixel.setSelectionRange(newPixel.value.length, newPixel.value.length);
    const targets = selectedPaths.size > 0 ? selectedPaths : new Set([selectedOrig]);
    for (const path of targets) {
      const row = rows.get(path);
      if (row) rows.set(path, { ...row, targetDimension: newPixel.value });
    }
    requestSelectedReconversion();
    updateConvertControl();
  }
});

newPixel.addEventListener("blur", () => {
  const value = newPixel.value.trim();
  if (value === "") return;

  if (dimensionsAreValid(value)) return;

  returnFocusToDimension = true;
  popup("Min Is 1024 px · Max Is 7680 px");
});

element<HTMLButtonElement>("notice-close").addEventListener("click", closePopup);
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    closePopup();
    closeSaveDialog();
    return;
  }

  const target = event.target as HTMLElement | null;
  const editingText = Boolean(target?.closest("input, textarea, select, [contenteditable='true']"));
  const popupOpen = Boolean(document.querySelector(".toast:not([hidden])"));
  const paths = [...rows.keys()];

  const selectAllShortcut = isSelectAllShortcut(event.key, event.metaKey, event.ctrlKey, IS_MAC);
  if (selectAllShortcut && !editingText && !popupOpen && !interactionBusy() && paths.length > 0) {
    event.preventDefault();
    applyUserSelection(selectAll(paths, selectedOrig));
    render();
    return;
  }

  if ((event.key === "ArrowUp" || event.key === "ArrowDown") && !editingText && !popupOpen && !interactionBusy()) {
    if (paths.length === 0) return;
    event.preventDefault();
    applyUserSelection(
      arrowSelection(
        paths,
        selectedOrig,
        selectionAnchor,
        event.key === "ArrowUp" ? -1 : 1,
        event.shiftKey,
      ),
    );
    render();
    rowEls.get(selectedOrig)?.scrollIntoView({ block: "nearest" });
    return;
  }

  if (keyboardDeleteAllowed(event.key, selectedPaths.size, interactionBusy(), popupOpen, editingText)) {
    event.preventDefault();
    void removeQueueRows([...selectedPaths], selectedPaths.size > 1);
  }
});

makeDraggable(notice);
makeDraggable(saveDialog);
makeDraggable(confirmDialog);
makeDraggable(discardDialog);
makeDraggable(discardRowDialog);
makeDraggable(discardSelectedDialog);
saveClose.addEventListener("click", closeSaveDialog);
saveCancel.addEventListener("click", closeSaveDialog);
saveConfirm.addEventListener("click", () => void saveResults());
saveLocation.addEventListener("input", () => {
  pendingFolder = saveLocation.value;
  updateSaveDestinationDisplay();
});
saveChange.addEventListener("click", async () => {
  if (saving || saveFolderPickerOpen) return;
  saveFolderPickerOpen = true;
  render();
  try {
    const chosen = await open({ directory: true, multiple: false, title: "Choose A Folder" });
    if (typeof chosen === "string") {
      pendingFolder = chosen;
    }
  } catch {
    saveDialog.hidden = true;
    popup("Could Not Open Folder Picker");
  } finally {
    saveFolderPickerOpen = false;
    updateSaveDestinationDisplay();
    render();
  }
});

clearButton.addEventListener("click", async () => {
  const policy = clearPolicy(rows.values(), interactionBusy());
  if (!policy.enabled) return;

  if (policy.requiresDiscardConfirmation) {
    const confirmed = await confirmDiscardAll();
    if (!confirmed) return;

    const temporaryPaths = [...rows.values()]
      .filter((row) => Boolean(row.output) && row.state !== "saved")
      .map((row) => row.output!);
    try {
      const failures = await invoke<string[]>("discard_files", { paths: temporaryPaths });
      if (failures.length > 0) {
        popup("Some Temporary Files Could Not Be Removed");
        return;
      }
      resetClearedQueue();
    } catch {
      popup("Temporary Files Could Not Be Removed");
    }
    return;
  }

  const savedTemporaryPaths = [...rows.values()]
    .filter((row) => Boolean(row.output))
    .map((row) => row.output!);
  if (savedTemporaryPaths.length > 0) {
    try {
      const failures = await invoke<string[]>("discard_files", { paths: savedTemporaryPaths });
      if (failures.length > 0) {
        popup("Some Temporary Files Could Not Be Removed");
        return;
      }
      resetClearedQueue();
    } catch {
      popup("Temporary Files Could Not Be Removed");
    }
    return;
  }

  resetClearedQueue();
});

removeSelectedButton.addEventListener("click", () => {
  void removeQueueRows([...selectedPaths], true);
});
convertButton.addEventListener("click", () => void convert());
saveButton.addEventListener("click", openSaveDialog);

render();
