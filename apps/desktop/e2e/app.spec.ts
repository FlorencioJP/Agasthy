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
    await expect(nav.getByRole("link", { name: "Processes" })).toBeVisible();
    await expect(nav.getByRole("link", { name: "Security" })).toBeVisible();
    await expect(nav.getByRole("link", { name: "AI Operator" })).toBeVisible();

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
    await expect(page.getByText("Otherwise, AGASTHY makes no internet requests.")).toBeVisible();
    await expect(page.getByText("Optional: an AI model for the Operator (off by default).")).toBeVisible();
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

  test("cleanup goes through a dry-run review before anything changes", async ({ page }) => {
    await page.goto("/#/storage");
    const tracked = page.getByRole("checkbox", { name: /dist/ });
    await expect(tracked).toBeDisabled();
    await expect(page.getByText("Not offered: contains files tracked by Git")).toBeVisible();
    await page.getByRole("checkbox", { name: /node_modules/ }).check();
    await page.getByRole("button", { name: /Review 1/ }).click();
    const dialog = page.getByRole("dialog", { name: "Review action" });
    await expect(dialog.getByText("Dry run only so far. Nothing has changed.", { exact: false })).toBeVisible();
    await expect(dialog.getByText("reversible", { exact: true })).toBeVisible();
    await dialog.getByRole("button", { name: "Confirm" }).click();
    await expect(dialog.getByText("Moved 1 item to quarantine").last()).toBeVisible();
    await expect(dialog.getByText("Verification:", { exact: false })).toBeVisible();
  });

  test("protected processes cannot be stopped", async ({ page }) => {
    await page.goto("/#/processes");
    await page.getByRole("button", { name: /^All/ }).click();
    await page.getByRole("row", { name: /fixture-system/ }).click();
    const details = page.getByRole("complementary", { name: "Process details" });
    await expect(details.getByText("AGASTHY will not stop this process", { exact: false })).toBeVisible();
    await expect(details.getByRole("button", { name: /Stop/ })).toHaveCount(0);
    await page.getByRole("row", { name: /fixture-server/ }).click();
    await expect(details.getByRole("button", { name: /Force quit/ })).toBeVisible();
    await expect(details.getByText("fixture-widget (via working directory)")).toBeVisible();
  });

  test("impact analysis is a simulation with a plan AGASTHY does not run", async ({ page }) => {
    await page.goto("/#/impact?id=package:homebrew:fixture-lib");
    await expect(page.getByText("high risk")).toBeVisible();
    await expect(page.getByText("Other installed software depends on fixture-lib.")).toBeVisible();
    await expect(page.getByText("Suggested plan (AGASTHY does not run these)")).toBeVisible();
    await expect(page.getByText("fixture unknown")).toBeVisible();
  });

  test("online advisory checks are off by default and disclose what is sent", async ({ page }) => {
    await page.goto("/#/security");
    const toggle = page.getByRole("checkbox", { name: "Check advisories online with OSV.dev" });
    await expect(toggle).not.toBeChecked();
    await page.getByRole("button", { name: "Show exactly what would be sent" }).click();
    await expect(page.getByText("npm  fixture-pkg@1.0.0")).toBeVisible();
  });

  test("operator answers locally and routes proposals through the dry-run review", async ({ page }) => {
    await page.goto("/#/operator");
    await expect(page.getByText("Built-in Operator.", { exact: false })).toBeVisible();
    await expect(page.getByRole("radio", { name: "Model" })).toBeDisabled();
    await page.getByRole("button", { name: "Free 10 GB safely" }).click();
    await expect(page.getByText("nothing sent off this machine")).toBeVisible();
    await page.getByRole("button", { name: "1 lookup" }).click();
    await expect(page.getByText("Disk analysis")).toBeVisible();
    await page.getByRole("button", { name: "Review" }).click();
    const dialog = page.getByRole("dialog", { name: "Review action" });
    await expect(dialog.getByText("Dry run only so far. Nothing has changed.", { exact: false })).toBeVisible();
  });

  test("cloud models need an explicit opt-in and keys go to the credential store", async ({ page }) => {
    await page.goto("/#/settings");
    await expect(page.getByRole("checkbox", { name: /Allow sending machine data to a cloud model/ })).not.toBeChecked();
    await page.getByLabel("Provider").selectOption("anthropic");
    await expect(page.getByText("in the cloud")).toBeVisible();
    await expect(page.getByText("Stored in the operating system", { exact: false })).toBeVisible();
    await page.getByLabel("Provider").selectOption("ollama");
    await expect(page.getByText("on this machine")).toBeVisible();
  });

  test("agents page shows sessions, what they left running and their worktrees", async ({ page }) => {
    await page.goto("/#/agents");
    const card = page.getByRole("article", { name: "Claude Code session" });
    await expect(card.getByText("fixture-widget")).toBeVisible();
    await expect(card.getByText("1 MCP")).toBeVisible();
    await expect(card.getByText(":5173")).toBeVisible();
    await card.getByRole("button", { name: "2 processes started" }).click();
    await expect(card.getByText("fixture vite")).toBeVisible();
    const left = page.getByRole("region", { name: "Left running" });
    await expect(left.getByText("Started by Codex CLI in /fixture/code/other", { exact: false })).toBeVisible();
    await expect(page.getByText("3 uncommitted")).toBeVisible();
    await expect(card.getByText("2.1M tokens", { exact: false })).toBeVisible();
    await expect(card.getByText("≈$1.40", { exact: false })).toBeVisible();
    await left.getByRole("button", { name: "Review stopping 1" }).click();
    await expect(page.getByRole("dialog", { name: "Review action" })).toBeVisible();
  });

  test("token usage has its own page with totals, sessions and sources", async ({ page }) => {
    await page.goto("/#/usage");
    await expect(page.getByRole("navigation").getByRole("link", { name: "Token Usage" })).toBeVisible();
    await expect(page.getByText("counters only, no message text", { exact: false })).toBeVisible();
    await expect(page.getByRole("radio", { name: "30 days" })).toHaveAttribute("aria-checked", "true");
    await expect(page.getByText("Sessions (1)")).toBeVisible();
    await expect(page.getByRole("row", { name: /fixture-widget.*claude-opus-5-5.*\$1\.40/ })).toBeVisible();
    await expect(page.getByRole("img", { name: /Daily token usage/ })).toBeVisible();
  });

  test("snapshots list what is missing as commands, never values", async ({ page }) => {
    await page.goto("/#/snapshots");
    await page.getByRole("button", { name: "What's missing?" }).click();
    await expect(page.getByText("npm install --global fixture-pkg@1.0.0")).toBeVisible();
    await expect(page.getByText("Only the name was recorded; set the value yourself.")).toBeVisible();
    await expect(page.getByText("AGASTHY does not run these commands", { exact: false })).toBeVisible();
  });
});
