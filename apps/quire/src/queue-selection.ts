export type QueueSelection = {
  selected: Set<string>;
  anchor: string;
  focus: string;
};

export function emptySelection(): QueueSelection {
  return { selected: new Set(), anchor: "", focus: "" };
}

export function selectQueueIndex(
  paths: string[],
  state: QueueSelection,
  index: number,
  extendRange: boolean,
  preserveSelection: boolean,
): QueueSelection {
  const path = paths[index];
  if (!path) return state;
  const selected = new Set(state.selected);
  let anchor = state.anchor;

  if (extendRange) {
    const existingAnchorIndex = paths.indexOf(anchor);
    const anchorIndex = existingAnchorIndex < 0 ? index : existingAnchorIndex;
    if (existingAnchorIndex < 0) anchor = path;
    const start = Math.min(anchorIndex, index);
    const end = Math.max(anchorIndex, index);
    for (let position = start; position <= end; position += 1) selected.add(paths[position]!);
  } else if (preserveSelection) {
    if (selected.has(path)) selected.delete(path);
    else selected.add(path);
    anchor = path;
  } else {
    if (selected.has(path)) selected.clear();
    else {
      selected.clear();
      selected.add(path);
    }
    anchor = path;
  }

  return { selected, anchor, focus: path };
}

export function selectQueueArrow(
  paths: string[],
  state: QueueSelection,
  direction: -1 | 1,
  extendRange: boolean,
  preserveSelection: boolean,
): QueueSelection {
  if (paths.length === 0) return state;
  const currentIndex = paths.indexOf(state.focus);
  let targetIndex: number;
  if (preserveSelection) targetIndex = direction < 0 ? 0 : paths.length - 1;
  else if (currentIndex < 0) targetIndex = direction < 0 ? paths.length - 1 : 0;
  else targetIndex = Math.max(0, Math.min(paths.length - 1, currentIndex + direction));
  return selectQueueIndex(paths, state, targetIndex, extendRange, preserveSelection);
}

export function selectEveryQueuePath(paths: string[]): QueueSelection {
  if (paths.length === 0) return emptySelection();
  return { selected: new Set(paths), anchor: paths[0]!, focus: paths[paths.length - 1]! };
}
