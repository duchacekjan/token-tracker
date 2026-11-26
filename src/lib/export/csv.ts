export type Cell = string | number | boolean | null;

export interface CsvOptions {
  delimiter: "," | ";";
  decimal: "." | ",";
  bom: boolean;
}

export function formatNumber(n: number, decimal: "." | ","): string {
  if (!Number.isFinite(n)) return "";
  let s: string;
  if (Number.isInteger(n)) {
    s = Math.abs(n) < 1e21 ? String(n) : n.toLocaleString("fullwide", { useGrouping: false });
  } else {
    s = n.toFixed(10);
    if (/^-?0\.0*$/.test(s)) s = "0";
    else s = s.replace(/0+$/, "").replace(/\.$/, "");
    if (s === "-0") s = "0";
  }
  return decimal === "," ? s.replace(".", ",") : s;
}

function cellText(c: Cell, o: CsvOptions, escapeFormula: boolean): string {
  if (c === null) return "";
  if (typeof c === "number") return formatNumber(c, o.decimal);
  if (typeof c === "boolean") return c ? "true" : "false";
  let s = c;
  if (escapeFormula && /^[=+\-@\t\r]/.test(s)) s = "'" + s;
  return s;
}

function quote(s: string, delimiter: string): string {
  if (s.includes(delimiter) || /["\r\n]/.test(s)) return `"${s.replace(/"/g, '""')}"`;
  return s;
}

export function toCsv(header: string[], rows: Cell[][], o: CsvOptions): string {
  const line = (cells: Cell[], esc: boolean) =>
    cells.map((c) => quote(cellText(c, o, esc), o.delimiter)).join(o.delimiter);
  const lines = [line(header, false), ...rows.map((r) => line(r, true))];
  return (o.bom ? "﻿" : "") + lines.join("\r\n") + "\r\n";
}
