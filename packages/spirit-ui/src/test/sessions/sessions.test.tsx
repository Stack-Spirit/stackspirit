import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { describe, expect, it, vi } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import type { AsifImportResult } from "../../bridge/wire";
import {
  EXPORT_SESSION_ID,
  fleetWithSessions,
  gatesFrom,
  renderSessions,
  renderSessionsHarness,
} from "./helpers";

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];

async function wcagViolations(container: HTMLElement) {
  const results = await axe.run(container, {
    runOnly: { type: "tag", values: WCAG_TAGS },
  });
  return results.violations;
}

function sessionsApi(bridge: MockBridge) {
  const api = bridge.sessions;
  if (!api) throw new Error("MockBridge is missing its sessions surface");
  return api;
}

function firstRow() {
  return screen.getByTestId(`session-row-${EXPORT_SESSION_ID}`);
}

describe("Sessions drawer gate state", () => {
  it("waiting_gate_disables_both_directions_and_names_missing_milestones", async () => {
    const bridge = new MockBridge("gate-asif-waiting");
    const gates = await bridge.gates.get();
    renderSessions({ bridge, gates });

    expect(within(firstRow()).getByRole("button", { name: "Export .asif.zip" })).toBeDisabled();
    expect(screen.getByTestId("sessions-import")).toBeDisabled();
    expect(
      screen.getByTestId(`session-export-reason-${EXPORT_SESSION_ID}`),
    ).toHaveTextContent("v4-asif-core");
    expect(screen.getByTestId("sessions-import-reason")).toHaveTextContent("v4-asif-cli");
    expect(screen.getByText("Waiting")).toBeInTheDocument();
  });

  it("partial_ready_enables_export_only_and_explains_import", async () => {
    const bridge = new MockBridge("gate-asif-ready-export-only");
    const gates = await bridge.gates.get();
    renderSessions({ bridge, gates });

    expect(within(firstRow()).getByRole("button", { name: "Export .asif.zip" })).toBeEnabled();
    expect(screen.getByTestId("sessions-import")).toBeDisabled();
    expect(screen.getByTestId("sessions-import-reason")).toHaveTextContent(
      "ASIF import capability is not proven.",
    );
    expect(screen.getByText("Ready · export only")).toBeInTheDocument();
  });

  it("feature_flag_off_keeps_both_directions_disabled_when_the_gate_is_ready", async () => {
    const bridge = new MockBridge("gate-asif-ready");
    const gates = await bridge.gates.get();
    gates.features.asif_actions = false;
    renderSessions({ bridge, gates });

    expect(within(firstRow()).getByRole("button", { name: "Export .asif.zip" })).toBeDisabled();
    expect(screen.getByTestId("sessions-import")).toBeDisabled();
    expect(
      screen.getByTestId(`session-export-reason-${EXPORT_SESSION_ID}`),
    ).toHaveTextContent("not implemented in this build");
    expect(screen.getByTestId("sessions-import-reason")).toHaveTextContent(
      "not implemented in this build",
    );
  });

  it("ready_gate_and_flag_enable_both_directions", async () => {
    const bridge = new MockBridge("gate-asif-ready");
    const gates = await bridge.gates.get();
    renderSessions({ bridge, gates });

    expect(within(firstRow()).getByRole("button", { name: "Export .asif.zip" })).toBeEnabled();
    expect(screen.getByTestId("sessions-import")).toBeEnabled();
    expect(screen.queryByTestId("sessions-import-reason")).not.toBeInTheDocument();
    expect(
      screen.queryByTestId(`session-export-reason-${EXPORT_SESSION_ID}`),
    ).not.toBeInTheDocument();
    expect(screen.getByText("Ready")).toBeInTheDocument();
  });

  it("disconnected_and_stale_states_disable_actions_and_say_why", async () => {
    const bridge = new MockBridge("gate-asif-ready");
    const gates = await bridge.gates.get();
    const fleet = fleetWithSessions();
    const disconnected = {
      state: "disconnected" as const,
      host_id: null,
      since: null,
      latency: null,
      last_ok_at: null,
      error: null,
    };
    renderSessions({ bridge, gates, fleet, connection: disconnected });

    expect(screen.getByTestId("sessions-disconnected")).toHaveTextContent(
      "Not connected to a VOWP home",
    );
    expect(within(firstRow()).getByRole("button", { name: "Export .asif.zip" })).toBeDisabled();
    expect(
      screen.getByTestId(`session-export-reason-${EXPORT_SESSION_ID}`),
    ).toHaveTextContent("Connect to a VOWP home");
    expect(screen.getByTestId("sessions-import-reason")).toHaveTextContent(
      "Connect to a VOWP home",
    );
  });

  it("shows the stale snapshot notice without enabling writes", async () => {
    const bridge = new MockBridge("gate-asif-ready");
    const gates = await bridge.gates.get();
    const fleet = fleetWithSessions();
    const stale = {
      state: "stale" as const,
      host_id: fleet.host_id,
      since: fleet.fetched_at,
      latency: fleet.latency,
      last_ok_at: fleet.fetched_at,
      error: {
        code: "remote_timeout",
        message: "the remote command did not finish within 20s",
        hint: null,
        retryable: true,
      },
    };
    renderSessions({ bridge, gates, fleet, connection: stale });

    expect(screen.getByTestId("sessions-stale")).toHaveTextContent(
      "The VOWP connection is stale",
    );
    expect(screen.getByTestId("sessions-import")).toBeDisabled();
    expect(
      screen.getByTestId(`session-export-reason-${EXPORT_SESSION_ID}`),
    ).toHaveTextContent("Reconnect before exporting or importing");
  });

  it("disables writes when a connected fleet snapshot is stale", async () => {
    const bridge = new MockBridge("gate-asif-ready");
    const gates = await bridge.gates.get();
    const fleet = fleetWithSessions();
    fleet.stale = true;
    // seedStore's default connection is connected and current, so only `fleet.stale`
    // distinguishes this case from a fresh snapshot.
    renderSessions({ bridge, gates, fleet });

    expect(screen.getByTestId("sessions-stale")).toHaveTextContent("marked stale");
    expect(screen.getByTestId("sessions-import")).toBeDisabled();
    expect(screen.getByTestId("sessions-import-reason")).toHaveTextContent(
      "The session snapshot is stale",
    );
    expect(within(firstRow()).getByRole("button", { name: "Export .asif.zip" })).toBeDisabled();
    expect(
      screen.getByTestId(`session-export-reason-${EXPORT_SESSION_ID}`),
    ).toHaveTextContent("The session snapshot is stale");
  });

  it("a_gate_from_a_previous_connection_never_enables_actions", async () => {
    const bridge = new MockBridge("gate-asif-ready");
    const gates = await bridge.gates.get();
    const fleet = fleetWithSessions();
    const connection = {
      state: "connected" as const,
      host_id: fleet.host_id,
      since: "2026-11-14T12:00:01.000Z",
      latency: fleet.latency,
      last_ok_at: "2026-11-14T12:00:01.000Z",
      error: null,
    };
    renderSessions({ bridge, gates, fleet, connection });

    expect(screen.getByText("Unknown")).toBeInTheDocument();
    expect(screen.getByTestId("sessions-import")).toBeDisabled();
    expect(screen.getByTestId("sessions-import-reason")).toHaveTextContent(
      "Probe the VOWP home to check this capability.",
    );
    expect(
      screen.getByTestId(`session-export-reason-${EXPORT_SESSION_ID}`),
    ).toHaveTextContent("Probe the VOWP home to check this capability.");
  });

  it("labels a session with its real VOWP state", async () => {
    const bridge = new MockBridge("gate-asif-ready");
    const gates = await bridge.gates.get();
    renderSessions({ bridge, gates });

    expect(within(firstRow()).getByText("Running")).toBeInTheDocument();
    expect(within(firstRow()).queryByText(/Unknown/)).not.toBeInTheDocument();
  });

  it("empty_snapshot_names_the_empty_state", async () => {
    const bridge = new MockBridge("gate-asif-ready");
    const gates = await bridge.gates.get();
    const fleet = fleetWithSessions();
    fleet.sessions = [];
    renderSessions({ bridge, gates, fleet });

    expect(screen.getByText("No sessions in the last snapshot")).toBeInTheDocument();
    expect(screen.getByTestId("sessions-import")).toBeEnabled();
  });
});

