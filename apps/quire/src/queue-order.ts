export type DropSide = "before" | "after";

export function reorderQueue<T extends { path: string }>(
  items: T[],
  sourcePath: string,
  targetPath: string,
  side: DropSide,
): T[] {
  if (sourcePath === targetPath) return items;
  const sourceIndex = items.findIndex((item) => item.path === sourcePath);
  const targetIndex = items.findIndex((item) => item.path === targetPath);
  if (sourceIndex < 0 || targetIndex < 0) return items;

  const reordered = [...items];
  const [source] = reordered.splice(sourceIndex, 1);
  const adjustedTarget = reordered.findIndex((item) => item.path === targetPath);
  const insertionIndex = adjustedTarget + (side === "after" ? 1 : 0);
  reordered.splice(insertionIndex, 0, source!);
  return reordered;
}
