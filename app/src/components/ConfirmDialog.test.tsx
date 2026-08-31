import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ConfirmDialog from "./ConfirmDialog";
import { messages } from "../messages";

const baseProps = {
  title: "Eliminar proyecto",
  message: messages.project.delete.confirmMessage("Fotosíntesis"),
  confirmText: "Fotosíntesis",
  onCancel: vi.fn(),
  onConfirm: vi.fn(),
};

describe("ConfirmDialog", () => {
  it("requires the exact name to enable the delete button", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(<ConfirmDialog {...baseProps} onConfirm={onConfirm} />);
    const input = screen.getByLabelText(messages.common.confirmNameLabel);
    const confirm = screen.getByRole("button", { name: messages.common.delete });
    expect(confirm).toBeDisabled();
    await user.type(input, "Fotos");
    expect(confirm).toBeDisabled();
    await user.type(input, "íntesis");
    expect(confirm).toBeEnabled();
    await user.click(confirm);
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("closes on Escape", async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();
    render(<ConfirmDialog {...baseProps} onCancel={onCancel} />);
    await user.keyboard("{Escape}");
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("returns focus to the trigger on close", () => {
    const trigger = document.createElement("button");
    document.body.appendChild(trigger);
    trigger.focus();
    const { unmount } = render(<ConfirmDialog {...baseProps} />);
    unmount();
    expect(trigger).toHaveFocus();
  });
});