describe("export flow", () => {
  it("exports_with_declared_losses_and_shows_only_the_local_file_name", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge("asif-export-losses");
    const gates = await bridge.gates.get();
    renderSessions({ bridge, gates });

    await user.click(within(firstRow()).getByRole("button", { name: "Export .asif.zip" }));
    expect(screen.getByTestId("export-panel")).toBeInTheDocument();
    expect(screen.getByTestId("export-policy-allow-loss")).toBeChecked();
    expect(screen.getByTestId("export-policy-warning")).toHaveTextContent(
      "Allow-loss is the default",
    );

    await user.click(screen.getByRole("button", { name: "Export .asif.zip" }));
    expect(await screen.findByTestId("export-result")).toBeVisible();

    expect(screen.getByTestId("export-file-name")).toHaveTextContent(
      `${EXPORT_SESSION_ID}.asif.zip`,
    );
    expect(screen.getByTestId("export-losses")).toHaveTextContent("cwd-omitted");
    expect(screen.getByTestId("export-losses")).toHaveTextContent(
      "contexts-reconstructed",
    );
    expect(screen.getByTestId("export-unsupported")).toHaveTextContent("vowp.jobs");
    expect(screen.getByTestId("export-result")).not.toHaveTextContent(/lossless/i);
    expect(document.body.textContent ?? "").not.toMatch(/\/srv|\/tmp/);

    expect(bridge.recordedAsifCalls()).toEqual([
      {
        kind: "export",
        session_id: EXPORT_SESSION_ID,
        strict: false,
        allow_loss: true,
        path: null,
      },
    ]);
  });

  it("strict_refusal_is_specific_and_allow_loss_is_a_separate_choice", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge("asif-strict-refusal");
    const gates = await bridge.gates.get();
    renderSessions({ bridge, gates });

    await user.click(within(firstRow()).getByRole("button", { name: "Export .asif.zip" }));
    await user.click(screen.getByTestId("export-policy-strict"));
    await user.click(screen.getByRole("button", { name: "Export .asif.zip" }));

    expect(await screen.findByTestId("export-error")).toHaveTextContent(
      "Export refused (exit 5)",
    );
    expect(screen.getByTestId("export-error")).toHaveTextContent(
      "blocking or integrity loss",
    );
    expect(bridge.recordedAsifCalls()).toHaveLength(1);

    await user.click(screen.getByRole("button", { name: "Choose allow-loss" }));
    expect(screen.getByTestId("export-policy-allow-loss")).toBeChecked();
    // Choosing a policy never retries by itself.
    expect(bridge.recordedAsifCalls()).toHaveLength(1);

    await user.click(screen.getByRole("button", { name: "Export .asif.zip" }));
    expect(await screen.findByTestId("export-result")).toBeVisible();
    expect(bridge.recordedAsifCalls()).toHaveLength(2);
    expect(bridge.recordedAsifCalls()[1]).toMatchObject({
      kind: "export",
      strict: false,
      allow_loss: true,
    });
  });

  it("save_cancel_says_nothing_was_saved", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge("gate-asif-ready");
    const gates = await bridge.gates.get();
    vi.spyOn(sessionsApi(bridge), "exportAsif").mockResolvedValueOnce(null);
    renderSessions({ bridge, gates });

    await user.click(within(firstRow()).getByRole("button", { name: "Export .asif.zip" }));
    await user.click(screen.getByRole("button", { name: "Export .asif.zip" }));

    expect(await screen.findByTestId("export-cancelled")).toHaveTextContent(
      "Nothing was saved.",
    );
    expect(screen.queryByTestId("export-result")).not.toBeInTheDocument();
  });

  it("shows a specific invalid session id reason", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge("gate-asif-ready");
    const gates = await bridge.gates.get();
    vi.spyOn(sessionsApi(bridge), "exportAsif").mockRejectedValueOnce({
      code: "invalid_input",
      message: "session id: must be a 26-character ULID",
      hint: null,
      retryable: false,
    });
    renderSessions({ bridge, gates });

    await user.click(within(firstRow()).getByRole("button", { name: "Export .asif.zip" }));
    await user.click(screen.getByRole("button", { name: "Export .asif.zip" }));

    expect(await screen.findByTestId("export-error")).toHaveTextContent("Invalid session id");
  });
});

