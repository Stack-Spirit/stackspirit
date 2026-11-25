import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";

import {
  E2E_LOCK,
  E2E_TIMEOUT,
  startViteServer,
  stopViteServer,
  type ViteServer,
} from "./support";

// One dev server serves every patched-bridge flow; the query string selects the mock
// scenario and which preview-only bridge methods the wrapper below replaces.
test.describe.configure({ mode: "serial", lock: E2E_LOCK, timeout: E2E_TIMEOUT });
test.use({ contextOptions: { reducedMotion: "reduce" } });

const PORT = 1429;
const BASE_URL = `http://127.0.0.1:${PORT}`;
const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];
const THEMES = ["light", "dark"] as const;

const PASSWORD = "e2e-one-time-password-DO-NOT-LEAK";
const SECRET_VALUE = "e2e-secret-value-1234";

/**
 * Preview `MockBridge` cannot store secrets or install a key without a desktop host,
 * and the e2e job may not change the shipped preview fixture. This wrapper keeps every
 * real bridge surface and replaces only those host-only calls with deterministic
 * metadata, so the Meadow cards render their disabled / not_supported / failed states
 * and their stored-secret list. It is test instrumentation: it never sees a value
 * after the call returns.
 */
const BRIDGE_WRAPPER = `
import { MOCK_SCENARIOS, MockBridge } from "/@id/@stackspirit/ui";

const params = new URLSearchParams(window.location.search);
const requested = params.get("scenario");
const scenario = MOCK_SCENARIOS.includes(requested) ? requested : "fresh";

export async function createBridge() {
  const bridge = new MockBridge(scenario);
  const passwordAuth = params.get("e2e-password-auth");

  if (passwordAuth) {
    const outcome =
      passwordAuth === "not_supported"
        ? { state: "not_supported", reason: "sudo_unavailable" }
        : passwordAuth === "failed"
          ? { state: "failed", detail: "sshd reload failed (e2e)" }
          : { state: "disabled" };
    const hosts = await bridge.hosts.list();
    const profile = hosts[0];
    const key = {
      exists: true,
      fingerprint: "SHA256:e2eAppKeyFingerprint000000000000000000000",
      public_key: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIE2E e2e",
    };
    const connectFails = params.get("e2e-connect-fails") === "1";
    bridge.preview = false;
    bridge.onboarding.keyStatus = async () => ({ ...key, installed_host_ids: hosts.map((host) => host.id) });
    // The setup assistant seeds its secure card only while onboarding is incomplete;
    // the Settings retry card is offered only for a host that already has the key.
    bridge.onboarding.installedHosts = async () =>
      connectFails ? [] : (await bridge.hosts.list()).map((host) => host.id);
    bridge.onboarding.createKey = async () => ({ ...key, installed_host_ids: [] });
    bridge.onboarding.installKey = async (input) => ({
      installed: true,
      fingerprint: input.host_key.sha256,
      password_login_disabled: outcome.state === "disabled",
      password_auth: outcome,
      profile,
    });
    if (connectFails) {
      // A real install saves the profile before it connects. Keeping this connection
      // failure honest leaves the setup transcript (and its receipt) on screen.
      bridge.connection.connect = async () => {
        throw {
          code: "ssh_auth",
          message: "SSH authentication failed after the key was installed.",
          hint: "The profile is saved; retry Connect when the server is reachable.",
          retryable: true,
        };
      };
    }
  }

  if (params.get("e2e-secrets") === "1") {
    let secrets = [
      {
        name: "OPENAI_API_KEY",
        last4: "a1b2",
        created_at: "2026-11-14T12:00:00.000Z",
        updated_at: "2026-11-14T12:00:00.000Z",
      },
      {
        name: "ANTHROPIC_API_KEY",
        last4: "9f3d",
        created_at: "2026-11-14T12:00:00.000Z",
        updated_at: "2026-11-14T12:00:00.000Z",
      },
    ];
    bridge.secrets.list = async () => ({
      secrets: secrets.map((entry) => ({ ...entry })),
      storage: null,
    });
    bridge.secrets.set = async (input) => {
      const now = new Date().toISOString();
      const entry = {
        name: input.name,
        last4: input.value.slice(-4),
        created_at: now,
        updated_at: now,
      };
      secrets = [...secrets.filter((candidate) => candidate.name !== input.name), entry];
      return { name: entry.name, last4: entry.last4 };
    };
    bridge.secrets.remove = async (name) => {
      const existed = secrets.some((entry) => entry.name === name);
      secrets = secrets.filter((entry) => entry.name !== name);
      return { name, removed: existed };
    };
  }

  return bridge;
}
`;

