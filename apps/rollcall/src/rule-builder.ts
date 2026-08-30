export type RenameFunction = "add-text" | "replace-text" | "change-case" | "add-numbers" | "add-date";

export type RenameRules = {
  template: string;
  find: string;
  replace: string;
  caseStyle: string;
  counterStart: number;
  counterPadding: number;
  dateSource: string;
  dateFormat: string;
  customDate: string;
};

export type RuleInputs = {
  prefix: string;
  suffix: string;
  find: string;
  replace: string;
  caseStyle: string;
  counterStart: number;
  counterPadding: number;
  numberPosition: string;
  datePosition: string;
  dateSource: string;
  dateFormat: string;
  customDate: string;
};

function clampedInteger(value: number, fallback: number, minimum: number, maximum: number): number {
  if (!Number.isFinite(value)) return fallback;
  return Math.max(minimum, Math.min(Math.trunc(value), maximum));
}

function validIsoDate(value: string): boolean {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/u.exec(value);
  if (!match) return false;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const date = new Date(Date.UTC(year, month - 1, day));
  return date.getUTCFullYear() === year && date.getUTCMonth() === month - 1 && date.getUTCDate() === day;
}

export function buildRenameRules(selectedFunction: string, inputs: RuleInputs): RenameRules {
  const counterStart = clampedInteger(inputs.counterStart, 1, 0, 999_999);
  const counterPadding = clampedInteger(inputs.counterPadding, 3, 1, 3);

  switch (selectedFunction as RenameFunction) {
    case "add-text":
      return {
        template: `${inputs.prefix}{name}${inputs.suffix}`,
        find: "",
        replace: "",
        caseStyle: "keep",
        counterStart,
        counterPadding,
        dateSource: "today",
        dateFormat: "yyyy-mm-dd",
        customDate: "",
      };
    case "replace-text":
      return {
        template: "{name}",
        find: inputs.find,
        replace: inputs.replace,
        caseStyle: "keep",
        counterStart,
        counterPadding,
        dateSource: "today",
        dateFormat: "yyyy-mm-dd",
        customDate: "",
      };
    case "change-case":
      return {
        template: "{name}",
        find: "",
        replace: "",
        caseStyle: inputs.caseStyle,
        counterStart,
        counterPadding,
        dateSource: "today",
        dateFormat: "yyyy-mm-dd",
        customDate: "",
      };
    case "add-numbers":
      if (!["beginning", "end"].includes(inputs.numberPosition)) throw new Error("Select A Number Position.");
      return {
        template: inputs.numberPosition === "beginning" ? "{n}-{name}" : "{name}-{n}",
        find: "",
        replace: "",
        caseStyle: "keep",
        counterStart,
        counterPadding,
        dateSource: "today",
        dateFormat: "yyyy-mm-dd",
        customDate: "",
      };
    case "add-date":
      if (!["beginning", "end"].includes(inputs.datePosition)) throw new Error("Select A Date Position.");
      if (!["today", "modified", "custom"].includes(inputs.dateSource)) throw new Error("Select A Date Source.");
      if (inputs.dateSource === "custom" && !validIsoDate(inputs.customDate)) {
        throw new Error("Select A Custom Date.");
      }
      if (!["yyyy-mm-dd", "mm-dd-yyyy", "dd-mm-yyyy"].includes(inputs.dateFormat)) {
        throw new Error("Select A Date Format.");
      }
      return {
        template: inputs.datePosition === "beginning" ? "{date}-{name}" : "{name}-{date}",
        find: "",
        replace: "",
        caseStyle: "keep",
        counterStart,
        counterPadding,
        dateSource: inputs.dateSource,
        dateFormat: inputs.dateFormat,
        customDate: inputs.dateSource === "custom" ? inputs.customDate : "",
      };
    default:
      throw new Error("Choose A Supported Rename Function.");
  }
}
