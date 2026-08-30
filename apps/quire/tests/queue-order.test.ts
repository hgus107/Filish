import assert from "node:assert/strict";
import test from "node:test";
import { reorderQueue } from "../src/queue-order.ts";

const queue = ["a", "b", "c", "d"].map((path) => ({ path }));
const paths = (items: typeof queue) => items.map((item) => item.path);

test("drag ordering handles upward and downward placement on both sides", () => {
  assert.deepEqual(paths(reorderQueue(queue, "a", "c", "after")), ["b", "c", "a", "d"]);
  assert.deepEqual(paths(reorderQueue(queue, "d", "b", "before")), ["a", "d", "b", "c"]);
  assert.deepEqual(paths(reorderQueue(queue, "b", "d", "before")), ["a", "c", "b", "d"]);
  assert.deepEqual(paths(reorderQueue(queue, "c", "a", "after")), ["a", "c", "b", "d"]);
});

test("drag ordering respects first, last, same-row, and missing-row boundaries", () => {
  assert.deepEqual(paths(reorderQueue(queue, "d", "a", "before")), ["d", "a", "b", "c"]);
  assert.deepEqual(paths(reorderQueue(queue, "a", "d", "after")), ["b", "c", "d", "a"]);
  assert.equal(reorderQueue(queue, "b", "b", "after"), queue);
  assert.equal(reorderQueue(queue, "missing", "b", "after"), queue);
  assert.equal(reorderQueue(queue, "b", "missing", "after"), queue);
});
