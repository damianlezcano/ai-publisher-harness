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

  it("shows metadata plus an external-open action instead of rendering binary as text", () => {
    render(
      <PreviewModal
        title="notas.pdf"
        preview={{ contentType: "application/pdf", dataBase64: btoa("%PDF-1.7") }}
        meta={{ name: "notas.pdf", byteSize: 8, kind: "pdf" }}
        onClose={vi.fn()}
        onOpenExternal={vi.fn()}
      />,
    );
    expect(screen.getByText("Documento PDF · 8 B")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Abrir con la aplicación" })).toBeInTheDocument();
    expect(screen.getByText(/No podemos previsualizar/)).toBeInTheDocument();
    expect(screen.queryByText("%PDF-1.7")).not.toBeInTheDocument();
  });

  it("sniffs a real PNG signature even when the declared type is generic", () => {
    const png = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3]);
    render(
      <PreviewModal
        title="foto"
        preview={{
          contentType: "application/octet-stream",
          dataBase64: btoa(String.fromCharCode(...png)),
        }}
        onClose={vi.fn()}
      />,
    );
    expect(document.querySelector("img")).toBeInTheDocument();
    expect(document.querySelector("pre")).toBeNull();
  });

  it("does not render raw HTML; the source is escaped as text", () => {
    render(
      <PreviewModal
        title="pagina.html"
        preview={{ contentType: "text/html", dataBase64: btoa("<script>alert(1)</script>") }}
        onClose={vi.fn()}
      />,
    );
    const pre = screen.getByText("<script>alert(1)</script>");
    expect(pre.tagName).toBe("PRE");
    expect(document.querySelector("script")).toBeNull();
  });
});
