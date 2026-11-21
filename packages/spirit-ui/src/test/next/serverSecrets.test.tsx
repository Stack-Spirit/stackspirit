import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { SpiritBridge } from "../../bridge/SpiritBridge";
import type {
  ConnectionSnapshot,
  HostProfile,
  SecretEntryView,
  SecretListView,
  SecretRemoveOutcome,
  SecretSetInput,
  SecretSetOutcome,
} from "../../bridge/wire";
import { formatDateTime } from "../../domain/format";
import { SecretStore } from "../../next/SecretStore";
import { SettingsView } from "../../next/SettingsView";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

const AT = "2026-11-18T11:00:00.000Z";
const UPDATED = "2026-11-18T12:30:00.000Z";
const HOST_ID = "host-server-secrets";
const SECRET_VALUE = ["unit", "-test", "-secret", "-value"].join("");

function host(): HostProfile {
  return {
    id: HOST_ID,
    label: "production-vps",
    ssh: { user: "root", host: "203.0.113.70", port: 22 },
    identity_file: "/app-data/ssh-keys/stackspirit-secrets",
    vowp_home: null,
    default_project: null,
    host_key: {
      algorithm: "ssh-ed25519",
      sha256: "SHA256:serverSecretsHostKey000000000000000000",
      pinned_at: AT,
    },
    created_at: AT,
    updated_at: AT,
    last_connected_at: AT,
  };
}

function connected(): ConnectionSnapshot {
  return {
    state: "connected",
    host_id: HOST_ID,
    since: AT,
    latency: null,
    last_ok_at: AT,
    error: null,
  };
}

function disconnected(): ConnectionSnapshot {
  return {
    state: "disconnected",
    host_id: null,
    since: null,
    latency: null,
    last_ok_at: null,
    error: null,
  };
}

function entry(name: string, last4: string): SecretEntryView {
  return {
    name,
    last4,
    created_at: AT,
    updated_at: UPDATED,
  };
}

function listOf(
  secrets: SecretEntryView[],
  storage: string | null = null,
): SecretListView {
  return { secrets, storage };
}

const DEFAULT_ROWS = [
  entry("OPENAI_API_KEY", "9f2a"),
  entry("ANTHROPIC_API_KEY", "4b7c"),
];

interface FakeOptions {
  list?: () => Promise<SecretListView>;
  set?: (input: SecretSetInput) => Promise<SecretSetOutcome>;
  remove?: (name: string) => Promise<SecretRemoveOutcome>;
  withSecrets?: boolean;
  connection?: ConnectionSnapshot;
}

/**
 * Only the parts SettingsView and SecretStore call are real. The cast keeps the test
 * focused on the vault card instead of re-stubbing the whole bridge.
 */
function fakeBridge(options: FakeOptions = {}) {
  const list = vi.fn(options.list ?? (async () => listOf(DEFAULT_ROWS)));
  const set = vi.fn(
    options.set ??
      (async (input: SecretSetInput) => ({
        name: input.name,
        last4: input.value.slice(-4),
      })),
  );
  const remove = vi.fn(
    options.remove ??
      (async (name: string) => ({ name, removed: true })),
  );
  const secrets =
    options.withSecrets === false ? undefined : { list, set, remove };
  const bridge = {
    kind: "tauri",
    preview: false,
    secrets,
    hosts: { list: async () => [host()] },
    engine: { keyStatus: async () => [] },
    connection: { status: async () => options.connection ?? connected() },
    fleet: { get: async () => null, rawDocument: async () => null },
    gates: {
      get: async () => {
        throw new Error("gate snapshots are not used by this test");
      },
      probe: async () => {
        throw new Error("gate probes are not used by this test");
      },
    },
  } as unknown as SpiritBridge;
  return { bridge, list, set, remove, secrets };
}

