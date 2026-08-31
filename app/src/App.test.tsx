import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { listen, type Event } from "@tauri-apps/api/event";
import App from "./App";
import type { ProjectSummary, ProjectView } from "./types";

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

const summary: ProjectSummary[] = [
  { id: "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22", name: "Fotosíntesis" },
];

const projectView: ProjectView = {
  id: "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22",
  name: "Fotosíntesis",
  materials: [],
  creations: [],
  publication: { state: "local", publicUrl: null },
};

const freeModel = {
  providerId: "opencode",
  modelId: "big-pickle",
  name: "big-pickle",
  free: true,
  recommended: true,
  deprecated: false,
};

const paidModel = { ...freeModel, modelId: "gpt-5", name: "gpt-5", free: false };

let taskHandler: ((event: Event<unknown>) => void) | null = null;

interface MockOptions {
  projects?: ProjectSummary[];
  openError?: unknown;
  requiresChoice?: boolean;
  freeModel?: boolean;
  agentSendError?: unknown;
}

function mockBackend(options: MockOptions = {}) {
  const {
    projects = summary,
    openError,
    requiresChoice = false,
    freeModel: useFree = true,
    agentSendError,
  } = options;
  invokeMock.mockImplementation((cmd: string) => {
    switch (cmd) {
      case "project_list":
        return Promise.resolve(projects);
      case "project_open":
        return openError ? Promise.reject(openError) : Promise.resolve(projectView);
      case "model_list":
        return Promise.resolve(useFree ? [freeModel] : []);
      case "provider_list":
        return Promise.resolve([]);
      case "model_get_selected":
        return Promise.resolve({
          model: useFree ? freeModel : paidModel,
          notice: null,
          requiresChoice,
        });
      case "agent_send":
        return agentSendError ? Promise.reject(agentSendError) : Promise.resolve(undefined);
      default:
        return Promise.resolve(undefined);
    }
  });
}

function captureTaskListener() {
  taskHandler = null;
  listenMock.mockImplementation((eventName: string, handler: (event: Event<unknown>) => void) => {
    if (eventName === "agent://task") taskHandler = handler;
    return Promise.resolve(() => {});
  });
}

async function openFirstProject() {
  await userEvent.click(await screen.findByRole("button", { name: "Abrir" }));
  await waitFor(() =>
    expect(screen.getByRole("heading", { name: "Fotosíntesis" })).toBeInTheDocument(),
  );
}

beforeEach(() => {
  invokeMock.mockReset();
  listenMock.mockReset();
  listenMock.mockResolvedValue(() => {});
  taskHandler = null;
});

describe("App", () => {
  it("navigates from the projects list into a project workspace", async () => {
    mockBackend();
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
    mockBackend();
    render(<App />);
    await waitFor(() =>
      expect(screen.getByRole("heading", { name: "Mis proyectos" })).toBeInTheDocument(),
    );
  });

  it("opens the Conectá tu IA panel from the app bar", async () => {
    mockBackend();
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Conectá tu IA" }));
    expect(await screen.findByRole("dialog", { name: "Conectá tu IA" })).toBeInTheDocument();
  });

  it("renders a single polite toast region for announcements", async () => {
    mockBackend();
    render(<App />);
    const region = await screen.findByRole("status");
    expect(region).toHaveAttribute("aria-live", "polite");
    expect(region).toHaveAttribute("aria-atomic", "true");
  });

  it("announces the ready toast when an agent task completes", async () => {
    mockBackend();
    captureTaskListener();
    render(<App />);
    await openFirstProject();
    act(() => {
      taskHandler?.({
        event: "agent://task",
        id: 1,
        payload: {
          projectId: summary[0].id,
          status: "completed",
          message: null,
          registeredCreationIds: [],
        },
      });
    });
    expect(await screen.findByText("Tu recurso está listo.")).toBeInTheDocument();
  });

  it("shows the free-model banner without claiming a blocked AI", async () => {
    mockBackend();
    render(<App />);
    await waitFor(() => expect(screen.getByText("Modelo gratuito")).toBeInTheDocument());
    expect(screen.queryByText(/No hay una IA conectada/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Conectar IA" })).not.toBeInTheDocument();
  });

  it("shows the requires-choice banner and opens the panel from Conectar IA", async () => {
    mockBackend({ requiresChoice: true });
    render(<App />);
    expect(await screen.findByText(/No hay una IA conectada/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Conectar IA" }));
    expect(await screen.findByRole("dialog", { name: "Conectá tu IA" })).toBeInTheDocument();
  });

  it("disables the composer when a model choice is required", async () => {
    mockBackend({ requiresChoice: true });
    render(<App />);
    await openFirstProject();
    expect(screen.getByLabelText("Pedido a la IA")).toBeDisabled();
  });

  it("shows the needs-reconnect banner after a connect-ai guidance error", async () => {
    mockBackend({ agentSendError: { code: "credential_revoked", message: "raw" } });
    render(<App />);
    await openFirstProject();
    await userEvent.type(screen.getByLabelText("Pedido a la IA"), "Creá algo");
    await userEvent.click(screen.getByRole("button", { name: "Enviar" }));
    await waitFor(() =>
      expect(
        screen.getByText("Necesitás volver a conectar tu cuenta.", {
          selector: ".provider-status-banner p",
        }),
      ).toBeInTheDocument(),
    );
  });

  it("shows a guided toast when opening a project fails", async () => {
    mockBackend({ openError: { code: "open_failed", message: "detalle interno" } });
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Abrir" }));
    expect(await screen.findByText("No pudimos abrir el recurso.")).toBeInTheDocument();
    expect(screen.queryByText("detalle interno")).not.toBeInTheDocument();
  });
});
