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
    invokeMock.mockResolvedValueOnce({
      id: "m1",
      displayName: "manual.pdf",
      originalFileName: "manual.pdf",
      kind: "pdf",
      byteSize: 9,
      createdAt: "2026-08-28T15:00:00Z",
    });
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
    const materials = [
      {
        id: "m1",
        displayName: "manual.pdf",
        originalFileName: "manual.pdf",
        kind: "pdf",
        byteSize: 9,
        createdAt: "2026-08-28T15:00:00Z",
      },
    ];
    render(<MaterialsPanel projectId={projectId} materials={materials} onRefresh={onRefresh} />);
    expect(screen.getByText("manual.pdf")).toBeInTheDocument();
    expect(screen.queryByText("m1")).not.toBeInTheDocument();
  });

  it("shows inline remove confirmation with the material name", async () => {
    const materials = [
      {
        id: "m1",
        displayName: "manual.pdf",
        originalFileName: "manual.pdf",
        kind: "pdf",
        byteSize: 9,
        createdAt: "2026-08-28T15:00:00Z",
      },
    ];
    render(<MaterialsPanel projectId={projectId} materials={materials} onRefresh={onRefresh} />);
    await userEvent.click(screen.getByRole("button", { name: "Quitar" }));
    expect(screen.getByRole("group", { name: "Confirmar eliminación" })).toHaveTextContent(
      "manual.pdf",
    );
    expect(screen.getByRole("button", { name: "Cancelar" })).toBeInTheDocument();
  });

  it("renders import report notes for duplicates and failures", async () => {
    let dropHandler: ((event: { payload: { type: string; paths: string[] } }) => void) | undefined;
    getCurrentWebviewMock.mockReturnValue({
      onDragDropEvent: vi.fn().mockImplementation((handler) => {
        dropHandler = handler;
        return Promise.resolve(() => {});
      }),
    } as never);

    invokeMock.mockResolvedValueOnce({
      items: [
        {
          sourceName: "ok.pdf",
          status: "added",
          materialId: "m2",
        },
        {
          sourceName: "dup.pdf",
          status: "duplicate",
          materialId: "m1",
        },
        {
          sourceName: "bad.exe",
          status: "unsupported",
          reason: "No admitido",
        },
      ],
    });

    render(<MaterialsPanel projectId={projectId} materials={[]} onRefresh={onRefresh} />);
    dropHandler?.({
      payload: { type: "drop", paths: ["/tmp/ok.pdf", "/tmp/dup.pdf", "/tmp/bad.exe"] },
    });

    await waitFor(() =>
      expect(screen.getByText("Ese archivo ya está en el proyecto.")).toBeInTheDocument(),
    );
    expect(screen.getByRole("alert")).toHaveTextContent("No pudimos agregar algunos archivos.");
    expect(onRefresh).toHaveBeenCalled();
  });
});
