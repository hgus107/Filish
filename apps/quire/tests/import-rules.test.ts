import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import {
  allowedImportExtensions,
  clearIsEnabled,
  duplicateNotice,
  importIsReady,
  partitionImportCandidates,
  removeSelectedIsEnabled,
  validImportDetails,
  validPageSelection,
  validRotatePageSelection,
} from "../src/import-rules.ts";
import { outputChoices, type TaskFunction } from "../src/task-options.ts";

const allTasks: TaskFunction[] = ["to-pdf", "extract", "merge", "split", "rotate", "compress", "ocr"];

test("every function and output combination can enable Import Files", () => {
  for (const task of allTasks) {
    for (const output of outputChoices(task)) {
      const pageRanges = task === "split" && output.value === "ranges" ? "1-3, 5" : "";
      assert.equal(importIsReady({ task, outputType: output.value, pageRanges, rotatePages: "", pickerOpen: false, importing: false }), true, `${task}/${output.value}`);
    }
  }
});

test("Import Files ignores task options and disables only during import or an open picker", () => {
  assert.equal(importIsReady({ task: null, outputType: "", pageRanges: "", rotatePages: "", pickerOpen: false, importing: false }), true);
  assert.equal(importIsReady({ task: "to-pdf", outputType: "", pageRanges: "", rotatePages: "", pickerOpen: false, importing: false }), true);
  assert.equal(importIsReady({ task: "split", outputType: "ranges", pageRanges: "", rotatePages: "", pickerOpen: false, importing: false }), true);
  assert.equal(importIsReady({ task: "split", outputType: "ranges", pageRanges: "3-1", rotatePages: "", pickerOpen: false, importing: false }), true);
  assert.equal(importIsReady({ task: "merge", outputType: "merged", pageRanges: "", rotatePages: "", pickerOpen: true, importing: false }), false);
  assert.equal(importIsReady({ task: "merge", outputType: "merged", pageRanges: "", rotatePages: "", pickerOpen: false, importing: true }), false);
});

test("Import does not require Function options while Apply validation remains separate", () => {
  const source = readFileSync(new URL("../src/main.ts", import.meta.url), "utf8");
  const addPaths = source.match(/async function addPaths\(paths: string\[\]\): Promise<void> \{[\s\S]*?\n\}/)?.[0] ?? "";
  assert.doesNotMatch(addPaths, /outputSelect|validDetails|Complete the required options/);
  assert.match(source, /convertButton\.disabled = saveDialogOpen[\s\S]*?!selectedFunction\(\) \|\| queue\.length === 0/);
  assert.match(source, /showNotice\("Please Select an Output Type\."\)/);
});

test("page selections cover positive, negative, edge, and boundary syntax", () => {
  for (const value of ["1", "1-3", "1, 3, 5", "1-3, 5, 8-10", " 1 - 3 , 5 ", "9007199254740991"]) {
    assert.equal(validPageSelection(value), true, value);
  }
  for (const value of ["", " ", "0", "01", "-1", "3-1", "abc", "1,", ",1", "1,,3", "1-3, 3", "1, 1", "9007199254740992"]) {
    assert.equal(validPageSelection(value), false, value);
  }
  assert.equal(validPageSelection("all", true), true);
  assert.equal(validPageSelection("all"), false);
  assert.equal(validImportDetails("split", "ranges", "", ""), false);
  assert.equal(validImportDetails("split", "ranges", "1-3, 3", ""), false);
  assert.equal(validImportDetails("split", "individual", "", ""), true);
});

