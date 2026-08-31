import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import PreviewModal from "./PreviewModal";

describe("PreviewModal", () => {
  const onClose = vi.fn();
  const trigger = document.createElement("button");
  trigger.textContent = "Abrir vista previa";

  beforeEach(() => {
    onClose.mockReset();
    document.body.innerHTML = "";
    document.body.appendChild(trigger);
    trigger.focus();
  });

  it("renders a dialog labelled by the creation title", () => {
    render(
      <PreviewModal
        title="Notas"
        preview={{ contentType: "text/plain", dataBase64: btoa("hola") }}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByRole("dialog", { name: "Notas" })).toBeInTheDocument();
  });

  it("moves focus into the modal and closes on Escape", () => {
    const { unmount } = render(
      <PreviewModal
        title="Notas"
        preview={{ contentType: "text/plain", dataBase64: btoa("hola") }}
        onClose={onClose}
      />,
    );
    const closeButton = screen.getByRole("button", { name: "Cerrar" });
    expect(closeButton).toHaveFocus();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
    unmount();
    expect(trigger).toHaveFocus();
  });

  it("escapes text content without rendering HTML", () => {
    render(
      <PreviewModal
        title="Notas"
        preview={{ contentType: "text/markdown", dataBase64: btoa("<script>alert(1)</script>") }}
        onClose={onClose}
      />,
    );
    const pre = screen.getByText("<script>alert(1)</script>");
    expect(pre.tagName).toBe("PRE");
    expect(document.querySelector("script")).toBeNull();
  });

  it("closes with the visible Cerrar button and returns focus", async () => {
    const { unmount } = render(
      <PreviewModal
        title="Foto"
        preview={{ contentType: "image/png", dataBase64: "abc" }}
        onClose={onClose}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Cerrar" }));
    expect(onClose).toHaveBeenCalled();
    unmount();
    expect(trigger).toHaveFocus();
  });
});
