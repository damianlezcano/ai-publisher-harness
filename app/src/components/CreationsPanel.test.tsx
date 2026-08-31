import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import CreationsPanel from "./CreationsPanel";
import { messages } from "../messages";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

const projectId = "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22";
const creations = [
  {
    id: "0198e4a6-86d6-7c16-b4c4-3197b355cf10",
    displayName: "actividad",
    kind: "web",
    visibility: "private" as const,
    byteSize: 1024,
    createdAt: "2026-08-28T15:00:00Z",
    revision: 1,
  },
];

beforeEach(() => {
  invokeMock.mockReset();
});

describe("CreationsPanel", () => {
  it("shows human-readable kind and the open action", () => {
    render(<CreationsPanel projectId={projectId} creations={creations} onRefresh={() => {}} />);
    expect(screen.getByText("actividad")).toBeInTheDocument();
    expect(screen.getByText(/Actividad interactiva/)).toBeInTheDocument();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  });

  it("renders the empty state with title and hint", () => {
    render(<CreationsPanel projectId={projectId} creations={[]} onRefresh={() => {}} />);
    expect(screen.getByText(messages.creation.empty.title)).toHaveClass("empty-state-title");
    expect(screen.getByText(messages.creation.empty.hint)).toHaveClass("empty-state-body");
  });

  it("opens a creation through the safe command", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    render(<CreationsPanel projectId={projectId} creations={creations} onRefresh={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Abrir en navegador" }));
    expect(invokeMock).toHaveBeenCalledWith("creation_open", {
      projectId,
      creationId: creations[0].id,
    });
  });

  it("opens web preview in an isolated window", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    render(<CreationsPanel projectId={projectId} creations={creations} onRefresh={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Vista previa" }));
    expect(invokeMock).toHaveBeenCalledWith("preview_open_web", {
      projectId,
      creationId: creations[0].id,
    });
  });

  it("shows Vista previa for file-kind text creations and calls preview_data with creation", async () => {
    const fileCreation = {
      id: "0198e4a6-86d6-7c16-b4c4-3197b355cf11",
      displayName: "notas.md",
      kind: "file",
      visibility: "private" as const,
      byteSize: 256,
      createdAt: "2026-08-28T15:00:00Z",
      revision: 1,
    };
    invokeMock.mockResolvedValueOnce({
      contentType: "text/markdown",
      dataBase64: btoa("# Hola"),
    });
    render(
      <CreationsPanel projectId={projectId} creations={[fileCreation]} onRefresh={() => {}} />,
    );
    expect(screen.getByRole("button", { name: "Vista previa" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Vista previa" }));
    expect(invokeMock).toHaveBeenCalledWith("preview_data", {
      projectId,
      resourceKind: "creation",
      resourceId: fileCreation.id,
    });
  });

  it("shows guided error notice when open fails", async () => {
    invokeMock.mockRejectedValueOnce({
      code: "open_failed",
      message: "No pudimos abrir ese recurso.",
    });
    render(<CreationsPanel projectId={projectId} creations={creations} onRefresh={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Abrir en navegador" }));
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(messages.error.openFailed.title),
    );
  });
});