async function installBridgeWrapper(page: Page) {
  // Match only the desktop app's own bridge module, never the linked UI package copies
  // under /@fs/.../packages/spirit-ui/src/**.
  await page.route(/^https?:\/\/[^/]+\/src\/bridge\/index\.ts(?:\?.*)?$/, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/javascript",
      headers: { "cache-control": "no-store" },
      body: BRIDGE_WRAPPER,
    });
  });
}

function watchConsole(page: Page): string[] {
  const lines: string[] = [];
  page.on("console", (message) => lines.push(message.text()));
  page.on("pageerror", (error) => lines.push(error.message));
  return lines;
}

async function expectNoWcagViolations(page: Page) {
  const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
  expect(
    results.violations.map((violation) => ({
      id: violation.id,
      targets: violation.nodes.map((node) => node.target.join(" ")),
    })),
  ).toEqual([]);
}

async function expectReadOnlyLocationChip(page: Page, chip: Locator) {
  await expect(chip).toBeVisible();
  expect(await chip.evaluate((element) => element.tagName)).toBe("SPAN");
  await expect(chip).not.toHaveAttribute("role");
  await expect(chip).not.toHaveAttribute("tabindex");
  // A move/switch control is a VOWP 0.5 gate; Meadow must not offer one yet.
  await expect(
    page.getByRole("button", {
      name: /move (this )?session|switch to (local|server)|run on (local|server)/i,
    }),
  ).toHaveCount(0);
}

let server: ViteServer | null = null;

test.beforeAll(async () => {
  server = await startViteServer("fresh", PORT);
});

test.afterAll(async () => {
  await stopViteServer(server?.process ?? null);
});

const PASSWORD_OUTCOME_TEXT = {
  disabled:
    "Password login is now off. StackSpirit verified a fresh key-only login before reporting this.",
  not_supported:
    "Password login is still on: this server has no usable sudo, so sshd was not changed (sudo_unavailable).",
  failed: "Password login is still on: the change failed (sshd reload failed (e2e)).",
} as const;

test("password-login outcomes render honestly and never expose the password", async ({
  page,
}) => {
  await installBridgeWrapper(page);

  for (const state of ["disabled", "not_supported", "failed"] as const) {
    for (const theme of THEMES) {
      const logs = watchConsole(page);
      await page.goto(
        `${BASE_URL}/?scenario=fresh&theme=${theme}&e2e-password-auth=${state}`,
      );
      await expect(page.getByTestId("next-shell")).toBeVisible();
      await page.getByTestId("settings-trigger").click();
      await page.getByRole("menuitem", { name: "All settings" }).click();

      const card = page.getByTestId("server-security");
      await expect(card).toBeVisible();
      const passwordInput = card.getByLabel("One-time server password");
      await passwordInput.fill(PASSWORD);
      await card.getByRole("button", { name: "Turn off password login" }).click();

      const outcome = page.getByTestId("password-auth-outcome");
      await expect(outcome).toBeVisible();
      await expect(outcome).toHaveText(PASSWORD_OUTCOME_TEXT[state]);
      await expect(passwordInput).toHaveValue("");
      await expect(page.locator("body")).not.toContainText(PASSWORD);
      expect(logs.filter((line) => line.includes(PASSWORD))).toEqual([]);
      await expectNoWcagViolations(page);
    }
  }
});

const ONBOARDING_PASSWORD_TEXT = {
  disabled:
    "Password login is now turned off; the StackSpirit key has a verified key-only login.",
  not_supported:
    "Password login is still on: this server has no usable sudo, so sshd was not changed (sudo_unavailable). The new key works.",
  failed:
    "Password login is still on: the change failed (sshd reload failed (e2e)). The new key works; you can retry from Settings.",
} as const;

