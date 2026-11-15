import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { renderApp } from "../render";

describe("Sessions drawer entry point", () => {
  it("opens the read-only drawer from the canvas header and restores focus", async () => {
    const user = userEvent.setup();
    await renderApp("fresh");
    const trigger = screen.getByTestId("open-sessions");
    expect(trigger).toHaveAccessibleName("Sessions, 0");
    await user.click(trigger);

    const drawer = await screen.findByRole("dialog", { name: "Sessions" });
    expect(within(drawer).getByText("No session snapshot")).toBeInTheDocument();
    expect(within(drawer).getByRole("button", { name: "Import .asif.zip" })).toBeDisabled();
    expect(within(drawer).getByTestId("sessions-import-reason")).toHaveTextContent(
      "Connect to a VOWP home",
    );

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Sessions" })).not.toBeInTheDocument();
    expect(document.activeElement).toBe(trigger);
  });
});
