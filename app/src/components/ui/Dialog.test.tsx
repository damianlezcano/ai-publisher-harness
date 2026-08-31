import { useRef, useState } from "react";
import type { RefObject } from "react";
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Dialog from "./Dialog";

function DialogHarness({ initialFocusRef }: { initialFocusRef?: RefObject<HTMLElement> }) {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  return (
    <>
      <button ref={triggerRef} type="button" onClick={() => setOpen(true)}>
        Abrir diálogo
      </button>
      {open && (
        <Dialog
          title="Confirmar acción"
          onClose={() => setOpen(false)}
          initialFocusRef={initialFocusRef}
        >
          <p>Contenido del diálogo</p>
          <div className="dialog-actions">
            <button type="button">Guardar</button>
            <button type="button" className="secondary">
              Cancelar
            </button>
          </div>
        </Dialog>
      )}
    </>
  );
}

describe("Dialog", () => {
  it("renders a labelled modal dialog with children", () => {
    render(
      <Dialog title="Confirmar" onClose={vi.fn()}>
        Contenido
      </Dialog>,
    );
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveAttribute("aria-modal", "true");
    const labelledBy = dialog.getAttribute("aria-labelledby");
    expect(labelledBy).toBeTruthy();
    expect(screen.getByText("Confirmar")).toHaveAttribute("id", labelledBy);
    expect(screen.getByText("Contenido")).toBeInTheDocument();
  });

  it("calls onClose on Escape", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(
      <Dialog title="Confirmar" onClose={onClose}>
        <button type="button">Guardar</button>
      </Dialog>,
    );
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("traps focus: Tab wraps forward and Shift+Tab wraps backward", async () => {
    const user = userEvent.setup();
    render(
      <Dialog title="Confirmar" onClose={vi.fn()}>
        <div className="dialog-actions">
          <button type="button">Primero</button>
          <button type="button" className="secondary">
            Segundo
          </button>
        </div>
      </Dialog>,
    );
    const first = screen.getByRole("button", { name: "Primero" });
    const second = screen.getByRole("button", { name: "Segundo" });
    expect(first).toHaveFocus();
    await user.tab();
    expect(second).toHaveFocus();
    await user.tab();
    expect(first).toHaveFocus();
    await user.tab({ shift: true });
    expect(second).toHaveFocus();
  });

  it("focuses the initialFocusRef on open and restores focus to the trigger on close", async () => {
    const user = userEvent.setup();
    function Harness() {
      const initialFocusRef = useRef<HTMLButtonElement>(null);
      return <DialogHarness initialFocusRef={initialFocusRef as RefObject<HTMLElement>} />;
    }
    render(<Harness />);
    const trigger = screen.getByRole("button", { name: "Abrir diálogo" });
    await user.click(trigger);
    const save = screen.getByRole("button", { name: "Guardar" });
    expect(save).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });

  it("applies an extra className for sizing", () => {
    render(
      <Dialog title="Confirmar" onClose={vi.fn()} className="provider-dialog">
        <button type="button">Guardar</button>
      </Dialog>,
    );
    expect(screen.getByRole("dialog")).toHaveClass("dialog provider-dialog");
  });
});
