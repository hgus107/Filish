import assert from "node:assert/strict";
import test from "node:test";
import { adjacentPath, pathsInRange } from "../src/queue-selection.ts";

const paths = ["a.txt", "b.txt", "c.txt", "d.txt"];

test("positive: range selection works in both drag directions", () => {
  assert.deepEqual(pathsInRange(paths, "b.txt", "d.txt"), ["b.txt", "c.txt", "d.txt"]);
  assert.deepEqual(pathsInRange(paths, "d.txt", "b.txt"), ["b.txt", "c.txt", "d.txt"]);
});

test("boundary: keyboard selection stops at the first and last file", () => {
  assert.equal(adjacentPath(paths, "a.txt", -1), "a.txt");
  assert.equal(adjacentPath(paths, "d.txt", 1), "d.txt");
});

test("edge: empty and missing selections are handled safely", () => {
  assert.equal(adjacentPath([], null, 1), null);
  assert.deepEqual(pathsInRange(paths, "missing.txt", "a.txt"), []);
});
