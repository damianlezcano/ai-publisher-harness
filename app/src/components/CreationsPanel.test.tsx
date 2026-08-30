import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import CreationsPanel from "./CreationsPanel";

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
  },
];

beforeEach(() => {
  invokeMock.mockReset();
});

describe("CreationsPanel", () => {
  it("shows human-readable kind and visibility", () => {
    render(<CreationsPanel projectId={projectId} creations={creations} onRefresh={() => {}} />);
    expect(screen.getByText("actividad")).toBeInTheDocument();
    expect(screen.getByText(/Actividad interactiva/)).toBeInTheDocument();
    expect(screen.getByText(/Privado/)).toBeInTheDocument();
  });

  it("opens a creation through the safe command", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    render(<CreationsPanel projectId={projectId} creations={creations} onRefresh={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Abrir" }));
    expect(invokeMock).toHaveBeenCalledWith("creation_open", {
      projectId,
      creationId: creations[0].id,
    });
  });

  it("toggles visibility to share a creation", async () => {
    invokeMock.mockResolvedValueOnce({ ...creations[0], visibility: "public" });
    render(<CreationsPanel projectId={projectId} creations={creations} onRefresh={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Se compartirá" }));
    expect(invokeMock).toHaveBeenCalledWith("creation_set_visibility", {
      projectId,
      creationId: creations[0].id,
      public: true,
    });
  });

  it("shows a human error when open fails", async () => {
    invokeMock.mockRejectedValueOnce({
      code: "open_failed",
      message: "No pudimos abrir ese recurso.",
    });
    render(<CreationsPanel projectId={projectId} creations={creations} onRefresh={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Abrir" }));
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent("No pudimos abrir ese recurso."),
    );
  });
});
