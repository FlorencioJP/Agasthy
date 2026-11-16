import { describe, expect, it } from "vitest";
import { formatBytes, formatRelative, shortenHome, unavailableReason, value } from "./format";

describe("format", () => {
  it("formats bytes in binary units", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(16 * 1024 ** 3)).toBe("16.0 GB");
    expect(formatBytes(-1)).toBe("—");
  });

  it("distinguishes zero from unavailable", () => {
    expect(value({ status: "available", value: 0 })).toBe(0);
    expect(value({ status: "unavailable", reason: "x" })).toBeNull();
    expect(unavailableReason({ status: "unsupported", platform: "windows" })).toContain("windows");
  });

  it("formats relative times", () => {
    const now = 1_700_000_000_000;
    expect(formatRelative(now - 10_000, now)).toBe("just now");
    expect(formatRelative(now - 3 * 3600_000, now)).toMatch(/3 hours ago/);
  });

  it("shortens home paths", () => {
    expect(shortenHome("/Users/dev/code/x", "/Users/dev")).toBe("~/code/x");
    expect(shortenHome("/opt/homebrew/bin/jq", "/Users/dev")).toBe("/opt/homebrew/bin/jq");
  });
});
