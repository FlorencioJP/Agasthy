// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { Meter } from "./meter";
import { StateView } from "./state-view";

afterEach(cleanup);

describe("StateView", () => {
  it("renders an error as an alert", () => {
    render(<StateView kind="error" title="Scan failed" description="git timed out" />);
    expect(screen.getByRole("alert").textContent).toContain("Scan failed");
    expect(screen.getByText("git timed out")).toBeTruthy();
  });

  it("keeps permission-denied distinct from empty", () => {
    const { container, rerender } = render(<StateView kind="empty" title="No repositories" />);
    expect(container.querySelector("[data-state=empty]")).toBeTruthy();
    rerender(<StateView kind="permission-denied" title="Access denied" />);
    expect(container.querySelector("[data-state=permission-denied]")).toBeTruthy();
    expect(container.querySelector("[data-state=empty]")).toBeNull();
  });
});

describe("Meter", () => {
  it("reports unavailable instead of zero", () => {
    render(<Meter value={null} label="GPU" />);
    expect(screen.getByRole("meter").getAttribute("aria-valuetext")).toBe("unavailable");
    expect(screen.getByRole("meter").getAttribute("aria-valuenow")).toBeNull();
  });

  it("reports a real zero as 0%", () => {
    render(<Meter value={0} label="CPU" />);
    expect(screen.getByRole("meter").getAttribute("aria-valuenow")).toBe("0");
  });
});
