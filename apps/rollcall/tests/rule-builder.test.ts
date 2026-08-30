import assert from "node:assert/strict";
import test from "node:test";
import { buildRenameRules, type RuleInputs } from "../src/rule-builder.ts";

const inputs: RuleInputs = {
  prefix: "Trip-",
  suffix: "-Final",
  find: "IMG",
  replace: "Photo",
  caseStyle: "upper",
  counterStart: 1,
  counterPadding: 3,
  numberPosition: "end",
  datePosition: "beginning",
  dateSource: "today",
  dateFormat: "yyyy-mm-dd",
  customDate: "",
};

test("positive: each dropdown function creates only its own rule", () => {
  assert.equal(buildRenameRules("add-text", inputs).template, "Trip-{name}-Final");
  assert.equal(buildRenameRules("replace-text", inputs).find, "IMG");
  assert.equal(buildRenameRules("change-case", inputs).caseStyle, "upper");
  assert.equal(buildRenameRules("add-numbers", inputs).template, "{name}-{n}");
  assert.equal(buildRenameRules("add-numbers", { ...inputs, numberPosition: "beginning" }).template, "{n}-{name}");
  assert.equal(buildRenameRules("add-date", inputs).template, "{date}-{name}");
});

test("negative: unsupported dropdown values are rejected", () => {
  assert.throws(() => buildRenameRules("delete-files", inputs), /Supported Rename Function/u);
  assert.throws(() => buildRenameRules("add-numbers", { ...inputs, numberPosition: "" }), /Number Position/u);
  assert.throws(() => buildRenameRules("add-date", { ...inputs, dateFormat: "" }), /Date Format/u);
  assert.throws(() => buildRenameRules("add-date", { ...inputs, dateSource: "" }), /Date Source/u);
  assert.throws(() => buildRenameRules("add-date", { ...inputs, dateSource: "custom", customDate: "" }), /Custom Date/u);
  assert.throws(() => buildRenameRules("add-date", { ...inputs, dateSource: "custom", customDate: "2026-02-30" }), /Custom Date/u);
});

test("positive: Custom Date is kept only for the Add Date custom source", () => {
  const custom = buildRenameRules("add-date", { ...inputs, dateSource: "custom", customDate: "2026-12-31" });
  assert.equal(custom.customDate, "2026-12-31");
  assert.equal(custom.dateSource, "custom");

  const today = buildRenameRules("add-date", { ...inputs, dateSource: "today", customDate: "2026-12-31" });
  assert.equal(today.customDate, "");
});

test("boundary: counter values are integers and remain inside supported limits", () => {
  const low = buildRenameRules("add-numbers", { ...inputs, counterStart: -10, counterPadding: 0 });
  assert.equal(low.counterStart, 0);
  assert.equal(low.counterPadding, 1);

  const high = buildRenameRules("add-numbers", {
    ...inputs,
    counterStart: 2_000_000.9,
    counterPadding: 99,
  });
  assert.equal(high.counterStart, 999_999);
  assert.equal(high.counterPadding, 3);
});

test("edge: empty Add Text fields produce the unchanged-name template", () => {
  const rules = buildRenameRules("add-text", { ...inputs, prefix: "", suffix: "" });
  assert.equal(rules.template, "{name}");
  assert.equal(rules.find, "");
  assert.equal(rules.caseStyle, "keep");
});
