export const DIMENSION_MIN = 1024;
export const DIMENSION_MAX = 7680;

export type Dimensions = {
  width: number;
  height: number;
};

export function formatDimensionInput(value: string): string {
  const digits = value.replace(/\D/g, "").slice(0, 8);
  return digits.length >= 4 ? `${digits.slice(0, 4)} * ${digits.slice(4)}` : digits;
}

export function parseDimensions(value: string): Dimensions | null {
  const match = /^(\d{4}) \* (\d{4})$/.exec(value);
  if (!match) return null;
  return { width: Number(match[1]), height: Number(match[2]) };
}

export function dimensionsAreValid(value: string): boolean {
  const dimensions = parseDimensions(value);
  if (!dimensions) return false;
  return (
    dimensions.width >= DIMENSION_MIN &&
    dimensions.width <= DIMENSION_MAX &&
    dimensions.height >= DIMENSION_MIN &&
    dimensions.height <= DIMENSION_MAX
  );
}
