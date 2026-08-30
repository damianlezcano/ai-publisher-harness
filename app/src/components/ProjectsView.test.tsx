import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import ProjectsView from "./ProjectsView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

const projects = [
  { id: "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22", name: "Fotosíntesis" },
  { id: "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f23", name: "Sistema solar" },
];

beforeEach(() => {
  invokeMock.mockReset();
});

describe("ProjectsView", () => {
  it("lists project names and never shows ids", () => {
    render(<ProjectsView projects={projects} onRefresh={async () => {}} onOpen={() => {}} />);
    expect(screen.getByText("Fotosíntesis")).toBeInTheDocument();
    expect(screen.getByText("Sistema solar")).toBeInTheDocument();
    expect(screen.queryByText(projects[0].id)).not.toBeInTheDocument();
  });

  it("creates a project and opens it", async () => {
    const onOpen = vi.fn();
    invokeMock.mockResolvedValueOnce({ id: "new-id", name: "Nuevo" });
    render(<ProjectsView projects={[]} onRefresh={async () => {}} onOpen={onOpen} />);
    await userEvent.click(screen.getByRole("button", { name: "Nuevo proyecto" }));
    await userEvent.type(screen.getByLabelText("Nombre del proyecto"), "Nuevo");
    await userEvent.click(screen.getByRole("button", { name: "Crear" }));
    expect(invokeMock).toHaveBeenCalledWith("project_create", { name: "Nuevo" });
    await waitFor(() => expect(onOpen).toHaveBeenCalledWith("new-id"));
  });

  it("renames a project", async () => {
    invokeMock.mockResolvedValueOnce({ id: projects[0].id, name: "Fotos" });
    render(<ProjectsView projects={projects} onRefresh={async () => {}} onOpen={() => {}} />);
    await userEvent.click(screen.getAllByRole("button", { name: "Renombrar" })[0]);
    const input = screen.getByLabelText("Nuevo nombre");
    await userEvent.clear(input);
    await userEvent.type(input, "Fotos");
    await userEvent.click(screen.getByRole("button", { name: "Guardar" }));
    expect(invokeMock).toHaveBeenCalledWith("project_rename", {
      projectId: projects[0].id,
      name: "Fotos",
    });
  });

  it("requires typing the project name to confirm delete", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    render(<ProjectsView projects={projects} onRefresh={async () => {}} onOpen={() => {}} />);
    await userEvent.click(screen.getAllByRole("button", { name: "Eliminar" })[0]);

    const dialog = screen.getByRole("dialog");
    const deleteButton = within(dialog).getByRole("button", { name: "Eliminar" });
    expect(deleteButton).toBeDisabled();
    await userEvent.type(within(dialog).getByLabelText("Nombre para confirmar"), "Fotosíntesis");
    expect(deleteButton).toBeEnabled();
    await userEvent.click(deleteButton);
    expect(invokeMock).toHaveBeenCalledWith("project_delete", { projectId: projects[0].id });
  });

  it("renders hostile names as text, not as HTML", () => {
    const hostile = [{ id: "x", name: "<img src=x onerror=alert(1)>" }];
    render(<ProjectsView projects={hostile} onRefresh={async () => {}} onOpen={() => {}} />);
    expect(screen.getByText("<img src=x onerror=alert(1)>")).toBeInTheDocument();
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
  });
});
