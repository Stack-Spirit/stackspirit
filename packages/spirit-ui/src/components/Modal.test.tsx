import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";

import { Button, Modal } from "../components";

function Harness() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button onClick={() => setOpen(true)}>Open dialog</Button>
      <Modal
        open={open}
        title="Test dialog"
        onClose={() => setOpen(false)}
        footer={<Button onClick={() => setOpen(false)}>Done</Button>}
      >
        <p>Dialog body</p>
      </Modal>
    </>
  );
}

describe("Modal", () => {
  it("closes on Escape and restores focus to the opener", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    const opener = screen.getByRole("button", { name: "Open dialog" });
    await user.click(opener);
    expect(screen.getByRole("dialog", { name: "Test dialog" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Test dialog" })).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
  });
});
