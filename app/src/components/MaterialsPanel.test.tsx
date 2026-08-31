import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import MaterialsPanel from "./MaterialsPanel";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);
const openDialogMock = vi.mocked(openDialog);
const getCurrentWebviewMock = vi.mocked(getCurrentWebview);

const projectId = "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22";
const onRefresh = vi.fn();

const material = {
  id: "m1",
  displayName: "manual.pdf",
  originalFileName: "manual.pdf",
  kind: "pdf",
  byteSize: 9,
  createdAt: "2026-08-28T15:00:00Z",
};

function mockDragDrop(): {
  dropHandler: (event: { payload: { type: string; paths: string[] } }) => void;
} {
  let dropHandler: ((event: { payload: { type: string; paths: string[] } }) => void) | undefined;
  getCurrentWebviewMock.mockReturnValue({
    onDragDropEvent: vi.fn().mockImplementation((handler) => {
      dropHandler = handler;
      return Promise.resolve(() => {});
    }),
  } as never);
  return {
    dropHandler: (event) => dropHandler?.(event),
  };
}

beforeEach(() => {
  invokeMock.mockReset();
  openDialogMock.mockReset();
  onRefresh.mockReset();
  getCurrentWebviewMock.mockReset();
  getCurrentWebviewMock.mockReturnValue({
    onDragDropEvent: vi.fn().mockResolvedValue(() => {}),
  } as never);
});

describe("MaterialsPanel", () => {
  it("adds a file via the picker", async () => {
    openDialogMock.mockResolvedValueOnce("/tmp/manual.pdf");
    invokeMock.mockResolvedValueOnce({ ...material });
    render(<MaterialsPanel projectId={projectId} materials={[]} onRefresh={onRefresh} />);
    await userEvent.click(screen.getByRole("button", { name: "Agregar archivo" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("material_add_from_path", {
        projectId,
        path: "/tmp/manual.pdf",
      }),
    );
    expect(onRefresh).toHaveBeenCalled();
  });

  it("renders an empty state with a picker action when there are no materials", async () => {
    render(<MaterialsPanel projectId={projectId} materials={[]} onRefresh={onRefresh} />);
    expect(screen.getByText("Agregá material para darle contexto a la IA")).toBeInTheDocument();
    expect(screen.getByText("o pegá una imagen con Ctrl+V")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Agregar archivo" }));
    expect(openDialogMock).toHaveBeenCalled();
  });

  it("shows a human error when the file is rejected", async () => {
    openDialogMock.mockResolvedValueOnce("/tmp/bad.pdf");
    invokeMock.mockRejectedValueOnce({
      code: "material_failed",
      message: "No pudimos agregar ese archivo.",
    });
    render(<MaterialsPanel projectId={projectId} materials={[]} onRefresh={onRefresh} />);
    await userEvent.click(screen.getByRole("button", { name: "Agregar archivo" }));
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent("No pudimos agregar ese archivo."),
    );
  });

  it("lists materials by name without ids", () => {
    render(<MaterialsPanel projectId={projectId} materials={[material]} onRefresh={onRefresh} />);
    expect(screen.getByText("manual.pdf")).toBeInTheDocument();
    expect(screen.queryByText("m1")).not.toBeInTheDocument();
  });

  it("shows inline remove confirmation with the material name", async () => {
    render(<MaterialsPanel projectId={projectId} materials={[material]} onRefresh={onRefresh} />);
    await userEvent.click(screen.getByRole("button", { name: "Quitar" }));
    expect(screen.getByRole("group", { name: "Confirmar eliminación" })).toHaveTextContent(
      "manual.pdf",
    );
    expect(screen.getByRole("button", { name: "Cancelar" })).toBeInTheDocument();
  });

  it("removes the material when the inline confirm is accepted", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    render(<MaterialsPanel projectId={projectId} materials={[material]} onRefresh={onRefresh} />);
    await userEvent.click(screen.getByRole("button", { name: "Quitar" }));
    await userEvent.click(screen.getByRole("button", { name: "Quitar" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("material_remove", {
        projectId,
        materialId: "m1",
      }),
    );
    expect(onRefresh).toHaveBeenCalled();
  });

  it("renders a batch import summary with per-file details", async () => {
    const { dropHandler } = mockDragDrop();
    invokeMock.mockResolvedValueOnce({
      items: [
        { sourceName: "a.pdf", status: "added", materialId: "m1" },
        { sourceName: "b.png", status: "added", materialId: "m2" },
        { sourceName: "c.docx", status: "added", materialId: "m3" },
        { sourceName: "dup.pdf", status: "duplicate", materialId: "m1" },
        { sourceName: "bad.exe", status: "unsupported", reason: "No admitido" },
      ],
    });

    render(<MaterialsPanel projectId={projectId} materials={[]} onRefresh={onRefresh} />);
    dropHandler({
      payload: {
        type: "drop",
        paths: ["/tmp/a.pdf", "/tmp/b.png", "/tmp/c.docx", "/tmp/dup.pdf", "/tmp/bad.exe"],
      },
    });

    await waitFor(() =>
      expect(
        screen.getByText("3 agregados · 1 ya estaba · 1 no se pudo agregar"),
      ).toBeInTheDocument(),
    );
    expect(screen.getByText("Se agregó a.pdf.")).toBeInTheDocument();
    expect(screen.getByText("Se agregó b.png.")).toBeInTheDocument();
    expect(screen.getByText("Se agregó c.docx.")).toBeInTheDocument();
    expect(screen.getByText("dup.pdf ya estaba en el proyecto.")).toBeInTheDocument();
    expect(screen.getByText("No se pudo agregar bad.exe.")).toBeInTheDocument();
    expect(onRefresh).toHaveBeenCalled();
  });

  it("shows a busy state with a spinner while importing", async () => {
    const { dropHandler } = mockDragDrop();
    let resolveImport: (value: unknown) => void;
    invokeMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveImport = resolve;
        }),
    );

    render(<MaterialsPanel projectId={projectId} materials={[]} onRefresh={onRefresh} />);
    dropHandler({ payload: { type: "drop", paths: ["/tmp/ok.pdf"] } });

    await waitFor(() => expect(screen.getByText("Agregando archivos…")).toBeInTheDocument());
    const status = screen.getByRole("status");
    expect(status.querySelector(".spinner")).not.toBeNull();
    expect(status.querySelector(".spinner")).toHaveAttribute("aria-hidden", "true");

    resolveImport!({ items: [{ sourceName: "ok.pdf", status: "added", materialId: "m2" }] });
    await waitFor(() => expect(screen.queryByText("Agregando archivos…")).not.toBeInTheDocument());
  });
});
