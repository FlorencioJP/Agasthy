import { expect, test } from "@playwright/test";
import { installFixtureIpc } from "./fixtures/ipc";

test("outside the desktop shell, no machine data is shown", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Open AGASTHY from the desktop app")).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Main" })).toHaveCount(0);
});

test.describe("with the fixture backend", () => {
  test.beforeEach(async ({ page }) => {
    await installFixtureIpc(page);
  });

  test("overview distinguishes unavailable metrics from zero", async ({ page }) => {
    await page.goto("/");
    const nav = page.getByRole("navigation", { name: "Main" });
    await expect(nav.getByRole("link", { name: "Overview" })).toBeVisible();
    await expect(nav.getByRole("link", { name: "Discovery" })).toBeVisible();
    // Only implemented sections are in the sidebar.
    await expect(nav.getByRole("link", { name: "Machine Graph" })).toBeVisible();
    await expect(nav.getByRole("link", { name: "AI Ecosystem" })).toBeVisible();
    await expect(nav.getByRole("link", { name: "Processes" })).toHaveCount(0);

    await expect(page.getByRole("meter", { name: "CPU" })).toHaveAttribute("aria-valuetext", "unavailable");
    await expect(page.getByRole("meter", { name: "Memory" })).toHaveAttribute("aria-valuenow", "25");
    await expect(page.getByText("1 repository has uncommitted changes")).toBeVisible();
  });

  test("discovery lists repositories and shows evidence", async ({ page }) => {
    await page.goto("/#/discovery?tab=repositories");
    const row = page.getByRole("row", { name: /fixture-widget/ });
    await expect(row).toBeVisible();
    await expect(row).toContainText("2 modified");
    await row.click();
    const details = page.getByRole("complementary", { name: "Details" });
    await expect(details.getByText("Source repository cloned locally")).toBeVisible();
    await expect(details.getByText("`.git` directory present")).toBeVisible();
    await expect(details.getByText("Verified").first()).toBeVisible();
  });

  test("empty tabs say they were not scanned instead of showing zero rows", async ({ page }) => {
    await page.goto("/#/discovery?tab=applications");
    await expect(page.getByText("Not scanned yet")).toBeVisible();
  });

  test("privacy page lists the command allowlist from the backend", async ({ page }) => {
    await page.goto("/#/privacy");
    await expect(page.getByText("AGASTHY makes no internet requests in this build.")).toBeVisible();
    await expect(page.getByRole("list", { name: "Command allowlist" }).getByRole("listitem")).toHaveText(["git"]);
  });

  test("machine graph renders relationships and simulates runtime removal", async ({ page }) => {
    await page.goto("/#/graph");
    const canvas = page.getByLabel("Graph canvas");
    await expect(canvas.getByText("fixture-widget")).toBeVisible();
    await expect(canvas.getByText("fixture-lib")).toBeVisible();
    await expect(page.getByText("3 nodes · 2 relationships")).toBeVisible();
    await canvas.getByText("Node.js").click();
    const impact = page.getByLabel("Impact analysis");
    await expect(impact.getByText("1 would break")).toBeVisible();
    await expect(impact.getByText("Would break", { exact: true })).toBeVisible();
    await expect(impact.getByText("package.json: >=20")).toBeVisible();
  });

  test("AI ecosystem answers questions from discovered evidence", async ({ page }) => {
    await page.goto("/#/ai");
    await expect(page.getByText("1 potential instruction conflict")).toBeVisible();
    await page.getByRole("button", { name: "Which AI coding assistants are installed?" }).click();
    await expect(page.getByRole("row", { name: /Fixture Assistant/ })).toContainText("installed");
    await page.getByRole("tab", { name: "Instructions & config" }).click();
    await page.getByRole("button", { name: /Potential conflicts/ }).click();
    const row = page.getByRole("row", { name: /AGENTS\.md/ });
    await expect(row).toBeVisible();
    await row.click();
    const details = page.getByRole("complementary", { name: "Details" });
    await expect(details.getByText("Shown as data. AGASTHY never follows instructions in these files.", { exact: false })).toBeVisible();
    await expect(details.getByText("Use fixture-pm for installs.", { exact: false })).toBeVisible();
  });
});
