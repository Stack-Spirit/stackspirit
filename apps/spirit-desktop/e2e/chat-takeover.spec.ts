import { expect, test, type Page } from "@playwright/test";

import {
  E2E_LOCK,
  E2E_TIMEOUT,
  startViteServer,
  stopViteServer,
  type ViteServer,
} from "./support";

// The lifecycle spec uses a local preview bridge wrapper: no real lease, VOWP home,
// token, engine or window close is involved. The wrapper keeps every MockBridge surface
// and adds only the safe writer view shapes the real core returns.
test.describe.configure({ mode: "serial", lock: E2E_LOCK, timeout: E2E_TIMEOUT });
test.use({ contextOptions: { reducedMotion: "reduce" } });

const PORT = 1431;
const BASE_URL = `http://127.0.0.1:${PORT}`;

const BRIDGE_WRAPPER = `
import { MockBridge } from "/@id/@stackspirit/ui";

const otherOwner = {
  kind: "opencode",
  label: "OpenCode · macbook",
  holder: "other-writer",
  node: "macbook",
  mode: "interactive",
  state: "active",
  activity: "idle",
};
const stackspiritOwner = {
  kind: "desktop_ui",
  label: "StackSpirit · macbook",
  holder: "stackspirit",
  node: "macbook",
  mode: "interactive",
  state: "active",
  activity: "idle",
};
const available = { state: "available", can_take_over: true, can_release: false };
const held = { state: "held", can_take_over: false, can_release: true };
const lost = {
  state: "lost",
  can_take_over: false,
  can_release: false,
  reason: "lease_lost",
};

export async function createBridge() {
  const params = new URLSearchParams(window.location.search);
  const requested = params.get("scenario");
  const bridge = new MockBridge(requested || "busy");
  const chat = bridge.chat;
  if (!chat) return bridge;

  const latest = new Map();
  const log = { takeOvers: [], releases: [] };
  window.__chatTakeover = log;

  function decorate(view) {
    latest.set(view.session, view);
    return {
      ...view,
      ownership: otherOwner,
      access: "read_only",
      writer: available,
    };
  }

  const start = chat.start.bind(chat);
  chat.start = async (input) => decorate(await start(input));
  const attach = chat.attach.bind(chat);
  chat.attach = async (session) => decorate(await attach(session));

  chat.takeOver = async (session) => {
    const base = latest.get(session);
    if (!base) throw { code: "chat_not_started", message: "open the session first" };
    log.takeOvers.push(session);
    const next = {
      ...base,
      state: "ready",
      attached: true,
      error: null,
      ownership: stackspiritOwner,
      access: "owner",
      writer: held,
    };
    latest.set(session, next);
    return next;
  };

  chat.releaseWriter = async (session) => {
    log.releases.push(session);
    const base = latest.get(session);
    if (!base) throw { code: "chat_not_started", message: "open the session first" };
    const next = {
      ...base,
      state: "ready",
      attached: true,
      error: null,
      ownership: otherOwner,
      access: "read_only",
      writer: available,
    };
    latest.set(session, next);
    return next;
  };

  log.lose = (session = "global") => {
    const base = latest.get(session);
    if (!base) return;
    const next = {
      ...base,
      state: "ready",
      attached: true,
      error: null,
      ownership: otherOwner,
      access: "read_only",
      writer: lost,
    };
    latest.set(session, next);
    bridge.updateChatView(session, next);
  };

  return bridge;
}
`;

let server: ViteServer | null = null;

test.beforeAll(async () => {
  server = await startViteServer("busy", PORT);
});

test.afterAll(async () => {
  await stopViteServer(server?.process ?? null);
});

async function installBridge(page: Page) {
  await page.route(/^https?:\/\/[^/]+\/src\/bridge\/index\.ts(?:\?.*)?$/, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/javascript",
      headers: { "cache-control": "no-store" },
      body: BRIDGE_WRAPPER,
    });
  });
}

async function openGlobal(page: Page) {
  await page.goto(`${BASE_URL}/?scenario=busy`);
  await page.getByTestId("sidebar-connect").click();
  await expect(
    page.getByRole("heading", { name: "Global orchestrator", level: 1 }),
  ).toBeVisible();
}

async function takeOver(page: Page) {
  await page.getByTestId("writer-take-over").click();
  const dialog = page.getByRole("alertdialog", { name: "Take over this session?" });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Take over" }).click();
  await expect(page.getByTestId("writer-release")).toBeVisible();
}

async function takeoverLog(page: Page) {
  return page.evaluate(
    () =>
      (window as unknown as {
        __chatTakeover: { takeOvers: string[]; releases: string[] };
      }).__chatTakeover,
  );
}

test("switching sessions releases the held writer before the next view opens", async ({
  page,
}) => {
  await installBridge(page);
  await openGlobal(page);
  await takeOver(page);
  expect((await takeoverLog(page)).releases).toEqual([]);

  // Expand the project and open its orchestrator: NextShell remounts ChatView with a new
  // session key, so the global view unmounts.
  await page.locator(".nx-proj__row").filter({ hasText: "vowp" }).first().click();
  await page.getByRole("button", { name: "Orchestrator", exact: true }).click();
  await expect
    .poll(async () => (await takeoverLog(page)).releases, { timeout: 5_000 })
    .toContain("global");
  await expect(page.getByTestId("writer-release")).toHaveCount(0);
});

test("closing the chat view releases the held writer", async ({ page }) => {
  await installBridge(page);
  await openGlobal(page);
  await takeOver(page);

  await page.getByTestId("nav-inbox").click();
  await expect(page.getByRole("heading", { name: "Inbox", level: 1 })).toBeVisible();
  await expect
    .poll(async () => (await takeoverLog(page)).releases, { timeout: 5_000 })
    .toContain("global");
});

test("a lost lease shows the read-only lost view and no release action", async ({
  page,
}) => {
  await installBridge(page);
  await openGlobal(page);
  await takeOver(page);

  await page.evaluate(() => {
    (
      window as unknown as {
        __chatTakeover: { lose: (session?: string) => void };
      }
    ).__chatTakeover.lose();
  });

  await expect(page.getByTestId("writer-reason")).toHaveText(
    "Another writer took this session over. This window is read-only.",
  );
  await expect(page.getByTestId("writer-release")).toHaveCount(0);
  await expect(page.getByTestId("session-ownership")).toContainText("OpenCode");
  const notice = page.getByTestId("readonly-notice");
  await expect(notice).toBeVisible();
  await expect(notice.getByRole("button")).toHaveCount(0);
  await expect(page.getByRole("textbox", { name: "Message" })).toBeDisabled();
  await expect(page.getByTestId("writer-take-over")).toHaveCount(0);
});
