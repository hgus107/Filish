export function savedCountLabel(count: number): string {
  return `${Math.max(0, Math.trunc(count))} Saved`;
}
