import assert from "node:assert/strict";
import test from "node:test";
import { duplicateTitle, sortByName } from "../src/queue-presentation.ts";

const items = [
  { originalName: "Photo 10.jpg", proposedName: "Trip 02.jpg" },
  { originalName: "Photo 2.jpg", proposedName: "Trip 10.jpg" },
];

test("positive: names sort naturally in ascending and descending directions", () => {
  assert.deepEqual(sortByName(items, "original", "ascending").map((item) => item.originalName), ["Photo 2.jpg", "Photo 10.jpg"]);
  assert.deepEqual(sortByName(items, "new", "descending").map((item) => item.proposedName), ["Trip 10.jpg", "Trip 02.jpg"]);
});

test("edge: view sorting does not mutate rename order", () => {
  const sorted = sortByName(items, "original", "ascending");
  assert.notEqual(sorted, items);
  assert.equal(items[0].originalName, "Photo 10.jpg");
});

test("boundary: duplicate notices use clear singular, plural, and folder labels", () => {
  assert.equal(duplicateTitle(1, false), "File Already In Queue");
  assert.equal(duplicateTitle(2, false), "2 Files Already In Queue");
  assert.equal(duplicateTitle(2_500, false), "2,500 Files Already In Queue");
  assert.equal(duplicateTitle(1, true), "Folder Already Imported");
});
