import { describe, expect, it } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import {
  ASIF_PACKAGE_LIMIT_BYTES,
  ASIF_PACKAGE_LIMIT_LABEL,
  asifActionAvailability,
  asifErrorCopy,
  asifGateBadge,
  asifLossCountMismatch,
  asifLossPolicyOptions,
  gatesForConnection,
  sessionStateLabel,
  sessionStateTone,
  sessionUpdated,
} from "../../domain/session";
import { NOW } from "../scenarios/fixtures";
import { sessionFixtures } from "./helpers";

describe("ASIF session domain", () => {
  it("maps every VOWP session state to a label and tone", () => {
    expect(sessionStateLabel("idle")).toBe("Idle");
    expect(sessionStateLabel("running")).toBe("Running");
    expect(sessionStateLabel("blocked")).toBe("Blocked");
    expect(sessionStateLabel("switching")).toBe("Switching");
    expect(sessionStateLabel("closed")).toBe("Closed");
    expect(sessionStateTone("idle")).toBe("info");
    expect(sessionStateTone("running")).toBe("ok");
    expect(sessionStateTone("blocked")).toBe("danger");
    expect(sessionStateTone("switching")).toBe("warn");
    expect(sessionStateTone("closed")).toBe("info");
  });

  it("keeps a non-contract session state visible", () => {
    expect(sessionStateLabel("unknown")).toBe("Unknown");
    expect(sessionStateLabel("quarantined")).toBe("Unknown (quarantined)");
    expect(sessionStateLabel("active")).toBe("Unknown (active)");
    expect(sessionStateTone("quarantined")).toBe("unknown");
  });

  it("uses the snapshot clock for the updated label", () => {
    const [session] = sessionFixtures();
    if (!session) throw new Error("fixture missing");
    expect(sessionUpdated(session, NOW).relative).toBe("updated 0s ago");
    expect(sessionUpdated(session, null).relative).toBe("updated unknown");
  });

  it("shows a partial ASIF gate as a warning, not a full green Ready", async () => {
    const partial = await new MockBridge("gate-asif-ready-export-only").gates.get();
    expect(asifGateBadge(partial)).toEqual({
      label: "Ready · export only",
      tone: "warn",
    });
    const full = await new MockBridge("gate-asif-ready").gates.get();
    expect(asifGateBadge(full)).toEqual({ label: "Ready", tone: "ok" });
  });

  it("drops a gate snapshot probed before the current connection", async () => {
    const gates = await new MockBridge("gate-asif-ready").gates.get();
    const connection = {
      state: "connected" as const,
      host_id: "01MOCKHOST000000000000000001",
      since: "2026-11-14T12:00:01.000Z",
      latency: null,
      last_ok_at: "2026-11-14T12:00:01.000Z",
      error: null,
    };
    expect(gatesForConnection(gates, connection)).toBeNull();
    expect(
      gatesForConnection(gates, { ...connection, since: "2026-11-14T11:59:00.000Z" }),
    ).toBe(gates);
  });

  it("combines connection, gate, and feature flag for availability", async () => {
    const gates = await new MockBridge("gate-asif-ready").gates.get();
    expect(
      asifActionAvailability({
        gates,
        action: "asif_export",
        connectionState: "connected",
        snapshotStale: false,
        bridgeAvailable: true,
      }),
    ).toEqual({ enabled: true, reason: "" });

    expect(
      asifActionAvailability({
        gates,
        action: "asif_import",
        connectionState: "stale",
        snapshotStale: false,
        bridgeAvailable: true,
      }),
    ).toMatchObject({ enabled: false, reason: expect.stringContaining("stale") });

    expect(
      asifActionAvailability({
        gates,
        action: "asif_import",
        connectionState: "connected",
        snapshotStale: true,
        bridgeAvailable: true,
      }),
    ).toMatchObject({
      enabled: false,
      reason: expect.stringContaining("session snapshot is stale"),
    });

    gates.features.asif_actions = false;
    expect(
      asifActionAvailability({
        gates,
        action: "asif_import",
        connectionState: "connected",
        snapshotStale: false,
        bridgeAvailable: true,
      }).reason,
    ).toContain("not implemented in this build");
  });

  it("maps the loss policy to exactly one bridge option", () => {
    expect(asifLossPolicyOptions("allow-loss")).toEqual({ allowLoss: true });
    expect(asifLossPolicyOptions("strict")).toEqual({ strict: true });
  });

  it("detects a loss-count mismatch instead of hiding it", () => {
    expect(asifLossCountMismatch(0, [])).toBe(false);
    expect(
      asifLossCountMismatch(2, [
        {
          kind: "cwd-omitted",
          stage: "translation",
          scope: "environment",
          message: "The working directory was omitted.",
        },
      ]),
    ).toBe(true);
  });

  it("maps every ASIF exit-code family to a fixed title", () => {
    expect(
      asifErrorCopy(
        {
          code: "refused",
          message: "VOWP refused the ASIF import.",
          hint: null,
          retryable: false,
        },
        "import",
      ).title,
    ).toBe("Import refused (exit 5)");
    expect(
      asifErrorCopy(
        {
          code: "conflict",
          message: "The ASIF import conflicts with an existing session's event ids.",
          hint: null,
          retryable: false,
        },
        "import",
      ).title,
    ).toBe("Import conflict (exit 4)");
    expect(
      asifErrorCopy(
        {
          code: "not_found",
          message: "session not found",
          hint: null,
          retryable: false,
        },
        "import",
      ).title,
    ).toBe("Unknown source session (exit 3)");
    expect(
      asifErrorCopy(
        {
          code: "not_found",
          message: "there is no active project in the current snapshot",
          hint: null,
          retryable: false,
        },
        "import",
      ).title,
    ).toBe("No active project");
    expect(
      asifErrorCopy(
        {
          code: "remote",
          message: "the remote command exited 7",
          hint: null,
          retryable: false,
        },
        "import",
      ).title,
    ).toBe("Transport failure (exit 7)");
    expect(
      asifErrorCopy(
        {
          code: "invalid_input",
          message: "asif import response: expected an object",
          hint: null,
          retryable: false,
        },
        "import",
      ).title,
    ).toBe("Package rejected (exit 2)");
  });

  it("passes the host's conflict hint through as the next step", () => {
    const stale = asifErrorCopy(
      {
        code: "conflict",
        message: "the fleet view is stale, so an ASIF import would be unsafe",
        hint: "Refresh the fleet view, then retry the import.",
        retryable: false,
      },
      "import",
    );
    expect(stale.title).toBe("Import conflict (exit 4)");
    expect(stale.next).toBe("Refresh the fleet view, then retry the import.");

    const changed = asifErrorCopy(
      {
        code: "conflict",
        message: "the selected ASIF package changed while it was being read",
        hint: "Choose the file again.",
        retryable: false,
      },
      "import",
    );
    expect(changed.title).toBe("Import conflict (exit 4)");
    expect(changed.next).toBe("Choose the file again.");

    const eventIds = asifErrorCopy(
      {
        code: "conflict",
        message: "The ASIF import conflicts with an existing session's event ids.",
        hint: null,
        retryable: false,
      },
      "import",
    );
    expect(eventIds.next).toContain("did not retry with a changed package");
  });

  it("names the archive limit in the oversized-package error", () => {
    const copy = asifErrorCopy(
      {
        code: "invalid_input",
        message: `the ASIF package exceeds the ${ASIF_PACKAGE_LIMIT_BYTES} byte limit`,
        hint: null,
        retryable: false,
      },
      "import",
    );
    expect(copy.title).toBe(`Package exceeds the ${ASIF_PACKAGE_LIMIT_LABEL} limit`);
    expect(copy.next).toContain("No upload was attempted");
  });
});