describe("import flow", () => {
  it("imports_an_idle_session_and_shows_losses_and_warnings_without_auto_run", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge("asif-import-warnings");
    const gates = await bridge.gates.get();
    renderSessions({ bridge, gates });

    await user.click(screen.getByTestId("sessions-import"));
    expect(screen.getByTestId("import-panel")).toBeInTheDocument();
    expect(screen.getByTestId("import-size-limit")).toHaveTextContent("272 MiB");
    expect(screen.getByTestId("import-policy-allow-loss")).toBeChecked();

    await user.click(screen.getByRole("button", { name: "Import .asif.zip" }));
    expect(await screen.findByTestId("import-result")).toBeVisible();

    expect(screen.getByTestId("import-session-id")).toHaveTextContent(
      "01JCF5WQ8ZK3M2V7N4P6R8T0AF",
    );
    expect(screen.getByTestId("import-state")).toHaveTextContent("Idle");
    expect(screen.getByTestId("import-losses")).toHaveTextContent("session-id-changed");
    expect(screen.getByTestId("import-losses")).toHaveTextContent("cwd-rebound");
    expect(screen.getByTestId("import-warnings")).toHaveTextContent(
      "project example-source left null",
    );
    expect(screen.getByTestId("import-unsupported")).toHaveTextContent(
      "VOWP declared no unsupported operations.",
    );
    expect(screen.getByTestId("import-idle-note")).toHaveTextContent("did not run a turn");
    expect(
      screen.queryByRole("button", { name: /^(run|start|continue|resume)\b/i }),
    ).not.toBeInTheDocument();
    expect(bridge.recordedActions()).toEqual([]);
    expect(bridge.recordedAsifCalls()).toHaveLength(1);
    expect(bridge.recordedAsifCalls()[0]).toMatchObject({
      kind: "import",
      strict: false,
      allow_loss: true,
    });
  });

  it("shows_unsupported_features_from_a_successful_import", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge("gate-asif-ready");
    const gates = await bridge.gates.get();
    const imported: AsifImportResult = {
      session_id: "01JCF5WQ8ZK3M2V7N4P6R8T0AF",
      loss_count: 1,
      losses: [
        {
          kind: "capture-id-not-preserved",
          stage: "translation",
          scope: "native",
          message: "The source capture identifier was not preserved.",
        },
      ],
      warnings: [],
      unsupported: ["continuation", "external_bindings"],
      state: "idle",
    };
    vi.spyOn(sessionsApi(bridge), "importAsif").mockResolvedValueOnce(imported);
    renderSessions({ bridge, gates });

    await user.click(screen.getByTestId("sessions-import"));
    await user.click(screen.getByRole("button", { name: "Import .asif.zip" }));
    expect(await screen.findByTestId("import-result")).toBeVisible();
    expect(screen.getByTestId("import-unsupported")).toHaveTextContent("continuation");
    expect(screen.getByTestId("import-unsupported")).toHaveTextContent(
      "external_bindings",
    );
    expect(screen.getByTestId("import-unsupported")).toHaveTextContent(
      "StackSpirit never forces it",
    );
  });

  it("conflict_gets_one_message_and_never_retries_or_mutates_the_package", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge("asif-conflict");
    const gates = await bridge.gates.get();
    renderSessions({ bridge, gates });

    await user.click(screen.getByTestId("sessions-import"));
    await user.click(screen.getByRole("button", { name: "Import .asif.zip" }));

    expect(await screen.findByTestId("import-error")).toHaveTextContent(
      "Import conflict (exit 4)",
    );
    expect(screen.getByTestId("import-error")).toHaveTextContent(
      "StackSpirit does not retry with a mutated package.",
    );
    expect(bridge.recordedAsifCalls().filter((call) => call.kind === "import")).toHaveLength(
      1,
    );
    expect(screen.queryByRole("button", { name: /choose allow-loss/i })).not.toBeInTheDocument();
  });

  it.each([
    {
      name: "stale fleet view",
      error: {
        code: "conflict",
        message: "the fleet view is stale, so an ASIF import would be unsafe",
        hint: "Refresh the fleet view, then retry the import.",
        retryable: false,
      },
      hint: "Refresh the fleet view, then retry the import.",
    },
    {
      name: "package changed while reading",
      error: {
        code: "conflict",
        message: "the selected ASIF package changed while it was being read",
        hint: "Choose the file again.",
        retryable: false,
      },
      hint: "Choose the file again.",
    },
  ])("shows the host's $name conflict hint in the panel", async ({ error, hint }) => {
    const user = userEvent.setup();
    const bridge = new MockBridge("gate-asif-ready");
    const gates = await bridge.gates.get();
    vi.spyOn(sessionsApi(bridge), "importAsif").mockRejectedValueOnce(error);
    renderSessions({ bridge, gates });

    await user.click(screen.getByTestId("sessions-import"));
    await user.click(screen.getByRole("button", { name: "Import .asif.zip" }));

    const panel = await screen.findByTestId("import-error");
    expect(panel).toHaveTextContent("Import conflict (exit 4)");
    expect(panel).toHaveTextContent(hint);
    expect(panel).toHaveTextContent(error.message);
  });

  it.each([
    {
      name: "invalid package exit 2",
      error: {
        code: "invalid_input",
        message: "asif import response: expected an object",
        hint: null,
        retryable: false,
      },
      title: "Package rejected (exit 2)",
    },
    {
      name: "unknown session exit 3",
      error: {
        code: "not_found",
        message: "session 01JCF5WQ8ZK3M2V7N4P6R8T0AC not found",
        hint: null,
        retryable: false,
      },
      title: "Unknown source session (exit 3)",
    },
    {
      name: "transport failure exit 7",
      error: {
        code: "remote",
        message: "the remote command exited 7",
        hint: null,
        retryable: false,
      },
      title: "Transport failure (exit 7)",
    },
    {
      name: "oversized package",
      error: {
        code: "invalid_input",
        message: "the ASIF package exceeds the 285212672 byte limit",
        hint: null,
        retryable: false,
      },
      title: "Package exceeds the 272 MiB limit",
    },
  ])("maps $name to fixed copy without a retry action", async ({ error, title }) => {
    const user = userEvent.setup();
    const bridge = new MockBridge("gate-asif-ready");
    const gates = await bridge.gates.get();
    vi.spyOn(sessionsApi(bridge), "importAsif").mockRejectedValueOnce(error);
    renderSessions({ bridge, gates });

    await user.click(screen.getByTestId("sessions-import"));
    await user.click(screen.getByRole("button", { name: "Import .asif.zip" }));

    const panel = await screen.findByTestId("import-error");
    expect(panel).toHaveTextContent(title);
    expect(panel).toHaveTextContent(error.message);
    expect(screen.queryByRole("button", { name: /retry/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /choose allow-loss/i })).not.toBeInTheDocument();
  });

  it("strict_refusal_is_specific_and_does_not_auto_retry", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge("gate-asif-ready");
    const gates = await bridge.gates.get();
    const importSpy = vi.spyOn(sessionsApi(bridge), "importAsif").mockRejectedValueOnce({
      code: "refused",
      message: "VOWP refused the ASIF import: an unsupported feature is required.",
      hint: "Review the package.",
      retryable: false,
    });
    renderSessions({ bridge, gates });

    await user.click(screen.getByTestId("sessions-import"));
    await user.click(screen.getByTestId("import-policy-strict"));
    await user.click(screen.getByRole("button", { name: "Import .asif.zip" }));

    expect(await screen.findByTestId("import-error")).toHaveTextContent(
      "Import refused (exit 5)",
    );
    expect(importSpy).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(importSpy).toHaveBeenCalledTimes(1));
    expect(screen.getByRole("button", { name: "Choose allow-loss" })).toBeInTheDocument();
    expect(importSpy).toHaveBeenCalledTimes(1);
  });

  it("import_cancel_says_nothing_was_imported", async () => {
    const user = userEvent.setup();
    const bridge = new MockBridge("gate-asif-ready");
    const gates = await bridge.gates.get();
    vi.spyOn(sessionsApi(bridge), "importAsif").mockResolvedValueOnce(null);
    renderSessions({ bridge, gates });

    await user.click(screen.getByTestId("sessions-import"));
    await user.click(screen.getByRole("button", { name: "Import .asif.zip" }));
    expect(await screen.findByTestId("import-cancelled")).toHaveTextContent(
      "Nothing was imported.",
    );
  });
});