test("Rotate page modes validate all pages, a single page, and one page range", () => {
  assert.equal(validRotatePageSelection("all", ""), true);
  assert.equal(validRotatePageSelection("all", "ignored"), true);

  for (const value of ["1", "7", "9007199254740991", " 7 "]) {
    assert.equal(validRotatePageSelection("single", value), true, value);
  }
  for (const value of ["", "0", "-1", "1.5", "1-3", "1,3", "abc", "9007199254740992"]) {
    assert.equal(validRotatePageSelection("single", value), false, value);
  }

  for (const value of ["1-1", "3-5", " 3 - 5 ", "1-9007199254740991"]) {
    assert.equal(validRotatePageSelection("range", value), true, value);
  }
  for (const value of ["", "0-1", "1-0", "5-3", "1", "1.5-3", "1-3,5", "abc", "1-9007199254740992"]) {
    assert.equal(validRotatePageSelection("range", value), false, value);
  }

  assert.equal(validImportDetails("rotate", "clockwise", "", "", "all"), true);
  assert.equal(validImportDetails("rotate", "clockwise", "", "7", "single"), true);
  assert.equal(validImportDetails("rotate", "clockwise", "", "", "single"), false);
  assert.equal(validImportDetails("rotate", "clockwise", "", "3-5", "range"), true);
  assert.equal(validImportDetails("rotate", "clockwise", "", "5-3", "range"), false);
});

test("Rotate controls expose the approved dropdown and conditional entry fields", () => {
  const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
  const source = readFileSync(new URL("../src/main.ts", import.meta.url), "utf8");
  const styles = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
  assert.match(html, /<select id="rotate-pages-mode"[^>]*>[\s\S]*?<option value="all" selected>All Pages<\/option>/);
  assert.match(html, /<option value="single">Single Page<\/option>/);
  assert.match(html, /<option value="range">Page Range<\/option>/);
  assert.match(html, /id="rotate-pages-static">All Pages/);
  assert.match(styles, /\.rotate-pages-control:has\(#rotate-pages\[hidden\]\) #rotate-pages-mode \{[\s\S]*?left: 0;[\s\S]*?width: 100%;/);
  assert.match(styles, /\.rotate-pages-arrow \{[\s\S]*?left: 17px;/);
  assert.match(source, /rotatePagesControl\.addEventListener\("click"[\s\S]*?rotatePagesMode\.showPicker\(\)/);
  assert.match(html, /id="rotate-pages-prompt" hidden>Enter Page Number:/);
  assert.match(html, /id="rotate-pages"/);
  assert.doesNotMatch(html, /rotate-pages-value-field/);
  assert.match(source, /async function changeRotatePageMode\(\)[\s\S]*?await discardOutputs\(\)/);
  assert.match(source, /async function changeRotatePageValue\(\)[\s\S]*?await discardOutputs\(\)/);
});

test("file filters match each task", () => {
  assert.deepEqual(allowedImportExtensions(null), ["doc", "docx", "html", "htm", "md", "markdown", "odt", "pdf", "rtf", "txt"]);
  assert.deepEqual(allowedImportExtensions("to-pdf"), ["doc", "docx", "html", "htm", "md", "markdown", "odt", "rtf", "txt"]);
  for (const task of allTasks.filter((task) => task !== "to-pdf")) {
    assert.deepEqual(allowedImportExtensions(task), ["pdf"]);
  }
});

test("duplicate notices name one or many files in sentence case", () => {
  assert.equal(duplicateNotice(["XYZ.pdf"]), "XYZ.pdf is already in the queue and was not added again.");
  assert.equal(
    duplicateNotice(["XYZ.pdf", "ABDM.pdf", "RTY-DER.pdf"]),
    "One or more files are already in the queue and were not added.",
  );
});

test("duplicate partitioning keeps only the first path and names every ignored duplicate", () => {
  const files = [
    { path: "/a.pdf", name: "Alpha.pdf" },
    { path: "/b.pdf", name: "Beta.pdf" },
    { path: "/b.pdf", name: "Beta.pdf" },
    { path: "/c.pdf", name: "Gamma.pdf" },
  ];
  const result = partitionImportCandidates(files, ["/a.pdf"]);
  assert.deepEqual(result.fresh.map((file) => file.name), ["Beta.pdf", "Gamma.pdf"]);
  assert.deepEqual(result.duplicateNames, ["Alpha.pdf", "Beta.pdf"]);
});

test("Clear and Remove Selected button states depend only on queue and selection counts", () => {
  assert.equal(clearIsEnabled(0), false);
  assert.equal(clearIsEnabled(1), true);
  assert.equal(clearIsEnabled(20_000), true);
  assert.equal(removeSelectedIsEnabled(0), false);
  assert.equal(removeSelectedIsEnabled(1), true);
  assert.equal(removeSelectedIsEnabled(20_000), true);
});
