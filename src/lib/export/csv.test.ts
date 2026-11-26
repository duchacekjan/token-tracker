import { describe, expect, it } from "vitest";
import { toCsv } from "./csv";

const base = { delimiter: "," as const, decimal: "." as const, bom: false };

describe("toCsv", () => {
  it("quotes delimiter, quotes, CR, LF; empty string stays empty", () => {
    const out = toCsv(["a"], [["x,y"], ['say "hi"'], ["l1\nl2"], ["c\rr"], [""], [null]], base);
    expect(out).toBe('a\r\n"x,y"\r\n"say ""hi"""\r\n"l1\nl2"\r\n"c\rr"\r\n\r\n\r\n');
  });
  it("semicolon delimiter quotes ; not ,", () => {
    const out = toCsv(["a", "b"], [["x;y", "p,q"]], { ...base, delimiter: ";" });
    expect(out).toBe('a;b\r\n"x;y";p,q\r\n');
  });
  it("decimal replacement and no exponent", () => {
    const out = toCsv(["n"], [[1.5], [1e-7], [12], [true], [false], [0.1 + 0.2]], { ...base, delimiter: ";", decimal: "," });
    expect(out).toBe("n\r\n1,5\r\n0,0000001\r\n12\r\ntrue\r\nfalse\r\n0,3\r\n");
    expect(toCsv(["n"], [[1e-7]], base)).toContain("0.0000001");
    expect(toCsv(["n"], [[1e-12]], base)).toBe("n\r\n0\r\n");
  });
  it("BOM", () => {
    expect(toCsv(["a"], [], { ...base, bom: true }).startsWith("﻿a")).toBe(true);
    expect(toCsv(["a"], [], base).startsWith("a")).toBe(true);
  });
  it("formula escape for strings only", () => {
    const out = toCsv(["h"], [["=SUM(A1)"], [-5], ["+1"], ["-x"], ["@a"], ["\tx"], ["ok"]], base);
    const lines = out.split("\r\n");
    expect(lines[1]).toBe("'=SUM(A1)");
    expect(lines[2]).toBe("-5");
    expect(lines[3]).toBe("'+1");
    expect(lines[4]).toBe("'-x");
    expect(lines[5]).toBe("'@a");
    expect(lines[6]).toBe("'\tx");
    expect(lines[7]).toBe("ok");
  });
  it("header not escaped", () => {
    expect(toCsv(["=h"], [], base)).toBe("=h\r\n");
  });
});
