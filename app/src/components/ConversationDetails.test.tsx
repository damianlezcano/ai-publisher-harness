import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import ConversationDetails from "./ConversationDetails";
import type { ProjectView } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const invokeMock = vi.mocked(invoke);

const project: ProjectView = {
  id: "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22",
  name: "Fotosíntesis",
  materials: [
    {
      id: "m1",
      displayName: "manual.pdf",
      originalFileName: "manual.pdf",
      kind: "pdf",
      byteSize: 10,
      createdAt: "2026-08-28T15:00:00Z",
    },
  ],
  creations: [
    {
      id: "c1",
      displayName: "Actividad",
      kind: "web",
      visibility: "private",
      byteSize: 20,
      createdAt: "2026-08-28T15:00:00Z",
      revision: 1,
    },
  ],
  messages: [],
  publication: { state: "local", publicUrl: null },
};

const model = {
  providerId: "opencode",
  modelId: "big-pickle",
  name: "Big Pickle",
  free: true,
  recommended: true,
  deprecated: false,
};

function setup() {
  invokeMock.mockImplementation((command: string) => {
    if (command === "model_list") return Promise.resolve([model]);
    return Promise.resolve(undefined);
  });
}

beforeEach(() => {
  invokeMock.mockReset();
  setup();
});

describe("ConversationDetails", () => {
  it("shows name, model/default, files, renames, selects, clears, opens folders, and closes", async () => {
    const onRefresh = vi.fn();
    const onClose = vi.fn();
    const user = userEvent.setup();
    render(<ConversationDetails project={project} active={false} onClose={onClose} onRefresh={onRefresh} />);

    expect(screen.getByLabelText("Nombre")).toHaveValue("Fotosíntesis");
    expect(await screen.findByRole("option", { name: /Big Pickle/ })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Predeterminado de Configuración" })).toBeInTheDocument();
    expect(screen.getByText("manual.pdf")).toBeInTheDocument();
    expect(screen.getByText("Actividad interactiva")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Nombre"), { target: { value: "Nueva" } });
    await user.click(screen.getByRole("button", { name: "Renombrar" }));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("project_rename", { projectId: project.id, name: "Nueva" }));
    expect(onRefresh).toHaveBeenCalled();

    await user.selectOptions(screen.getByLabelText("Modelo de esta conversación"), "opencode::big-pickle");
    expect(invokeMock).toHaveBeenCalledWith("conversation_model_select", {
      projectId: project.id,
      providerId: "opencode",
      modelId: "big-pickle",
    });
    await user.selectOptions(screen.getByLabelText("Modelo de esta conversación"), "");
    expect(invokeMock).toHaveBeenCalledWith("conversation_model_clear", { projectId: project.id });

    const folders = screen.getAllByRole("button", { name: "Abrir carpeta contenedora" });
    await user.click(folders[0]);
    await user.click(folders[1]);
    expect(invokeMock).toHaveBeenCalledWith("material_open_folder", { projectId: project.id, materialId: "m1" });
    expect(invokeMock).toHaveBeenCalledWith("creation_open_folder", { projectId: project.id, creationId: "c1" });
    await user.click(screen.getByRole("button", { name: "Cerrar" }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("disables model changes during an active turn", async () => {
    render(<ConversationDetails project={{ ...project, model: { providerId: "opencode", modelId: "big-pickle" } }} active onClose={() => {}} onRefresh={() => {}} />);
    expect(await screen.findByLabelText("Modelo de esta conversación")).toBeDisabled();
    expect(screen.getByText("Esperá a que termine la solicitud antes de cambiar el modelo.")).toBeInTheDocument();
  });
});
