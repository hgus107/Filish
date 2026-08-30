export type SortColumn = "original" | "new";
export type SortDirection = "ascending" | "descending";

type NamedItem = {
  originalName: string;
  proposedName: string;
};

const nameCollator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

export function sortByName<T extends NamedItem>(items: T[], column: SortColumn | null, direction: SortDirection): T[] {
  if (column === null) return items;
  const field = column === "original" ? "originalName" : "proposedName";
  const multiplier = direction === "ascending" ? 1 : -1;
  return [...items].sort((left, right) => nameCollator.compare(left[field], right[field]) * multiplier);
}

export function duplicateTitle(count: number, directory: boolean): string {
  if (directory) return "Folder Already Imported";
  return count === 1 ? "File Already In Queue" : `${count.toLocaleString()} Files Already In Queue`;
}
