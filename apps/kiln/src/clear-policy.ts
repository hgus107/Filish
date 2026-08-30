export type ClearRow = {
  state: string;
  output?: string;
};

export type ClearPolicy = {
  enabled: boolean;
  requiresDiscardConfirmation: boolean;
};

export function clearPolicy(rows: Iterable<ClearRow>, running: boolean): ClearPolicy {
  const queue = [...rows];
  const enabled = queue.length > 0 && !running;
  const requiresDiscardConfirmation =
    enabled && queue.some((row) => Boolean(row.output) && row.state !== "saved");
  return { enabled, requiresDiscardConfirmation };
}
