import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { messages } from "../messages";
import ProjectsView from "./ProjectsView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

const projects = [
  { id: "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22", name: "Fotosíntesis" },
  { id: "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f23", name: "Sistema solar" },
];

const FIRST_RUN_DISMISSED_KEY = "educai.firstRunDismissed";

function getView(container: HTMLElement) {
  const view = container.querySelector(".view");
  if (!view) throw new Error("Projects view root not found");
  return view;
}

beforeEach(() => {
  invokeMock.mockReset();
  localStorage.clear();
});

describe("ProjectsView", () => {
  it("lists project names and never shows ids", () => {
    render(<ProjectsView projects={projects} onRefresh={async () => {}} onOpen={() => {}} />);
    expect(screen.getByText("Fotosíntesis")).toBeInTheDocument();
    expect(screen.getByText("Sistema solar")).toBeInTheDocument();
    expect(screen.queryByText(projects[0].id)).not.toBeInTheDocument();
  });

  it("renders the empty state and opens the create form from its action", async () => {
    render(<ProjectsView projects={[]} onRefresh={async () => {}} onOpen={() => {}} />);
    expect(screen.getByText(messages.project.empty.title)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: messages.project.empty.action }));
    expect(screen.getByLabelText(messages.project.nameLabel)).toBeInTheDocument();
  });

  it("renders the first-run guide with five steps and dismisses it", async () => {
    render(<ProjectsView projects={[]} onRefresh={async () => {}} onOpen={() => {}} />);
    expect(screen.getByText(messages.project.firstRun.title)).toBeInTheDocument();
    for (const step of messages.project.firstRun.steps) {
      expect(screen.getByText(step)).toBeInTheDocument();
    }
    await userEvent.click(screen.getByRole("button", { name: messages.project.firstRun.dismiss }));
    expect(localStorage.getItem(FIRST_RUN_DISMISSED_KEY)).toBe("1");
    expect(screen.queryByText(messages.project.firstRun.title)).not.toBeInTheDocument();
  });

  it("hides the first-run guide when projects exist", () => {
    render(<ProjectsView projects={projects} onRefresh={async () => {}} onOpen={() => {}} />);
    expect(screen.queryByText(messages.project.firstRun.title)).not.toBeInTheDocument();
  });

  it("opens and cancels the create form with keyboard shortcuts", async () => {
    const { container } = render(
      <ProjectsView projects={[]} onRefresh={async () => {}} onOpen={() => {}} />,
    );
    const view = getView(container);
    fireEvent.keyDown(view, { key: "n", ctrlKey: true });
    expect(screen.getByLabelText(messages.project.nameLabel)).toBeInTheDocument();
    fireEvent.keyDown(view, { key: "Escape" });
    expect(screen.queryByLabelText(messages.project.nameLabel)).not.toBeInTheDocument();
  });

  it("prefills the default project name when opening the create form", async () => {
    render(<ProjectsView projects={[]} onRefresh={async () => {}} onOpen={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: messages.project.newButton }));
    const input = screen.getByLabelText(messages.project.nameLabel) as HTMLInputElement;
    expect(input.value).toBe(messages.project.defaultName);
  });

  it("creates a project with Enter and opens it", async () => {
    const onOpen = vi.fn();
    invokeMock.mockResolvedValueOnce({ id: "new-id", name: messages.project.defaultName });
    render(<ProjectsView projects={[]} onRefresh={async () => {}} onOpen={onOpen} />);
    await userEvent.click(screen.getByRole("button", { name: messages.project.newButton }));
    await userEvent.keyboard("{Enter}");
    expect(invokeMock).toHaveBeenCalledWith("project_create", {
      name: messages.project.defaultName,
    });
    await waitFor(() => expect(onOpen).toHaveBeenCalledWith("new-id"));
  });

  it("creates a project with a custom name and opens it", async () => {
    const onOpen = vi.fn();
    invokeMock.mockResolvedValueOnce({ id: "new-id", name: "Nuevo" });
    render(<ProjectsView projects={[]} onRefresh={async () => {}} onOpen={onOpen} />);
    await userEvent.click(screen.getByRole("button", { name: messages.project.newButton }));
    const input = screen.getByLabelText(messages.project.nameLabel);
    await userEvent.clear(input);
    await userEvent.type(input, "Nuevo");
    await userEvent.click(screen.getByRole("button", { name: messages.common.create }));
    expect(invokeMock).toHaveBeenCalledWith("project_create", { name: "Nuevo" });
    await waitFor(() => expect(onOpen).toHaveBeenCalledWith("new-id"));
  });

  it("renames a project", async () => {
    invokeMock.mockResolvedValueOnce({ id: projects[0].id, name: "Fotos" });
    render(<ProjectsView projects={projects} onRefresh={async () => {}} onOpen={() => {}} />);
    await userEvent.click(screen.getAllByRole("button", { name: messages.project.rename })[0]);
    const input = screen.getByLabelText(messages.project.renameLabel);
    await userEvent.clear(input);
    await userEvent.type(input, "Fotos");
    await userEvent.click(screen.getByRole("button", { name: messages.common.save }));
    expect(invokeMock).toHaveBeenCalledWith("project_rename", {
      projectId: projects[0].id,
      name: "Fotos",
    });
  });

  it("requires typing the project name to confirm delete", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    render(<ProjectsView projects={projects} onRefresh={async () => {}} onOpen={() => {}} />);
    await userEvent.click(screen.getAllByRole("button", { name: messages.common.delete })[0]);

    const dialog = screen.getByRole("dialog");
    const deleteButton = within(dialog).getByRole("button", { name: messages.common.delete });
    expect(deleteButton).toBeDisabled();
    await userEvent.type(
      within(dialog).getByLabelText(messages.common.confirmNameLabel),
      "Fotosíntesis",
    );
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