function install(bridge: SpiritBridge, connection: ConnectionSnapshot = connected()) {
  resetFleetStore();
  useFleetStore.setState({
    bridge,
    preview: false,
    status: "ready",
    connection,
    hosts: [host()],
    hostsLoaded: true,
    selectedHostId: HOST_ID,
  });
}

function renderStore(options: FakeOptions = {}) {
  const fake = fakeBridge(options);
  install(fake.bridge, options.connection ?? connected());
  return { ...fake, ...render(<SecretStore />) };
}

afterEach(() => {
  resetFleetStore();
});

describe("Settings server secrets", () => {
  it("lists stored names, last4 and update time without a value", async () => {
    const { list } = renderStore();

    expect(
      await screen.findByRole("list", { name: "Stored server secrets" }),
    ).toBeInTheDocument();
    expect(screen.getByText("OPENAI_API_KEY")).toBeInTheDocument();
    expect(screen.getByText("ANTHROPIC_API_KEY")).toBeInTheDocument();
    expect(screen.getByText("•••• 9f2a")).toBeInTheDocument();
    expect(screen.getByText("•••• 4b7c")).toBeInTheDocument();

    const row = screen.getByTestId("secret-row-OPENAI_API_KEY");
    expect(
      within(row).getByText(`Updated ${formatDateTime(UPDATED)}`),
    ).toBeInTheDocument();
    expect(list).toHaveBeenCalledTimes(1);
    expect(document.body.textContent).not.toContain(SECRET_VALUE);
  });

  it("never calls the store without a connection", async () => {
    const { list, set, remove } = renderStore({ connection: disconnected() });

    expect(
      screen.getByText("Connect a server to manage its secrets"),
    ).toBeInTheDocument();
    expect(list).not.toHaveBeenCalled();
    expect(set).not.toHaveBeenCalled();
    expect(remove).not.toHaveBeenCalled();
    expect(screen.getByLabelText("Secret name")).toBeDisabled();
    expect(screen.getByLabelText("Secret value")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("requires the first-secret acknowledgement before enabling Save", async () => {
    const user = userEvent.setup();
    const { set } = renderStore();
    await screen.findByRole("list", { name: "Stored server secrets" });

    const warning = screen.getByTestId("secret-first-warning");
    expect(warning).toHaveTextContent("Before your first server secret");
    expect(warning).toHaveTextContent("on production-vps in VOWP");
    expect(warning).toHaveTextContent(
      "The assistant only sees secret names and metadata",
    );

    await user.type(screen.getByLabelText("Secret name"), "NEW_PROVIDER_KEY");
    await user.type(screen.getByLabelText("Secret value"), SECRET_VALUE);
    const save = screen.getByRole("button", { name: "Save" });
    expect(save).toBeDisabled();

    await user.click(save);
    expect(set).not.toHaveBeenCalled();

    await user.click(
      screen.getByLabelText("I understand this stores the key on the server"),
    );
    expect(save).toBeEnabled();
    await user.click(save);

    await waitFor(() => expect(set).toHaveBeenCalledTimes(1));
    expect(set).toHaveBeenCalledWith({
      name: "NEW_PROVIDER_KEY",
      value: SECRET_VALUE,
    });
    await waitFor(() =>
      expect(screen.queryByTestId("secret-first-warning")).not.toBeInTheDocument(),
    );
  });

  it("keeps the first-save acknowledgement across a Settings remount", async () => {
    const user = userEvent.setup();
    const { bridge, unmount } = renderStore();
    await screen.findByRole("list", { name: "Stored server secrets" });

    await user.type(screen.getByLabelText("Secret name"), "NEW_PROVIDER_KEY");
    await user.type(screen.getByLabelText("Secret value"), SECRET_VALUE);
    await user.click(
      screen.getByLabelText("I understand this stores the key on the server"),
    );
    await user.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByTestId("secret-stored");

    unmount();
    install(bridge);
    render(<SecretStore />);
    await screen.findByRole("list", { name: "Stored server secrets" });

    expect(
      screen.queryByTestId("secret-first-warning"),
    ).not.toBeInTheDocument();
    await user.type(screen.getByLabelText("Secret name"), "ANOTHER_KEY");
    await user.type(screen.getByLabelText("Secret value"), "another-value");
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  it("clears the value before set resolves and keeps it out of the DOM", async () => {
    const user = userEvent.setup();
    let resolveSet: ((outcome: SecretSetOutcome) => void) | undefined;
    const set = vi.fn(
      () =>
        new Promise<SecretSetOutcome>((resolve) => {
          resolveSet = resolve;
        }),
    );
    renderStore({ set });
    await screen.findByRole("list", { name: "Stored server secrets" });

    await user.type(screen.getByLabelText("Secret name"), "NEW_PROVIDER_KEY");
    const valueInput = screen.getByLabelText("Secret value");
    await user.type(valueInput, SECRET_VALUE);
    await user.click(
      screen.getByLabelText("I understand this stores the key on the server"),
    );
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(valueInput).toHaveValue("");
    expect(set).toHaveBeenCalledTimes(1);
    expect(set).toHaveBeenCalledWith({
      name: "NEW_PROVIDER_KEY",
      value: SECRET_VALUE,
    });
    expect(document.body.textContent).not.toContain(SECRET_VALUE);
    expect(document.body.innerHTML).not.toContain(SECRET_VALUE);

    await act(async () => {
      resolveSet?.({ name: "NEW_PROVIDER_KEY", last4: "7c3d" });
    });

    expect(await screen.findByTestId("secret-stored")).toHaveTextContent(
      "stored NEW_PROVIDER_KEY, ends in …7c3d",
    );
    expect(screen.getByLabelText("Secret name")).toHaveValue("");
    expect(valueInput).toHaveValue("");
    expect(screen.queryByTestId("secret-first-warning")).not.toBeInTheDocument();
  });

  it("shows a typed set error, clears the value and leaves the list usable", async () => {
    const user = userEvent.setup();
    const set = vi.fn(
      async (): Promise<SecretSetOutcome> => {
        throw {
          code: "ssh_auth",
          message: "SSH authentication failed",
          hint: "Check the SSH agent or the identity file path.",
          retryable: true,
        };
      },
    );
    renderStore({ set });
    await screen.findByRole("list", { name: "Stored server secrets" });

    await user.type(screen.getByLabelText("Secret name"), "NEW_PROVIDER_KEY");
    const valueInput = screen.getByLabelText("Secret value");
    await user.type(valueInput, SECRET_VALUE);
    await user.click(
      screen.getByLabelText("I understand this stores the key on the server"),
    );
    await user.click(screen.getByRole("button", { name: "Save" }));

    const alert = await screen.findByTestId("secret-set-error");
    expect(alert).toHaveTextContent("SSH authentication failed");
    expect(alert).toHaveTextContent(
      "Check the SSH agent or the identity file path.",
    );
    expect(valueInput).toHaveValue("");
    expect(document.body.textContent).not.toContain(SECRET_VALUE);
    expect(screen.getByLabelText("Secret name")).toHaveValue("NEW_PROVIDER_KEY");
    expect(
      screen.getByRole("list", { name: "Stored server secrets" }),
    ).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Revoke" })).toHaveLength(2);

    // The form stays usable: the value is empty, not the name or the controls.
    await user.type(valueInput, "second-attempt-value");
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  it("requires the inline revoke confirmation, cancel does nothing", async () => {
    const user = userEvent.setup();
    const remove = vi.fn(
      async (name: string): Promise<SecretRemoveOutcome> => ({
        name,
        removed: true,
      }),
    );
    renderStore({ remove });
    await screen.findByRole("list", { name: "Stored server secrets" });

    const row = screen.getByTestId("secret-row-OPENAI_API_KEY");
    await user.click(within(row).getByRole("button", { name: "Revoke" }));
    expect(remove).not.toHaveBeenCalled();
    expect(within(row).getByText("Revoke OPENAI_API_KEY?")).toBeInTheDocument();

    await user.click(within(row).getByRole("button", { name: "Cancel" }));
    expect(remove).not.toHaveBeenCalled();
    expect(within(row).queryByText("Revoke OPENAI_API_KEY?")).not.toBeInTheDocument();
    expect(screen.getByTestId("secret-row-OPENAI_API_KEY")).toBeInTheDocument();
  });

  it("confirms revoke, calls remove and refreshes the list", async () => {
    const user = userEvent.setup();
    let rows = [...DEFAULT_ROWS];
    const list = vi.fn(async () => listOf(rows));
    const remove = vi.fn(
      async (name: string): Promise<SecretRemoveOutcome> => {
        rows = rows.filter((row) => row.name !== name);
        return { name, removed: true };
      },
    );
    renderStore({ list, remove });
    await screen.findByRole("list", { name: "Stored server secrets" });

    const row = screen.getByTestId("secret-row-OPENAI_API_KEY");
    await user.click(within(row).getByRole("button", { name: "Revoke" }));
    const confirm = within(row).getByRole("group", {
      name: "Confirm revoke OPENAI_API_KEY",
    });
    await user.click(within(confirm).getByRole("button", { name: "Revoke" }));

    await waitFor(() => expect(remove).toHaveBeenCalledWith("OPENAI_API_KEY"));
    await waitFor(() =>
      expect(screen.queryByTestId("secret-row-OPENAI_API_KEY")).not.toBeInTheDocument(),
    );
    expect(list).toHaveBeenCalledTimes(2);
    expect(
      screen.getByTestId("secret-row-ANTHROPIC_API_KEY"),
    ).toBeInTheDocument();
    expect(await screen.findByTestId("secret-notice")).toHaveTextContent(
      "Removed OPENAI_API_KEY.",
    );
  });

  it("refreshes and explains when a revoke target was already gone", async () => {
    const user = userEvent.setup();
    let rows = [entry("OPENAI_API_KEY", "9f2a")];
    const list = vi.fn(async () => listOf(rows));
    const remove = vi.fn(async (name: string): Promise<SecretRemoveOutcome> => {
      rows = [];
      throw {
        code: "not_found",
        message: `secret ${name}`,
        hint: null,
        retryable: false,
      };
    });
    renderStore({ list, remove });
    await screen.findByRole("list", { name: "Stored server secrets" });

    const row = screen.getByTestId("secret-row-OPENAI_API_KEY");
    await user.click(within(row).getByRole("button", { name: "Revoke" }));
    const confirm = within(row).getByRole("group", {
      name: "Confirm revoke OPENAI_API_KEY",
    });
    await user.click(within(confirm).getByRole("button", { name: "Revoke" }));

    expect(await screen.findByTestId("secret-notice")).toHaveTextContent(
      "OPENAI_API_KEY was already gone. The list was refreshed.",
    );
    expect(
      screen.queryByTestId("secret-row-OPENAI_API_KEY"),
    ).not.toBeInTheDocument();
    expect(list).toHaveBeenCalledTimes(2);
  });

  it("keeps the row and shows the typed error when revoke fails", async () => {
    const user = userEvent.setup();
    const list = vi.fn(async () => listOf([entry("OPENAI_API_KEY", "9f2a")]));
    const remove = vi.fn(async (): Promise<SecretRemoveOutcome> => {
      throw {
        code: "ssh_unavailable",
        message: "SSH is not available",
        hint: "Check the connection and retry.",
        retryable: true,
      };
    });
    renderStore({ list, remove });
    await screen.findByRole("list", { name: "Stored server secrets" });

    const row = screen.getByTestId("secret-row-OPENAI_API_KEY");
    await user.click(within(row).getByRole("button", { name: "Revoke" }));
    const confirm = within(row).getByRole("group", {
      name: "Confirm revoke OPENAI_API_KEY",
    });
    await user.click(within(confirm).getByRole("button", { name: "Revoke" }));

    const alert = await screen.findByTestId("secret-row-error-OPENAI_API_KEY");
    expect(alert).toHaveTextContent("SSH is not available");
    expect(screen.getByTestId("secret-row-OPENAI_API_KEY")).toBeInTheDocument();
    expect(list).toHaveBeenCalledTimes(1);
  });

  it("says VOWP did not report a storage mode when none is given", async () => {
    renderStore({ list: async () => listOf([entry("OPENAI_API_KEY", "9f2a")], null) });

    expect(await screen.findByText("VOWP did not report a storage mode")).toBeInTheDocument();
    expect(screen.queryByTestId("vault-mode")).not.toBeInTheDocument();
    expect(screen.queryByText(/0\.4\.0/)).not.toBeInTheDocument();
  });

  it.each([
    ["basic", "Basic", "Files under the runtime user, mode 0600, not encrypted at rest."],
    ["vault_user", "Vault user", "Stored in the vault user's home."],
    ["encrypted", "Encrypted", "Encrypted at rest with age."],
  ])("shows the %s storage mode badge and note", async (mode, label, note) => {
    renderStore({
      list: async () => listOf([entry("OPENAI_API_KEY", "9f2a")], mode),
    });

    const badge = await screen.findByTestId("vault-mode");
    expect(badge).toHaveTextContent(label);
    expect(screen.getByTestId("vault-mode-note")).toHaveTextContent(note);
    expect(screen.queryByText("VOWP did not report a storage mode")).not.toBeInTheDocument();
  });

  it("shows an unknown storage token raw and says VOWP reported it", async () => {
    renderStore({
      list: async () => listOf([entry("OPENAI_API_KEY", "9f2a")], "future_mode"),
    });

    expect(await screen.findByTestId("vault-mode")).toHaveTextContent("future_mode");
    expect(screen.getByTestId("vault-mode-note")).toHaveTextContent(
      "VOWP reported this storage mode: future_mode.",
    );
  });

  it("disables the form with the exact reason when set is not supported", async () => {
    const user = userEvent.setup();
    const set = vi.fn(
      async (): Promise<SecretSetOutcome> => {
        throw {
          code: "not_supported",
          message: "secret set is not supported by this VOWP build",
          hint: "Use the server's own VOWP CLI.",
          retryable: false,
        };
      },
    );
    renderStore({ set });
    await screen.findByRole("list", { name: "Stored server secrets" });

    await user.type(screen.getByLabelText("Secret name"), "NEW_PROVIDER_KEY");
    await user.type(screen.getByLabelText("Secret value"), SECRET_VALUE);
    await user.click(
      screen.getByLabelText("I understand this stores the key on the server"),
    );
    await user.click(screen.getByRole("button", { name: "Save" }));

    const blocked = await screen.findByTestId("secret-set-unavailable");
    expect(blocked).toHaveTextContent(
      "secret set is not supported by this VOWP build",
    );
    expect(screen.getByLabelText("Secret name")).toBeDisabled();
    expect(screen.getByLabelText("Secret value")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(screen.getAllByRole("button", { name: "Revoke" })).toHaveLength(2);
    expect(document.body.textContent).not.toContain(SECRET_VALUE);
  });

  it("appears in Settings under the advanced section", async () => {
    const user = userEvent.setup();
    const { bridge } = fakeBridge({ connection: disconnected() });
    install(bridge, disconnected());
    render(<SettingsView theme="system" onTheme={() => undefined} />);

    expect(screen.queryByTestId("server-secrets")).not.toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "Show advanced settings" }),
    );

    expect(screen.getByTestId("server-secrets")).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Server secrets" }),
    ).toBeInTheDocument();
  });
});
