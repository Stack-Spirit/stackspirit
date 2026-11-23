import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import type { ChatUsageView } from "../../bridge/wire";
import type { SpiritBridge } from "../../bridge/SpiritBridge";
import { MockBridge } from "../../bridge/MockBridge";
import {
  UsageStats,
  cacheHitPercent,
  formatCompactTokens,
  type TelemetryUsage,
} from "../../next/ContextMeter";
import { ContextMeter } from "../../next/ContextMeter";
import { messageStamp } from "../../next/ChatView";
import { resetConversations } from "../../next/conversations";
import { NextShell } from "../../next/NextShell";
import { resetShellUi } from "../../next/shellUi";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

const bridges: SpiritBridge[] = [];

afterEach(() => {
  for (const bridge of bridges.splice(0)) bridge.dispose?.();
  resetConversations();
  resetShellUi();
  resetFleetStore();
});

function usage(overrides: Partial<TelemetryUsage> = {}): TelemetryUsage {
  const base: ChatUsageView = {
    context_tokens_used: 23_600,
    context_limit: 128_000,
    input_tokens: 12_800,
    output_tokens: 2_400,
    cache_read_tokens: 8_400,
    turns: 3,
    cost_usd: null,
  };
  return { ...base, ...overrides };
}

describe("message stamp", () => {
  it("formats MM/DD HH:MM in local time and rejects bad input", () => {
    const d = new Date(2026, 10, 21, 10, 24);
    expect(messageStamp(d.toISOString())).toBe("11/21 10:24");
    expect(messageStamp(undefined)).toBeNull();
    expect(messageStamp("nope")).toBeNull();
  });
});

describe("chat messages", () => {
  it("user bubble has date/time and a working Copy button; assistant footer is formatted", async () => {
    resetFleetStore();
    resetConversations();
    const bridge = new MockBridge({ scenario: "chat-tools", hosts: [] });
    bridges.push(bridge);
    const user = userEvent.setup();
    render(<NextShell bridge={bridge} />);
    await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));
    await user.click(screen.getByRole("button", { name: /Set up with AI/ }));
    await user.type(await screen.findByRole("textbox", { name: "Message" }), "go{Enter}");

    const meta = await screen.findByTestId("user-meta");
    expect(within(meta).getByTestId("user-time").textContent).toMatch(
      /^\d{2}\/\d{2} \d{2}:\d{2}$/,
    );
    await user.click(within(meta).getByRole("button", { name: "Copy message" }));
    expect(within(meta).getByRole("button", { name: "Copied" })).toBeInTheDocument();

    const footer = await screen.findByTestId("answer-footer");
    expect(within(footer).getByRole("button", { name: "Copy answer" })).toBeInTheDocument();
    const answerMeta = within(footer).getByTestId("answer-meta");
    expect(answerMeta.textContent).toMatch(/\d{2}\/\d{2} \d{2}:\d{2}$/);
  });
});

describe("assistant footer", () => {
  async function openChat(scenario: "empty" | "chat-tools") {
    resetFleetStore();
    resetConversations();
    const bridge = scenario === "empty" ? new MockBridge(scenario) : new MockBridge({ scenario, hosts: [] });
    bridges.push(bridge);
    const user = userEvent.setup();
    render(<NextShell bridge={bridge} />);
    await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));
    await user.click(screen.getByRole("button", { name: /Set up with AI/ }));
    const box = await screen.findByRole("textbox", { name: "Message" });
    return { user, box };
  }

  it("shows session usage only on the latest reply; model and retry on every reply", async () => {
    const { user, box } = await openChat("empty");
    await user.type(box, "one{Enter}");
    await waitFor(() => expect(screen.getAllByTestId("answer-footer")).toHaveLength(1));
    await user.type(box, "two{Enter}");
    await waitFor(() => expect(screen.getAllByTestId("answer-footer")).toHaveLength(2));
    const footers = screen.getAllByTestId("answer-footer");
    expect(within(footers[0]!).queryByTestId("answer-usage")).toBeNull();
    expect(within(footers[1]!).getByTestId("answer-usage").textContent).toMatch(/Usage 2\d(\.\d)?K tok/);
    for (const footer of footers) {
      expect(within(footer).getByTestId("preview-tag")).toBeInTheDocument();
      expect(within(footer).getByTestId("answer-retry")).toBeInTheDocument();
    }
  });

  it("renders the completion header with duration and steps", async () => {
    const { user, box } = await openChat("chat-tools");
    await user.type(box, "go{Enter}");
    const header = await screen.findByTestId("completion-header");
    expect(header.textContent).toMatch(/^Completed in (\d+s|1m \d+s) · 3 steps$/);
    expect(header.textContent).not.toMatch(/\d{3,}m/);
  });

  it("formats the footer: tokens incl. cache, preview tag, retry, timestamp", async () => {
    const { user, box } = await openChat("empty");
    await user.type(box, "one{Enter}");
    const footer = await screen.findByTestId("answer-footer");
    expect(within(footer).getByTestId("answer-usage").textContent).toBe("Usage 23.6K tok");
    expect(within(footer).getByTestId("answer-retry")).toBeInTheDocument();
    expect(within(footer).getByTestId("answer-meta").textContent).toMatch(/\d{2}\/\d{2} \d{2}:\d{2}$/);
  });
});

describe("telemetry strip", () => {
  it("renders turns/steps/tok-s, tok + cache hit and the context ring", () => {
    render(
      <>
        <UsageStats usage={usage({ steps: 7, tokens_per_second: 42.4 })} />
        <ContextMeter usage={usage()} />
      </>,
    );
    expect(screen.getByTestId("usage-pill-turns")).toHaveTextContent(
      "3 turns 7 steps · 42 tok/s",
    );
    expect(screen.getByTestId("usage-pill-tokens")).toHaveTextContent(
      "23.6K tok · Cache hit 40%",
    );
    expect(screen.getByTestId("context-meter")).toHaveTextContent("18%");
    expect(screen.getByTestId("context-meter").querySelector("svg circle")).not.toBeNull();
  });

  it("omits steps, tok/s and cache hit when unavailable", () => {
    render(<UsageStats usage={usage({ cache_read_tokens: null })} />);
    expect(screen.getByTestId("usage-pill-turns").textContent).toBe("3 turns");
    expect(screen.getByTestId("usage-pill-tokens").textContent).toBe("15.2K tok");
  });

  it("formats compact counts and the cache hit ratio", () => {
    expect(formatCompactTokens(10_800)).toBe("10.8K");
    expect(formatCompactTokens(3_600_000)).toBe("3.6M");
    expect(cacheHitPercent(usage())).toBe(40);
    expect(cacheHitPercent(usage({ cache_read_tokens: 0 }))).toBeNull();
  });
});
