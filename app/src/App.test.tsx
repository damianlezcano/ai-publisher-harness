import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import App from "./App";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: vi.fn(() => ({
    onDragDropEvent: vi.fn().mockResolvedValue(() => {}),
  })),
}));

const invokeMock = vi.mocked(invoke);
const listenMock = vi.mocked(listen);

const summary = [{ id: "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22", name: "Fotosíntesis" }];
const projectView = {
  id: "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22",
  name: "Fotosíntesis",
  materials: [],
  creations: [],
  publication: { state: "local", publicUrl: null },
};

beforeEach(() => {
  invokeMock.mockReset();
  listenMock.mockReset();
  listenMock.mockResolvedValue(() => {});
});

describe("App", () => {
  it("navigates from the projects list into a project workspace", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "project_list") return Promise.resolve(summary);
      if (cmd === "project_open") return Promise.resolve(projectView);
      if (cmd === "model_list") return Promise.resolve([]);
      if (cmd === "provider_list") return Promise.resolve([]);
      if (cmd === "model_get_selected") {
        return Promise.resolve({
          model: {
            providerId: "opencode",
            modelId: "big-pickle",
            name: "big-pickle",
            free: true,
            recommended: true,
            deprecated: false,
          },
          notice: null,
          requiresChoice: false,
        });
      }
      return Promise.resolve(undefined);
    });

    render(<App />);
    await waitFor(() => expect(screen.getByText("Fotosíntesis")).toBeInTheDocument());

    await userEvent.click(screen.getByRole("button", { name: "Abrir" }));
    await waitFor(() =>
      expect(screen.getByRole("heading", { name: "Fotosíntesis" })).toBeInTheDocument(),
    );
    expect(screen.getByRole("heading", { name: "Asistente" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Materiales" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Creaciones" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Compartir" })).toBeInTheDocument();
  });

  it("shows the projects view as the landing screen", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "project_list") return Promise.resolve(summary);
      if (cmd === "model_list") return Promise.resolve([]);
      if (cmd === "provider_list") return Promise.resolve([]);
      if (cmd === "model_get_selected") {
        return Promise.resolve({
          model: {
            providerId: "opencode",
            modelId: "big-pickle",
            name: "big-pickle",
            free: true,
            recommended: true,
            deprecated: false,
          },
          notice: null,
          requiresChoice: false,
        });
      }
      return Promise.resolve(undefined);
    });
    render(<App />);
    await waitFor(() =>
      expect(screen.getByRole("heading", { name: "Mis proyectos" })).toBeInTheDocument(),
    );
  });

  it("opens the Conectá tu IA panel from the app bar", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "project_list") return Promise.resolve(summary);
      if (cmd === "model_list") return Promise.resolve([]);
      if (cmd === "provider_list") return Promise.resolve([]);
      if (cmd === "model_get_selected") {
        return Promise.resolve({
          model: {
            providerId: "opencode",
            modelId: "big-pickle",
            name: "big-pickle",
            free: true,
            recommended: true,
            deprecated: false,
          },
          notice: null,
          requiresChoice: false,
        });
      }
      return Promise.resolve(undefined);
    });
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Conectá tu IA" }));
    expect(await screen.findByRole("dialog", { name: "Conectá tu IA" })).toBeInTheDocument();
  });
});
