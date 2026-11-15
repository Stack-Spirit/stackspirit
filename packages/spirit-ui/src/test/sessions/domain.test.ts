import { describe, expect, it } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import {
  ASIF_PACKAGE_LIMIT_LABEL,
  asifActionAvailability,
  asifErrorCopy,
  asifGateBadge,
  asifLossCountMismatch,
  asifLossPolicyOptions,
  sessionStateLabel,
  sessionStateTone,
  sessionUpdated,
} from "../../domain/session";
import { NOW } from "../scenarios/fixtures";
import { sessionFixtures } from "./helpers";

describe("ASIF session domain", () => {
  it("keeps an unknown session state visible", () => {
    expect(sessionStateLabel("active")).toBe("Active");
    expect(sessionStateLabel("quarantined")).toBe("Unknown (quarantined)");
    expect(sessionStateTone("idle")).toBe("info");
    expect(sessionStateTone("failed")).toBe("danger");
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

  it("combines connection, gate, and feature flag for availability", async () => {
    const gates = await new MockBridge("gate-asif-ready").gates.get();
    expect(
      asifActionAvailability({
        gates,
        action: "asif_export",
        connectionState: "connected",
        bridgeAvailable: true,
      }),
    ).toEqual({ enabled: true, reason: "" });

    expect(
      asifActionAvailability({
        gates,
        action: "asif_import",
        connectionState: "stale",
        bridgeAvailable: true,
      }),
    ).toMatchObject({ enabled: false, reason: expect.stringContaining("stale") });

    gates.features.asif_actions = false;
    expect(
      asifActionAvailability({
        gates,
        action: "asif_import",
        connectionState: "connected",
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

  it("names the archive limit in the oversized-package error", () => {
    const copy = asifErrorCopy(
      {
        code: "invalid_input",
        message: `the ASIF package exceeds the ${ASIF_PACKAGE_LIMIT_LABEL} byte limit`,
        hint: null,
        retryable: false,
      },
      "import",
    );
    expect(copy.title).toBe(`Package exceeds the ${ASIF_PACKAGE_LIMIT_LABEL} limit`);
    expect(copy.next).toContain("No upload was attempted");
  });
});