describe("Sessions drawer accessibility", () => {
  it("escape_closes_the_drawer_and_restores_focus", async () => {
    const user = userEvent.setup();
    const gates = await gatesFrom("gate-asif-ready");
    renderSessionsHarness({ gates });
    const trigger = screen.getByRole("button", { name: "Open sessions" });
    await user.click(trigger);

    const dialog = await screen.findByRole("dialog", { name: "Sessions" });
    expect(within(dialog).getByRole("button", { name: "Close drawer" })).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Import .asif.zip" })).toBeEnabled();
    expect(within(dialog).getAllByRole("button", { name: "Export .asif.zip" })).toHaveLength(
      2,
    );
    for (const exportButton of within(dialog).getAllByRole("button", {
      name: "Export .asif.zip",
    })) {
      expect(exportButton).toBeEnabled();
    }

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(document.activeElement).toBe(trigger);
  });

  it("names the loss policy group and both radio choices", async () => {
    const user = userEvent.setup();
    const gates = await gatesFrom("gate-asif-ready");
    renderSessions({ gates });

    await user.click(screen.getByTestId("sessions-import"));
    expect(
      screen.getByRole("group", { name: "Loss policy (advanced)" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("radio", { name: /Allow declared losses/ }),
    ).toBeChecked();
    expect(
      screen.getByRole("radio", { name: /Strict — refuse blocking or integrity losses/ }),
    ).not.toBeChecked();
  });

  it("has no critical or serious axe violations on the sessions drawer", async () => {
    const bridge = new MockBridge("asif-import-warnings");
    const gates = await bridge.gates.get();
    renderSessions({ bridge, gates });
    const violations = (await wcagViolations(document.body)).filter(
      (violation) => violation.impact === "critical" || violation.impact === "serious",
    );
    expect(violations).toEqual([]);
  });
});

