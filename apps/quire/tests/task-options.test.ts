import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import {
  acceptsExtension,
  completedStatusLabel,
  conversionIssue,
  conversionTargets,
  formatBytes,
  outputChoices,
  outputLabel,
  taskFailureFallback,
  taskFailureSummary,
} from "../src/task-options.ts";

test("function output choices use the approved labels", () => {
  assert.equal(outputChoices("to-pdf")[0]?.label, "One PDF");
  assert.equal(outputChoices("to-pdf")[1]?.label, "Separate PDFs");
  assert.equal(outputChoices("extract")[2]?.label, "Markdown and Text");
  assert.deepEqual(outputChoices("compress").map((choice) => choice.label), ["Smallest", "Standard", "Largest"]);
  assert.equal(outputLabel("compress", "standard"), "Standard");
  assert.equal(outputLabel("compress", "unknown"), "Choose a Type");
});

test("Function menu starts with Choose a Function and keeps functions alphabetical", () => {
  const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
  const labels = [...html.matchAll(/<option value="(?:compress|to-pdf|extract|ocr|merge|rotate|split)"(?: selected)?>([^<]+)<\/option>/g)]
    .map((match) => match[1]);
  assert.deepEqual(labels, [
    "Compress PDF",
    "Convert to PDF",
    "Extract PDF to Markdown or Text",
    "Make Scanned PDF Searchable",
    "Merge PDFs",
    "Rotate PDF",
    "Split PDF",
  ]);
  assert.match(html, /<option value="" selected disabled>Choose a Function<\/option>/);
  assert.doesNotMatch(html, /<option value="to-pdf" selected>/);
});

