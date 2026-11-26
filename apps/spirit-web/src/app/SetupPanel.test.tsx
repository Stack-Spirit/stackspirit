import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { SetupPanel } from "./SetupPanel";

describe("Web setup stays account-free", () => {
  it("puts the local daemon form first and keeps the cloud link optional", () => {
    render(<SetupPanel error={null} />);
    expect(
      screen.getByRole("heading", { name: "Connect to the StackSpirit daemon" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Use this address" })).toBeInTheDocument();
    expect(screen.getByPlaceholderText(/the dev-server origin/i)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Always-On Homes" })).toHaveAttribute(
      "href",
      "/cloud",
    );
    expect(screen.getByText(/Local and SSH server mode needs no account/i)).toBeInTheDocument();
    expect(document.querySelector('input[type="password"]')).toBeNull();
    expect(screen.queryByRole("button", { name: /Sign in to StackSpirit/i })).not.toBeInTheDocument();
  });
});