test("setup assistant installs a key and reports password login without leaking the password", async ({
  page,
}) => {
  await installBridgeWrapper(page);

  for (const state of ["disabled", "not_supported", "failed"] as const) {
    for (const theme of THEMES) {
      const logs = watchConsole(page);
      await page.goto(
        `${BASE_URL}/?scenario=fresh&theme=${theme}&e2e-password-auth=${state}&e2e-connect-fails=1`,
      );
      await expect(page.getByTestId("next-shell")).toBeVisible();
      await page
        .getByRole("button", { name: "Chat with the setup assistant" })
        .click();

      const chip = page.getByTestId("session-location");
      await expect(chip).toContainText("Local · this Mac");
      await expectReadOnlyLocationChip(page, chip);

      // A saved server never sees the new-server password card until the owner asks
      // for another server.
      const savedCard = page.getByTestId("saved-server-card");
      await expect(savedCard).toContainText("staging-vps");
      await expect(page.getByRole("form", { name: "Secure server details" })).toHaveCount(0);
      await savedCard.getByRole("button", { name: "Add another server", exact: true }).click();

      const form = page.getByRole("form", { name: "Secure server details" });
      await expect(form).toBeVisible();
      await form.getByLabel("Server address").fill("203.0.113.10");
      await form.getByLabel("One-time password from your provider").fill(PASSWORD);
      await form.getByRole("button", { name: "Connect securely" }).click();
      await expect(
        form.getByText("Server details received for 203.0.113.10"),
      ).toBeVisible();

      await form.getByRole("checkbox", { name: /I verified this fingerprint/ }).check();
      await form.getByRole("button", { name: "Install key" }).click();
      await expect(page.getByText("Key installed for 203.0.113.10")).toBeVisible();
      await expect(page.getByText(ONBOARDING_PASSWORD_TEXT[state])).toBeVisible();
      await expect(page.locator("body")).not.toContainText(PASSWORD);
      expect(logs.filter((line) => line.includes(PASSWORD))).toEqual([]);
      expect(server?.log() ?? "").not.toContain(PASSWORD);
      await expectNoWcagViolations(page);
    }
  }
});

test("server secrets warn, list masked keys and revoke without exposing values", async ({
  page,
}) => {
  await installBridgeWrapper(page);

  for (const theme of THEMES) {
    const logs = watchConsole(page);
    await page.goto(`${BASE_URL}/?scenario=fresh&theme=${theme}&e2e-secrets=1`);
    await expect(page.getByTestId("next-shell")).toBeVisible();
    await page.getByTestId("sidebar-connect").click();
    await expect(page.getByTestId("health-card")).toBeVisible();

    await page.getByTestId("settings-trigger").click();
    await page.getByRole("menuitem", { name: "All settings" }).click();
    await page.getByRole("button", { name: "Show advanced settings" }).click();

    const card = page.getByTestId("server-secrets");
    await expect(card).toBeVisible();
    await expect(card.getByTestId("secret-first-warning")).toBeVisible();
    await expect(card.getByText("Before your first server secret")).toBeVisible();

    const list = card.getByRole("list", { name: "Stored server secrets" });
    await expect(list.getByTestId("secret-row-OPENAI_API_KEY")).toContainText(
      "•••• a1b2",
    );
    await expect(list.getByTestId("secret-row-ANTHROPIC_API_KEY")).toContainText(
      "•••• 9f3d",
    );

    await card
      .getByRole("checkbox", { name: "I understand this stores the key on the server" })
      .check();
    await card.getByLabel("Secret name").fill("GITHUB_TOKEN");
    await card.getByLabel("Secret value").fill(SECRET_VALUE);
    await card.getByRole("button", { name: "Save" }).click();
    await expect(card.getByTestId("secret-stored")).toContainText("ends in …1234");
    await expect(list.getByTestId("secret-row-GITHUB_TOKEN")).toContainText(
      "•••• 1234",
    );
    await expect(page.locator("body")).not.toContainText(SECRET_VALUE);

    const revoked = list.getByTestId("secret-row-ANTHROPIC_API_KEY");
    await revoked.getByRole("button", { name: "Revoke" }).click();
    const confirm = card.getByRole("group", {
      name: "Confirm revoke ANTHROPIC_API_KEY",
    });
    await confirm.getByRole("button", { name: "Revoke" }).click();
    await expect(
      list.getByTestId("secret-row-ANTHROPIC_API_KEY"),
    ).toHaveCount(0);
    await expect(card.getByTestId("secret-notice")).toContainText(
      "Removed ANTHROPIC_API_KEY.",
    );

    expect(logs.filter((line) => line.includes(SECRET_VALUE))).toEqual([]);
    expect(server?.log() ?? "").not.toContain(SECRET_VALUE);
    await expectNoWcagViolations(page);
  }
});

