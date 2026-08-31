import { useRef, useState } from "react";
import type { RefObject } from "react";
import { messages } from "../messages";
import Dialog from "./ui/Dialog";

interface ConfirmDialogProps {
  title: string;
  message: string;
  confirmText: string;
  busy?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export default function ConfirmDialog({
  title,
  message,
  confirmText,
  busy,
  onCancel,
  onConfirm,
}: ConfirmDialogProps) {
  const [value, setValue] = useState("");
  const ready = value === confirmText;
  const inputRef = useRef<HTMLInputElement>(null);

  return (
    <Dialog title={title} onClose={onCancel} initialFocusRef={inputRef as RefObject<HTMLElement>}>
      <p>{message}</p>
      <label className="sr-only" htmlFor="confirm-input">
        {messages.common.confirmNameLabel}
      </label>
      <input
        ref={inputRef}
        id="confirm-input"
        value={value}
        onChange={(e) => setValue(e.target.value)}
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
