import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ConfirmDialog from "./ConfirmDialog";
import { messages } from "../messages";

const baseProps = {
  title: messages.conversations.deleteConfirmTitle,
  message: messages.conversations.deleteConfirmBody,
  confirmPrompt: messages.common.confirmPrompt,
  onCancel: vi.fn(),
  onConfirm: vi.fn(),
};

describe("ConfirmDialog", () => {
  it("requires an affirmative sí to enable the delete button", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(<ConfirmDialog {...baseProps} onConfirm={onConfirm} />);
    const input = screen.getByLabelText(messages.common.confirmNameLabel);
    const confirm = screen.getByRole("button", { name: messages.common.delete });
    expect(confirm).toBeDisabled();
    await user.type(input, "No");
    expect(confirm).toBeDisabled();
    await user.clear(input);
    await user.type(input, "sí");
    expect(confirm).toBeEnabled();
    await user.click(confirm);
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it.each(["Sí", "sí", "SI", "si"])("accepts %s as a valid confirmation", async (affirmative) => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(<ConfirmDialog {...baseProps} onConfirm={onConfirm} />);
    const input = screen.getByLabelText(messages.common.confirmNameLabel);
    const confirm = screen.getByRole("button", { name: messages.common.delete });
    await user.type(input, affirmative);
    expect(confirm).toBeEnabled();
    await user.click(confirm);
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("trims surrounding whitespace before matching", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(<ConfirmDialog {...baseProps} onConfirm={onConfirm} />);
    const input = screen.getByLabelText(messages.common.confirmNameLabel);
    const confirm = screen.getByRole("button", { name: messages.common.delete });
    await user.type(input, "  sí  ");
    expect(confirm).toBeEnabled();
    await user.click(confirm);
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it.each(["   ", "s i", "siii", "no"])("does not enable deletion for %j", async (text) => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(<ConfirmDialog {...baseProps} onConfirm={onConfirm} />);
    const input = screen.getByLabelText(messages.common.confirmNameLabel);
    const confirm = screen.getByRole("button", { name: messages.common.delete });
    await user.type(input, text);
    expect(confirm).toBeDisabled();
  });

  it("does not require the conversation title", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(<ConfirmDialog {...baseProps} onConfirm={onConfirm} />);
    const input = screen.getByLabelText(messages.common.confirmNameLabel);
    const confirm = screen.getByRole("button", { name: messages.common.delete });
    await user.type(input, "Fotosíntesis");
    expect(confirm).toBeDisabled();
  });

  it("does not delete when the confirmation is invalid even via Enter", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(<ConfirmDialog {...baseProps} onConfirm={onConfirm} />);
    const input = screen.getByLabelText(messages.common.confirmNameLabel);
    await user.type(input, "borrar");
    await user.keyboard("{Enter}");
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("keeps exact-name matching when confirmText is provided (project flow)", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(<ConfirmDialog {...baseProps} confirmText="Fotosíntesis" onConfirm={onConfirm} />);
    const input = screen.getByLabelText(messages.common.confirmNameLabel);
    const confirm = screen.getByRole("button", { name: messages.common.delete });
    await user.type(input, "fotosintesis");
    expect(confirm).toBeDisabled();
    await user.clear(input);
    await user.type(input, "Fotosíntesis");
    expect(confirm).toBeEnabled();
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
