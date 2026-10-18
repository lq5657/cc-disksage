import { describe, expect, it } from "vitest";
import { formatBytes, formatPercent } from "./format";

describe("formatBytes", () => {
  it("handles zero and invalid values", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(-1)).toBe("0 B");
    expect(formatBytes(Number.NaN)).toBe("0 B");
  });

  it("formats common units", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1024)).toBe("1.0 KB");
    expect(formatBytes(10 * 1024 ** 2)).toBe("10 MB");
    expect(formatBytes(71.6 * 1024 ** 3)).toBe("72 GB");
  });
});

describe("formatPercent", () => {
  it("rounds and handles invalid values", () => {
    expect(formatPercent(35.7)).toBe("36%");
    expect(formatPercent(Number.POSITIVE_INFINITY)).toBe("0%");
  });
});
