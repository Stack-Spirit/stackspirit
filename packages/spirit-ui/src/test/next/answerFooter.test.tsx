import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { SpiritBridge } from "../../bridge/SpiritBridge";
import { MockBridge } from "../../bridge/MockBridge";
import { resetConversations } from "../../next/conversations";
import { NextShell } from "../../next/NextShell";
import { SpiritLoader, TurnLoader } from "../../next/SpiritLoader";
import { resetShellUi } from "../../next/shellUi";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

const bridges: SpiritBridge[] = [];

afterEach(() => {
  for (const bridge of bridges.splice(0)) bridge.dispose?.();
  resetConversations();
  resetShellUi();
  resetFleetStore();
});

describe("assistant turn layout", () => {
  it("has no StackSpirit sender header and ends with an answer footer", async () => {
    resetFleetStore();
    resetConversations();
    const bridge = new MockBridge({ scenario: "chat-tools", hosts: [] });
    bridges.push(bridge);
    const user = userEvent.setup();
    render(<NextShell bridge={bridge} />);
    await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));
    await user.click(screen.getByRole("button", { name: /Set up with AI/ }));
    await user.type(await screen.findByRole("textbox", { name: "Message" }), "go{Enter}");

    const footer = await screen.findByTestId("answer-footer");
    const message = screen.getByTestId("assistant-message");
    expect(message.querySelector(".nx-msg__who")).toBeNull();
    expect(within(message).queryByText("StackSpirit")).toBeNull();
    expect(within(footer).getByRole("button", { name: "Copy answer" })).toBeInTheDocument();
    expect(within(footer).getByTestId("answer-meta")).toBeInTheDocument();
    expect(footer.compareDocumentPosition(message.querySelector("p")!)).toBe(
      Node.DOCUMENT_POSITION_PRECEDING,
    );
  });
});

describe("SpiritLoader", () => {
  it("renders the animated sprout character with status semantics", () => {
    render(<SpiritLoader label="StackSpirit is replying" />);
    const status = screen.getByRole("status");
    expect(status).toHaveTextContent("StackSpirit is replying");
    const svg = status.querySelector("svg")!;
    expect(svg).toHaveAttribute("aria-hidden", "true");
    expect(svg.querySelector(".nx-loader__sprout")).not.toBeNull();
    expect(svg.querySelectorAll(".nx-loader__leaf")).toHaveLength(2);
    expect(svg.querySelectorAll(".nx-loader__eye")).toHaveLength(2);
    expect(svg.querySelectorAll("path").length).toBeGreaterThan(10);
  });
});

describe("TurnLoader", () => {
  it("shows rotating Meadow dots and visible, ticking Thinking text", () => {
    vi.useFakeTimers();
    try {
      render(<TurnLoader />);
      const status = screen.getByTestId("turn-loader");
      expect(status).toHaveAttribute("role", "status");
      expect(screen.getByTestId("turn-loader-text")).toHaveTextContent("Thinking…");
      const dots = screen.getByTestId("turn-loader-dots");
      expect(dots).toHaveAttribute("width", "16");
      const fills = [...dots.querySelectorAll("circle")].map((c) => c.getAttribute("fill"));
      expect(fills).toEqual(["#2f8547", "#52b86d", "#78c462"]);
      expect(dots.querySelector(".nx-turn-loader__orbit")).not.toBeNull();
      act(() => {
        vi.advanceTimersByTime(3000);
      });
      expect(screen.getByTestId("turn-loader-text")).toHaveTextContent("Thinking for 3s ···");
    } finally {
      vi.useRealTimers();
    }
  });
});
