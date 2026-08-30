export type QueueSelection = {
  selected: string[];
  active: string;
  anchor: string;
};

export function rangeBetween(paths: string[], anchor: string, target: string): string[] {
  const anchorIndex = paths.indexOf(anchor);
  const targetIndex = paths.indexOf(target);
  if (targetIndex < 0) return [];
  if (anchorIndex < 0) return [target];
  const start = Math.min(anchorIndex, targetIndex);
  const end = Math.max(anchorIndex, targetIndex);
  return paths.slice(start, end + 1);
}

export function clickSelection(
  paths: string[],
  _active: string,
  _anchor: string,
  target: string,
  shiftKey: boolean,
  currentSelected: Iterable<string> = [],
): QueueSelection {
  if (!paths.includes(target)) return { selected: [], active: "", anchor: "" };
  if (shiftKey) {
    const selected = new Set([...currentSelected].filter((path) => paths.includes(path)));
    if (selected.has(target)) selected.delete(target);
    else selected.add(target);
    const ordered = paths.filter((path) => selected.has(path));
    const nextActive = selected.has(target) ? target : (ordered[ordered.length - 1] ?? "");
    return { selected: ordered, active: nextActive, anchor: nextActive };
  }
  return { selected: [target], active: target, anchor: target };
}

export function arrowSelection(
  paths: string[],
  active: string,
  anchor: string,
  direction: -1 | 1,
  shiftKey: boolean,
): QueueSelection {
  if (paths.length === 0) return { selected: [], active: "", anchor: "" };
  const currentIndex = paths.indexOf(active);
  const startIndex = currentIndex < 0 ? (direction === 1 ? 0 : paths.length - 1) : currentIndex;
  const nextIndex = Math.max(0, Math.min(startIndex + (currentIndex < 0 ? 0 : direction), paths.length - 1));
  const next = paths[nextIndex];
  if (!shiftKey) return { selected: [next], active: next, anchor: next };
  const fixedAnchor = paths.includes(anchor) ? anchor : paths.includes(active) ? active : next;
  return { selected: rangeBetween(paths, fixedAnchor, next), active: next, anchor: fixedAnchor };
}

export function selectAll(paths: string[], active: string): QueueSelection {
  const nextActive = paths.includes(active) ? active : (paths[0] ?? "");
  return { selected: [...paths], active: nextActive, anchor: nextActive };
}

export function isSelectAllShortcut(key: string, metaKey: boolean, ctrlKey: boolean, isMac: boolean): boolean {
  if (key.toLowerCase() !== "a") return false;
  return isMac ? metaKey : ctrlKey;
}

export function activeAfterRemovals(paths: string[], active: string, removed: ReadonlySet<string>): string {
  if (!removed.has(active)) return paths.includes(active) ? active : "";
  const activeIndex = paths.indexOf(active);
  if (activeIndex < 0) return "";
  for (let index = activeIndex + 1; index < paths.length; index += 1) {
    if (!removed.has(paths[index])) return paths[index];
  }
  for (let index = activeIndex - 1; index >= 0; index -= 1) {
    if (!removed.has(paths[index])) return paths[index];
  }
  return "";
}

export function keyboardDeleteAllowed(
  key: string,
  selectedCount: number,
  running: boolean,
  popupOpen: boolean,
  editingText: boolean,
): boolean {
  return (
    (key === "Delete" || key === "Backspace") &&
    selectedCount > 0 &&
    !running &&
    !popupOpen &&
    !editingText
  );
}
