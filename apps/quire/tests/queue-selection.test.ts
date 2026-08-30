import assert from "node:assert/strict";
import test from "node:test";
import {
  emptySelection,
  selectEveryQueuePath,
  selectQueueArrow,
  selectQueueIndex,
} from "../src/queue-selection.ts";

const paths = ["a", "b", "c", "d", "e"];

test("click selects one row and Shift+Click adds an adjacent range", () => {
  const clicked = selectQueueIndex(paths, emptySelection(), 1, false, false);
  assert.deepEqual([...clicked.selected], ["b"]);
  const ranged = selectQueueIndex(paths, clicked, 3, true, false);
  assert.deepEqual([...ranged.selected], ["b", "c", "d"]);
});

test("plain click selects one row, replaces it with another, and deselects the same row", () => {
  let state = selectQueueIndex(paths, emptySelection(), 0, false, false);
  state = selectQueueIndex(paths, state, 2, false, false);
  assert.deepEqual([...state.selected], ["c"]);
  state = selectQueueIndex(paths, state, 2, false, false);
  assert.deepEqual([...state.selected], []);
});

test("Command+Click toggles non-adjacent rows without clearing earlier choices", () => {
  let state = selectQueueIndex(paths, emptySelection(), 0, false, false);
  state = selectQueueIndex(paths, state, 2, false, true);
  state = selectQueueIndex(paths, state, 4, false, true);
  assert.deepEqual([...state.selected], ["a", "c", "e"]);
  state = selectQueueIndex(paths, state, 2, false, true);
  assert.deepEqual([...state.selected], ["a", "e"]);
});

test("Shift+Arrow extends adjacent selection and respects boundaries", () => {
  let state = selectQueueIndex(paths, emptySelection(), 1, false, false);
  state = selectQueueArrow(paths, state, 1, true, false);
  state = selectQueueArrow(paths, state, 1, true, false);
  assert.deepEqual([...state.selected], ["b", "c", "d"]);
  state = selectQueueArrow(paths, state, 1, true, false);
  state = selectQueueArrow(paths, state, 1, true, false);
  assert.equal(state.focus, "e");
  assert.equal(state.selected.size, 4);
});

test("Command+Arrow preserves a non-adjacent selection at queue boundaries", () => {
  let state = selectQueueIndex(paths, emptySelection(), 2, false, false);
  state = selectQueueArrow(paths, state, -1, false, true);
  assert.deepEqual([...state.selected], ["c", "a"]);
  state = selectQueueArrow(paths, state, 1, false, true);
  assert.deepEqual([...state.selected], ["c", "a", "e"]);
});

test("Command+A selects every row including one-row and empty boundaries", () => {
  assert.deepEqual([...selectEveryQueuePath(paths).selected], paths);
  assert.deepEqual([...selectEveryQueuePath(["only"]).selected], ["only"]);
  assert.equal(selectEveryQueuePath([]).selected.size, 0);
});

test("selection safely handles empty queues, invalid indexes, missing focus, and both boundaries", () => {
  const empty = emptySelection();
  assert.equal(selectQueueIndex(paths, empty, -1, false, false), empty);
  assert.equal(selectQueueIndex(paths, empty, paths.length, false, false), empty);
  assert.equal(selectQueueArrow([], empty, 1, false, false), empty);

  const downFromNothing = selectQueueArrow(paths, empty, 1, false, false);
  assert.equal(downFromNothing.focus, "a");
  const upFromNothing = selectQueueArrow(paths, empty, -1, false, false);
  assert.equal(upFromNothing.focus, "e");

  const atTop = selectQueueArrow(paths, downFromNothing, -1, false, false);
  assert.equal(atTop.focus, "a");
  const atBottom = selectQueueArrow(paths, upFromNothing, 1, false, false);
  assert.equal(atBottom.focus, "e");

  const missingAnchor = { selected: new Set<string>(), anchor: "missing", focus: "missing" };
  const extended = selectQueueIndex(paths, missingAnchor, 2, true, false);
  assert.deepEqual([...extended.selected], ["c"]);
});
