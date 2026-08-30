export function pathsInRange(paths: string[], from: string, to: string): string[] {
  const fromIndex = paths.indexOf(from);
  const toIndex = paths.indexOf(to);
  if (fromIndex < 0 || toIndex < 0) return [];
  const start = Math.min(fromIndex, toIndex);
  const end = Math.max(fromIndex, toIndex);
  return paths.slice(start, end + 1);
}

export function adjacentPath(paths: string[], current: string | null, direction: -1 | 1): string | null {
  if (paths.length === 0) return null;
  const currentIndex = current === null ? -1 : paths.indexOf(current);
  if (currentIndex < 0) return direction > 0 ? paths[0] : paths[paths.length - 1];
  return paths[Math.max(0, Math.min(paths.length - 1, currentIndex + direction))];
}
