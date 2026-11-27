import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { SetupPanel } from "./SetupPanel";

describe("Web landing is account-first", () => {
  it("shows the StackSpirit Account card before the collapsed local daemon option", () => {
    render(<SetupPanel error={null} />);

    expect(screen.getByTestId("web-auth-panel")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "StackSpirit Account" })).toBeInTheDocument();
    expect(screen.getByTestId("auth-google")).toHaveAttribute(
      "href",
      "https://api.stackspirit.io/v1/auth/google/start",
    );
    expect(screen.getByTestId("auth-github")).toHaveAttribute(
      "href",
      "https://api.stackspirit.io/v1/auth/github/start",
    );
    expect(screen.getByLabelText("Email")).toBeInTheDocument();
    expect(screen.getByLabelText("Password")).toBeInTheDocument();

    expect(
      screen.getByRole("button", { name: /Or connect to a local daemon without an account/i }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Connect to the StackSpirit daemon" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText(/VITE_VOWP_DAEMON_URL/)).not.toBeInTheDocument();
  });

  it("reveals the daemon form and its technical detail only after the disclosure", async () => {
    const user = userEvent.setup();
    render(<SetupPanel error={null} />);

    await user.click(
      screen.getByRole("button", { name: /Or connect to a local daemon without an account/i }),
    );

    expect(
      screen.getByRole("heading", { name: "Connect to the StackSpirit daemon" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Use this address" })).toBeInTheDocument();
    expect(screen.getByPlaceholderText(/the dev-server origin/i)).toBeInTheDocument();
    expect(screen.getByText(/VITE_VOWP_DAEMON_URL/)).toBeInTheDocument();
  });
});
