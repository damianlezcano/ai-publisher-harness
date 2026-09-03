import { useRef, useState } from "react";
import type { RefObject } from "react";
import { messages } from "../messages";
import Dialog from "./ui/Dialog";

interface ConfirmDialogProps {
  title: string;
  message: string;
  confirmPrompt?: string;
  confirmText?: string;
  busy?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

function normalizeConfirmation(text: string): string {
  return text
    .trim()
    .toLowerCase()
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "");
}

export default function ConfirmDialog({
  title,
  message,
  confirmPrompt,
  confirmText,
  busy,
  onCancel,
  onConfirm,
}: ConfirmDialogProps) {
  const [value, setValue] = useState("");
  const ready =
    confirmText !== undefined
      ? value === confirmText
      : normalizeConfirmation(value) === normalizeConfirmation(messages.common.confirmYes);
  const inputRef = useRef<HTMLInputElement>(null);

  return (
    <Dialog
      title={title}
      onClose={onCancel}
      initialFocusRef={inputRef as RefObject<HTMLElement>}
      className="confirm-dialog"
    >
      <p>{message}</p>
      {confirmPrompt && <p id="confirm-prompt">{confirmPrompt}</p>}
      <label className="sr-only" htmlFor="confirm-input">
        {messages.common.confirmNameLabel}
      </label>
      <input
        ref={inputRef}
        id="confirm-input"
        value={value}
        onChange={(e) => setValue(e.target.value)}
        aria-describedby={confirmPrompt ? "confirm-prompt" : undefined}
      />
      <div className="dialog-actions">
        <button type="button" className="secondary" onClick={onCancel}>
          {messages.common.cancel}
        </button>
        <button type="button" className="danger" disabled={!ready || busy} onClick={onConfirm}>
          {messages.common.delete}
        </button>
      </div>
    </Dialog>
  );
}
