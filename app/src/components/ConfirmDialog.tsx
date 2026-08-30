import { useState } from "react";

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

  return (
    <div className="dialog-backdrop" role="presentation">
      <div className="dialog" role="dialog" aria-modal="true" aria-labelledby="confirm-title">
        <h2 id="confirm-title">{title}</h2>
        <p>{message}</p>
        <label className="sr-only" htmlFor="confirm-input">
          Nombre para confirmar
        </label>
        <input
          id="confirm-input"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          autoFocus
        />
        <div className="dialog-actions">
          <button type="button" className="secondary" onClick={onCancel}>
            Cancelar
          </button>
          <button type="button" className="danger" disabled={!ready || busy} onClick={onConfirm}>
            Eliminar
          </button>
        </div>
      </div>
    </div>
  );
}