test("Apply changes only to Cancel or Cancelling while work runs and has no progress state", () => {
  const source = readFileSync(new URL("../src/main.ts", import.meta.url), "utf8");
  const backend = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
  const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
  const styles = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
  assert.doesNotMatch(source, /showProgress|noticePurpose.*progress|Stopping the current conversion safely/);
  assert.doesNotMatch(source, /"Converting"|state: "working"|TaskProgress|activeEligiblePaths/);
  assert.doesNotMatch(source, /listen<TaskProgress>\("task-progress"/);
  assert.doesNotMatch(backend, /task-progress|Starting Conversion|struct TaskProgress/);
  assert.match(html, />Apply<\/button>/);
  assert.match(source, /cancellationRequested \? "Cancelling" : "Cancel"/);
  assert.match(source, /activeRunId = request\.runId;\s*renderApplyButton\(\);/);
  assert.match(source, /taskControls\.inert = busy \|\| saveDialogOpen/);
  assert.doesNotMatch(source, /Convert Again|Discard the current converted files and convert again|Please save the converted files or clear the current queue/);
  assert.doesNotMatch(source, /Files? Saved\./);
  assert.doesNotMatch(html, /Drag Files or Use the Arrow Buttons to Set the PDF Order/);
  assert.match(source, /showNotice\("Please Select an Output Type\."\)/);
  assert.match(source, /const taskRunning = activeRunId !== "";[\s\S]*?convertButton\.textContent = taskRunning[\s\S]*?: "Apply";/);
  assert.match(source, /showNotice\(invalidDetailsMessage\(task\)\)/);
  assert.match(styles, /\.action-bar \.button\.secondary \{\s*height: 28\.8px;/);
  assert.match(styles, /\.function-field > span,[\s\S]*?#output-type:disabled \{\s*color: var\(--ink\);/);
});

test("Save is modal, Apply keeps its label during Save, and successful Split clears its range", () => {
  const source = readFileSync(new URL("../src/main.ts", import.meta.url), "utf8");
  const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
  const styles = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
  assert.match(html, /id="save-modal-backdrop"[^>]*hidden/);
  assert.match(html, /id="save-dialog"[^>]*aria-modal="true"/);
  assert.match(source, /function setSaveDialogOpen\(open: boolean\)[\s\S]*?child as HTMLElement\)\.inert = open/);
  assert.match(source, /setSaveDialogOpen\(true\);[\s\S]*?saveDialog\.hidden = false/);
  assert.match(source, /saveDialog\.hidden = true;[\s\S]*?setSaveDialogOpen\(false\)/);
  assert.match(source, /if \(saveDialogOpen \|\| editing \|\| busy \|\| queue\.length === 0\) return/);
  assert.match(source, /if \(busy \|\| saveDialogOpen\) return;[\s\S]*?readyForImport\(\)/);
  assert.match(source, /if \(task === "split" && convertedOutputs\.length > 0\) pageRangesInput\.value = ""/);
  assert.match(styles, /\.save-modal-backdrop \{[\s\S]*?z-index: 4/);
  assert.match(styles, /\.save-dialog \{\s*z-index: 5/);
});

test("every Function has its approved completion and failure language", () => {
  const tasks = ["compress", "to-pdf", "extract", "ocr", "merge", "rotate", "split"] as const;
  assert.deepEqual(tasks.map(completedStatusLabel), ["Compressed", "Converted", "Extracted", "Searchable", "Merged", "Rotated", "Done"]);
  assert.equal(taskFailureFallback("rotate"), "Rotation Failed.");
  assert.equal(taskFailureSummary("merge", 3), "3 Files Could Not Be Merged.");
});

test("single-output Functions choose their output internally and hide Output Type", () => {
  const source = readFileSync(new URL("../src/main.ts", import.meta.url), "utf8");
  const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
  assert.match(html, /id="output-field"/);
  assert.match(source, /outputField\.hidden = choices\.length === 1/);
  assert.match(source, /if \(choices\.length === 1\) outputSelect\.value = choices\[0\]!\.value/);
  assert.deepEqual(outputChoices("merge"), [{ value: "merged", label: "One Combined PDF" }]);
  assert.deepEqual(outputChoices("ocr"), [{ value: "searchable", label: "Searchable PDF" }]);
  assert.match(source, /outputs\.length > 0 && task === "to-pdf" && outputSelect\.value === "combined"/);
});

test("a single conversion failure shows its exact backend message", () => {
  const source = readFileSync(new URL("../src/main.ts", import.meta.url), "utf8");
  assert.match(source, /failureMessages\.push\(message\)/);
  assert.match(source, /uniqueMessages\.length === 1[\s\S]*?uniqueMessages\[0\]!/);
});

test("temporary-file cleanup errors remain visible until explicitly dismissed", () => {
  const source = readFileSync(new URL("../src/main.ts", import.meta.url), "utf8");
  assert.match(source, /showNotice\("Some Temporary Files Could Not Be Removed\.", true, true\)/);
  assert.match(source, /function hideNotice\(force = false\)[\s\S]*?if \(noticePersistent && !force\) return;/);
  assert.match(source, /dismissNotice\.addEventListener\("click", \(\) => hideNotice\(true\)\)/);
});

test("Function and Output changes keep the queue while resetting outputs and selection", () => {
  const source = readFileSync(new URL("../src/main.ts", import.meta.url), "utf8");
  const changeFunction = source.match(/async function changeFunction\(\): Promise<void> \{[\s\S]*?\n\}/)?.[0] ?? "";
  const changeOutput = source.match(/async function changeOutput\(\): Promise<void> \{[\s\S]*?\n\}/)?.[0] ?? "";
  for (const handler of [changeFunction, changeOutput]) {
    assert.match(handler, /await discardOutputs\(\);/);
    assert.match(handler, /clearSelection\(\);/);
    assert.doesNotMatch(handler, /emptyQueueState|functionChangeIsBlocked|outputChangeIsBlocked|showNotice/);
  }
});

test("Shift mouse selection prevents native text highlighting before row click", () => {
  const source = readFileSync(new URL("../src/main.ts", import.meta.url), "utf8");
  assert.match(source, /row\.addEventListener\("mousedown"[\s\S]*?event\.shiftKey[\s\S]*?event\.preventDefault\(\)/);
});

test("each newly opened Save popup starts with an empty destination", () => {
  const source = readFileSync(new URL("../src/main.ts", import.meta.url), "utf8");
  const showSaveDialog = source.match(/function showSaveDialog\(\): void \{[\s\S]*?\n\}/)?.[0] ?? "";
  assert.match(showSaveDialog, /saveDestination\.value = "";/);
  assert.match(showSaveDialog, /applySave\.disabled = true;/);
  assert.match(source, /saveDestination\.addEventListener\("keydown"[\s\S]*?event\.key !== "Enter" \|\| applySave\.disabled[\s\S]*?applySaveResults\(false\)/);
});

test("popup messages remain selectable while popup dragging remains enabled", () => {
  const source = readFileSync(new URL("../src/main.ts", import.meta.url), "utf8");
  const styles = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
  assert.match(source, /pendingDraggedPopup = popup/);
  assert.match(source, /pointTouchesText\(selectableText, event\.clientX, event\.clientY\)/);
  assert.match(source, /if \(distance < 5\) return/);
  assert.doesNotMatch(source, /popupDragStartY = event\.clientY;\s*event\.preventDefault\(\);/);
  assert.match(styles, /\.notice-copy \{[\s\S]*?cursor: text;[\s\S]*?user-select: text;/);
});

test("PDF tools reject non-PDF inputs", () => {
  assert.equal(acceptsExtension("merge", "pdf"), true);
  assert.equal(acceptsExtension("merge", "docx"), false);
  assert.equal(acceptsExtension("to-pdf", "doc"), true);
  assert.equal(acceptsExtension("to-pdf", "docx"), true);
  assert.equal(acceptsExtension("to-pdf", ".md"), true);
  assert.equal(acceptsExtension("to-pdf", "pdf"), false);
});

test("every Function accepts only its complete approved extension matrix", () => {
  const extensions = ["doc", "docx", "html", "htm", "md", "markdown", "odt", "pdf", "rtf", "txt", "jpg", "ps", "", ".PDF"];
  const documentExtensions = new Set(["doc", "docx", "html", "htm", "md", "markdown", "odt", "rtf", "txt"]);
  for (const task of ["to-pdf", "extract", "merge", "split", "rotate", "compress", "ocr"] as const) {
    for (const extension of extensions) {
      const normalized = extension.toLowerCase().replace(/^\./, "");
      const expected = task === "to-pdf" ? documentExtensions.has(normalized) : normalized === "pdf";
      assert.equal(acceptsExtension(task, extension), expected, `${task}/${extension || "empty"}`);
    }
  }
});

test("file sizes are readable", () => {
  assert.equal(formatBytes(512), "512 B");
  assert.equal(formatBytes(1536), "1.5 KB");
  assert.equal(formatBytes(5 * 1024 * 1024), "5 MB");
});

test("Apply targets Ready selections or every Ready compatible file when nothing is selected", () => {
  const files = [
    { path: "first.pdf", state: "ready" },
    { path: "second.pdf", state: "converted" },
    { path: "third.pdf", state: "ready" },
  ];
  assert.deepEqual(conversionTargets("compress", files, new Set()).map((file) => file.path), ["first.pdf", "third.pdf"]);
  assert.deepEqual(conversionTargets("rotate", files, new Set(["third.pdf"])).map((file) => file.path), ["third.pdf"]);
  assert.deepEqual(conversionTargets("extract", files, new Set(["second.pdf"])), []);
  assert.deepEqual(conversionTargets("split", files, new Set()), []);
  assert.deepEqual(conversionTargets("split", files, new Set(["first.pdf"])).map((file) => file.path), ["first.pdf"]);
  assert.deepEqual(conversionTargets("merge", files, new Set(["first.pdf"])), []);
  assert.deepEqual(conversionTargets("merge", files, new Set()).map((file) => file.path), ["first.pdf", "third.pdf"]);
});

test("conversion validation explains empty, unsupported, selected, and Split cardinality cases", () => {
  const ready = [{ path: "first.pdf", state: "ready" }, { path: "second.pdf", state: "ready" }];
  const unavailable = [{ path: "first.pdf", state: "unsupported" }];
  assert.equal(conversionIssue("compress", ready, new Set()), "");
  assert.equal(conversionIssue("compress", unavailable, new Set()), "No ready files in the queue are supported by this function.");
  assert.equal(conversionIssue("to-pdf", [], new Set()), "Add Non-PDF Files to Convert.");
  assert.equal(conversionIssue("compress", ready, new Set(["missing.pdf"])), "Select at least one ready file supported by this function.");
  assert.equal(conversionIssue("split", [ready[0]!], new Set()), "");
  assert.equal(conversionIssue("split", ready, new Set()), "Only One PDF Can Be Split at a Time. Please Select One PDF.");
  assert.equal(conversionIssue("split", ready, new Set(["first.pdf"])), "");
  assert.equal(conversionIssue("split", ready, new Set(["first.pdf", "second.pdf"])), "Only One PDF Can Be Split at a Time. Please Select One PDF.");
  assert.equal(conversionIssue("split", unavailable, new Set()), "Only One PDF Can Be Split at a Time. Please Select One PDF.");
  assert.equal(conversionIssue("merge", [ready[0]!], new Set()), "Select at least two ready PDFs to merge.");
  assert.equal(conversionIssue("merge", ready, new Set()), "");
});

test("conversion targeting covers every queue state, selection mode, and Function", () => {
  const files = [
    { path: "ready-a.pdf", state: "ready" },
    { path: "ready-b.pdf", state: "ready" },
    { path: "converted.pdf", state: "converted" },
    { path: "failed.pdf", state: "failed" },
    { path: "unsupported.pdf", state: "unsupported" },
    { path: "working.pdf", state: "working" },
  ];
  for (const task of ["to-pdf", "extract", "rotate", "compress", "ocr"] as const) {
    assert.deepEqual(conversionTargets(task, files, new Set()).map((file) => file.path), ["ready-a.pdf", "ready-b.pdf"], task);
    assert.deepEqual(conversionTargets(task, files, new Set(["ready-b.pdf"])).map((file) => file.path), ["ready-b.pdf"], task);
    assert.deepEqual(conversionTargets(task, files, new Set(["converted.pdf", "failed.pdf", "unsupported.pdf", "working.pdf"])), [], task);
    assert.deepEqual(conversionTargets(task, files, new Set(["ready-a.pdf", "converted.pdf"])).map((file) => file.path), ["ready-a.pdf"], task);
  }
  assert.deepEqual(conversionTargets("merge", files, new Set()).map((file) => file.path), ["ready-a.pdf", "ready-b.pdf"]);
  assert.deepEqual(conversionTargets("merge", files, new Set(["ready-a.pdf"])), []);
  assert.deepEqual(conversionTargets("split", files, new Set()), []);
  assert.deepEqual(conversionTargets("split", files, new Set(["ready-a.pdf"])).map((file) => file.path), ["ready-a.pdf"]);
  assert.deepEqual(conversionTargets("split", files, new Set(["ready-a.pdf", "ready-b.pdf"])), []);
  assert.deepEqual(conversionTargets("split", files, new Set(["failed.pdf"])), []);
});
