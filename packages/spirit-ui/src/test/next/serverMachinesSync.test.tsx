import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import type { HostProfile } from "../../bridge/wire";
import { resetCloudAccount, useCloudAccount } from "../../cloud/store";
import type { CloudMachine } from "../../cloud/types";
import { ServerMachinesSettings, useMachineSync } from "../../next/ServerMachinesSettings";
import { Sidebar } from "../../next/Sidebar";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

const AT = "2026-11-28T10:00:00.000Z";

afterEach(() => {
  resetCloudAccount();
  useMachineSync.getState().reset();
  resetFleetStore();
});

async function setup(options: { signedIn: boolean; connect: boolean; machines?: CloudMachine[] }) {
  resetFleetStore();
  resetCloudAccount();
  const bridge = new MockBridge("busy");
  await useFleetStore.getState().initialize(bridge);
  const host = useFleetStore.getState().hosts[0] as HostProfile;
  if (options.connect) await useFleetStore.getState().connect(host.id);
  useCloudAccount.setState({
    phase: options.signedIn ? "signed_in" : "local",
    account: options.signedIn ? { id: "acct_1", email: "owner@example.com", name: "Owner" } : null,
    machines: options.machines ?? [],
  });
  return host;
}

function shell() {
  return (
    <>
      <Sidebar
        view={{ kind: "server" }}
        onNavigate={() => undefined}
        onAddServer={() => undefined}
        onAddProject={() => undefined}
        onFormCluster={() => undefined}
        onOpenSettings={() => undefined}
        resizeHandle={{}}
        resizing={false}
        onOpenRaw={() => undefined}
      />
      <ServerMachinesSettings onOpenAccount={() => undefined} />
    </>
  );
}

describe("machine Account sync", () => {
  it("shows Local only when signed out and offers no sync", async () => {
    await setup({ signedIn: false, connect: true });
    render(shell());
    expect(screen.getByTestId("sidebar-sync-badge")).toHaveTextContent("Local only");
    expect(screen.getByTestId("settings-home-sync-badge")).toHaveTextContent("Local only");
    expect(screen.queryByText("Sync to Account")).toBeNull();
  });

  it("shows Account-synced for a machine enrolled on the signed-in Account", async () => {
    const host = await setup({ signedIn: true, connect: true });
    useCloudAccount.setState({
      machines: [
        { id: "mch_1", name: host.label, status: "online", project: null, last_seen_at: AT, provisioning: null },
      ],
    });
    render(shell());
    expect(screen.getByTestId("sidebar-sync-badge")).toHaveTextContent("Account-synced");
    expect(screen.getByTestId("settings-home-sync-badge")).toHaveAttribute("data-sync-status", "synced");
    expect(screen.queryByText("Sync to Account")).toBeNull();
  });

  it("syncs a connected Local-only machine in one click", async () => {
    const host = await setup({ signedIn: true, connect: true });
    const enroller = vi.fn().mockResolvedValue(undefined);
    useMachineSync.getState().configure(enroller);
    useCloudAccount.setState({ refreshMachines: vi.fn().mockResolvedValue(undefined) });
    const user = userEvent.setup();
    render(<ServerMachinesSettings onOpenAccount={() => undefined} />);
    expect(screen.getByTestId("settings-home-sync-badge")).toHaveTextContent("Local only");
    await user.click(screen.getByTestId(`sync-to-account-${host.id}`));
    expect(enroller).toHaveBeenCalledTimes(1);
    expect(enroller).toHaveBeenCalledWith(host.id);
    await waitFor(() =>
      expect(screen.getByTestId("settings-home-sync-badge")).toHaveTextContent("Account-synced"),
    );
    expect(screen.queryByTestId(`sync-to-account-${host.id}`)).toBeNull();
  });

  it("explains a disconnected machine cannot be synced yet", async () => {
    const host = await setup({ signedIn: true, connect: false });
    useMachineSync.getState().configure(vi.fn());
    render(<ServerMachinesSettings onOpenAccount={() => undefined} />);
    expect(screen.getByTestId(`sync-to-account-${host.id}`)).toBeDisabled();
    expect(screen.getByTestId(`sync-reason-${host.id}`)).toHaveTextContent(/offline or disconnected/);
  });

  it("reports a failed sync without leaking details and stays Local only", async () => {
    const host = await setup({ signedIn: true, connect: true });
    useMachineSync.getState().configure(vi.fn().mockRejectedValue(new Error("BEGIN PRIVATE KEY relay-secret")));
    const user = userEvent.setup();
    const { container } = render(<ServerMachinesSettings onOpenAccount={() => undefined} />);
    await user.click(screen.getByTestId(`sync-to-account-${host.id}`));
    expect(await screen.findByTestId(`sync-error-${host.id}`)).toHaveTextContent("Nothing was changed");
    expect(container.textContent).not.toMatch(/PRIVATE KEY|relay-secret/);
    expect(screen.getByTestId("settings-home-sync-badge")).toHaveTextContent("Local only");
  });
});
