import assert from "node:assert/strict";
import test from "node:test";
import { renameButtonLabel } from "../src/presentation.ts";

test("save action uses a simple label and shows busy state", () => {
  assert.equal(renameButtonLabel(1, false), "Save As");
  assert.equal(renameButtonLabel(2, false), "Save As");
  assert.equal(renameButtonLabel(2, true), "Saving…");
});
