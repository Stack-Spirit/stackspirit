import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Button } from "./Button";

describe("Button", () => {
  it("stays disabled while loading even when the caller passes disabled={false}", () => {
    render(
      <Button loading disabled={false}>
        Save host
      </Button>,
    );
    const button = screen.getByRole("button", { name: "Save host" });
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("aria-busy", "true");
  });
});