test("the setup session chip reads Local and is read-only", async ({ page }) => {
  for (const theme of THEMES) {
    await page.goto(`${BASE_URL}/?scenario=empty&theme=${theme}`);
    await page.getByRole("button", { name: /Set up with AI/ }).click();
    const chip = page.getByTestId("session-location");
    await expect(chip).toContainText("Local · this Mac");
    await expectReadOnlyLocationChip(page, chip);
    await expect(page.getByRole("textbox", { name: "Message" })).toBeEnabled();
    await expectNoWcagViolations(page);
  }
});

test("a connected session chip reads Local and is read-only", async ({ page }) => {
  for (const theme of THEMES) {
    await page.goto(`${BASE_URL}/?scenario=busy&theme=${theme}`);
    await page.getByTestId("sidebar-connect").click();
    await expect(page.getByTestId("health-card")).toBeVisible();
    const chip = page.getByTestId("session-location");
    await expect(chip).toContainText("Local · this Mac");
    await expectReadOnlyLocationChip(page, chip);
    await expectNoWcagViolations(page);
  }
});

test("a saved offline server chats without connecting and meets WCAG 2.2 AA", async ({
  page,
}) => {
  for (const theme of THEMES) {
    await page.goto(`${BASE_URL}/?scenario=fresh&theme=${theme}`);
    await expect(page.getByTestId("next-shell")).toBeVisible();
    await expect(page.getByTestId("connect-prompt-connect")).toBeVisible();

    await page.getByRole("button", { name: "Chat with the setup assistant" }).click();
    const chip = page.getByTestId("session-location");
    await expect(chip).toContainText("Local · this Mac");
    await expectReadOnlyLocationChip(page, chip);

    const card = page.getByTestId("saved-server-card");
    await expect(card).toContainText("staging-vps");
    await expect(card).toContainText("203.0.113.10");
    await expect(card).toContainText("owner");
    await expect(card).toContainText("22");
    await expect(page.getByRole("form", { name: "Secure server details" })).toHaveCount(0);
    await expect(page.getByTestId("sidebar-connect")).toBeVisible();
    await expect(page.getByRole("textbox", { name: "Message" })).toBeEnabled();

    await page.getByRole("textbox", { name: "Message" }).fill("What can I do with my server?");
    await page.getByRole("button", { name: "Send" }).click();
    await expect(page.getByText(/Preview reply\./)).toBeVisible();

    await expectNoWcagViolations(page);
  }
});

test("the server chat gate keeps the composer disabled and invents no reply", async ({
  page,
}) => {
  for (const theme of THEMES) {
    await page.goto(`${BASE_URL}/?scenario=chat-server-gated&theme=${theme}`);
    await page.getByTestId("sidebar-connect").click();
    await expect(page.getByTestId("health-card")).toBeVisible();

    const gate = page.getByTestId("chat-error");
    await expect(gate).toBeVisible();
    await expect(gate).toHaveAttribute("data-gate", "true");
    await expect(gate).toContainText(
      "Server chat needs a newer StackSpirit on the host.",
    );
    await expect(page.getByTestId("session-location")).toContainText("Not connected");
    await expect(page.getByRole("textbox", { name: "Message" })).toBeDisabled();
    await expect(page.getByText(/Preview reply\./)).toHaveCount(0);
    await expectNoWcagViolations(page);
  }
});
