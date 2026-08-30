export function renameButtonLabel(changed: number, busy: boolean): string {
  void changed;
  return busy ? "Saving…" : "Save As";
}
