import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import ConversationDetails from "./ConversationDetails";
import type { ProjectView } from "../types";
import { humanDate, humanSize } from "../messages";

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
    if (command === "provider_list") return Promise.resolve([]);
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
    render(
      <ConversationDetails
        project={project}
        active={false}
        onClose={onClose}
        onRefresh={onRefresh}
      />,
    );

    expect(screen.getByLabelText("Nombre")).toHaveValue("Fotosíntesis");
    expect(await screen.findByRole("option", { name: /Big Pickle/ })).toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: "Predeterminado de Configuración" }),
    ).toBeInTheDocument();
    expect(screen.getByText("manual.pdf")).toBeInTheDocument();
    expect(screen.getByText("Actividad")).toBeInTheDocument();

    // Compact resource rows show a trustworthy date and the file size.
    const collapse = (text: string) => text.replace(/\s+/g, " ");
    const materialRow = screen.getByText("manual.pdf").closest("li");
    expect(materialRow).toHaveClass("item-row");
    const materialMeta = collapse(materialRow?.querySelector(".item-meta")?.textContent ?? "");
    expect(materialMeta).toContain(humanSize(project.materials[0].byteSize));
    expect(materialMeta).toContain(collapse(humanDate(project.materials[0].createdAt)));
    const creationRow = screen.getByText("Actividad").closest("li");
    expect(collapse(creationRow?.querySelector(".item-meta")?.textContent ?? "")).toContain(
      collapse(humanDate(project.creations[0].createdAt)),
    );
    expect(screen.getByText(/Actividad interactiva/)).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Nombre"), { target: { value: "Nueva" } });
    await user.click(screen.getByRole("button", { name: "Renombrar" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("project_rename", {
        projectId: project.id,
        name: "Nueva",
      }),
    );
    expect(onRefresh).toHaveBeenCalled();

    await user.selectOptions(
      screen.getByLabelText("Modelo de esta conversación"),
      "opencode::big-pickle",
    );
    expect(invokeMock).toHaveBeenCalledWith("conversation_model_select", {
      projectId: project.id,
      providerId: "opencode",
      modelId: "big-pickle",
    });
    await user.selectOptions(screen.getByLabelText("Modelo de esta conversación"), "");
    expect(invokeMock).toHaveBeenCalledWith("conversation_model_clear", { projectId: project.id });

    // One "Abrir carpeta contenedora" per section (materials and creations),
    // never repeated per individual file.
    const folderButtons = screen.getAllByRole("button", { name: "Abrir carpeta contenedora" });
    expect(folderButtons).toHaveLength(2);
    await user.click(folderButtons[0]);
    await user.click(folderButtons[1]);
    expect(invokeMock).toHaveBeenCalledWith("materials_open_folder", {
      projectId: project.id,
    });
    expect(invokeMock).toHaveBeenCalledWith("creations_open_folder", {
      projectId: project.id,
    });
    await user.click(screen.getByRole("button", { name: "Cerrar" }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("opens a text material in the in-app viewer with escaped text", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list") return Promise.resolve([model]);
      if (command === "provider_list") return Promise.resolve([]);
      if (command === "preview_data")
        return Promise.resolve({ contentType: "text/markdown", dataBase64: btoa("<b>Hola</b>") });
      return Promise.resolve(undefined);
    });
    const user = userEvent.setup();
    render(
      <ConversationDetails
        project={project}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Abrir: manual.pdf" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("preview_data", {
        projectId: project.id,
        resourceKind: "material",
        resourceId: "m1",
      }),
    );
    const dialog = await screen.findByRole("dialog", { name: "manual.pdf" });
    expect(dialog.querySelector("pre")).toBeInTheDocument();
    expect(dialog.querySelector("script")).toBeNull();
  });

  it("shows a PNG material as an image, never as binary text", async () => {
    const imageProject: ProjectView = {
      ...project,
      materials: [
        {
          id: "m1",
          displayName: "diagrama.png",
          originalFileName: "diagrama.png",
          kind: "image",
          byteSize: 42,
          createdAt: "2026-08-28T15:00:00Z",
        },
      ],
    };
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list") return Promise.resolve([model]);
      if (command === "provider_list") return Promise.resolve([]);
      if (command === "preview_data")
        return Promise.resolve({ contentType: "image/png", dataBase64: "ZmFrZQ==" });
      return Promise.resolve(undefined);
    });
    const user = userEvent.setup();
    render(
      <ConversationDetails
        project={imageProject}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Abrir: diagrama.png" }));
    const dialog = await screen.findByRole("dialog", { name: "diagrama.png" });
    expect(dialog.querySelector("img")).toBeInTheDocument();
    expect(dialog.querySelector("pre")).toBeNull();
  });

  it("disables model changes during an active turn", async () => {
    render(
      <ConversationDetails
        project={{ ...project, model: { providerId: "opencode", modelId: "big-pickle" } }}
        active
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    expect(await screen.findByLabelText("Modelo de esta conversación")).toBeDisabled();
    expect(
      screen.getByText("Esperá a que termine la solicitud antes de cambiar el modelo."),
    ).toBeInTheDocument();
  });
});
