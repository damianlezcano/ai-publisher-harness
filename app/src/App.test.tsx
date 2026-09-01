import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { listen, type Event } from "@tauri-apps/api/event";
import App from "./App";
import type { ProjectSummary, ProjectView } from "./types";
import { messages } from "./messages";

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

const baseSummary: ProjectSummary = {
  id: "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22",
  name: "Fotosíntesis",
  createdAt: "2026-08-31T10:00:00Z",
  updatedAt: "2026-08-31T10:30:00Z",
  shared: false,
};

const otherSummary: ProjectSummary = {
  id: "0198e4a6-6e70-7c02-8c0e-8b6fd26f1f23",
  name: "Ecosistemas",
  createdAt: "2026-08-31T09:00:00Z",
  updatedAt: "2026-08-31T09:30:00Z",
  shared: true,
};

const projectView: ProjectView = {
  id: "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22",
  name: "Fotosíntesis",
  materials: [],
  creations: [],
  publication: { state: "local", publicUrl: null },
  messages: [],
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
  agentStatus?: string | string[];
}

function mockBackend(options: MockOptions = {}) {
  const {
    projects = [baseSummary],
    openError,
    requiresChoice = false,
    freeModel: useFree = true,
    agentSendError,
    agentStatus = "ready",
  } = options;

  const allProjects = projects.map((project) => ({ ...project }));
  let agentStatusIndex = 0;
  function nextAgentStatus(): string {
    if (typeof agentStatus === "string") return agentStatus;
    const status = agentStatus[Math.min(agentStatusIndex, agentStatus.length - 1)];
    if (agentStatusIndex < agentStatus.length - 1) agentStatusIndex++;
    return status;
  }

  invokeMock.mockImplementation((cmd: string, args?: unknown) => {
    switch (cmd) {
      case "project_list":
        return Promise.resolve([...allProjects]);
      case "project_create": {
        const name =
          (args as { name?: string } | undefined)?.name ?? messages.conversation.defaultName;
        const created: ProjectSummary = {
          id: "new-conversation-id",
          name,
          createdAt: "2026-08-31T11:00:00Z",
          updatedAt: "2026-08-31T11:00:00Z",
          shared: false,
        };
        allProjects.push(created);
        return Promise.resolve(created);
      }
      case "project_open": {
        if (openError) return Promise.reject(openError);
        const { projectId } = (args as { projectId: string }) ?? {};
        const summary = allProjects.find((project) => project.id === projectId);
        return Promise.resolve({
          ...projectView,
          id: projectId ?? projectView.id,
          name: summary?.name ?? projectView.name,
        });
      }
      case "project_rename": {
        const { projectId, name } = (args as { projectId: string; name: string }) ?? {};
        const target = allProjects.find((p) => p.id === projectId);
        if (target) target.name = name;
        return Promise.resolve(target ?? null);
      }
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
      case "app_status":
        return Promise.resolve({ version: "0.1.0", agent: nextAgentStatus() });
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

async function waitForWorkspace(expectedName: string = projectView.name) {
  await waitFor(() =>
    expect(screen.getByRole("heading", { name: expectedName })).toBeInTheDocument(),
  );
}

beforeEach(() => {
  invokeMock.mockReset();
  listenMock.mockReset();
  listenMock.mockResolvedValue(() => {});
  taskHandler = null;
});

describe("App", () => {
  it("creates and opens a default conversation on first launch when no conversations exist", async () => {
    mockBackend({ projects: [] });
    render(<App />);
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("project_create", {
        name: messages.conversation.defaultName,
      }),
    );
    await waitForWorkspace(messages.conversation.defaultName);
    expect(
      screen.queryByRole("heading", { name: messages.project.listHeading }),
    ).not.toBeInTheDocument();
  });

  it("opens the first conversation directly when conversations already exist", async () => {
    mockBackend({ projects: [baseSummary, otherSummary] });
    render(<App />);
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("project_open", { projectId: baseSummary.id }),
    );
    await waitForWorkspace();
    expect(
      screen.queryByRole("heading", { name: messages.project.listHeading }),
    ).not.toBeInTheDocument();
  });

  it("renders the conversation sidebar with the selected item marked", async () => {
    mockBackend({ projects: [baseSummary, otherSummary] });
    render(<App />);
    const sidebar = await screen.findByRole("navigation", {
      name: messages.conversations.listAriaLabel,
    });
    expect(sidebar).toBeInTheDocument();

    const selected = await waitFor(() => {
      const button = screen.getByRole("button", { name: new RegExp(baseSummary.name) });
      expect(button).toHaveAttribute("aria-current", "page");
      return button;
    });
    expect(selected).toBeInTheDocument();

    const sharedButton = screen.getByRole("button", { name: new RegExp(otherSummary.name) });
    expect(sharedButton).not.toHaveAttribute("aria-current");
    expect(screen.getByText(messages.conversations.sharedLabel)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: messages.conversations.title })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: messages.conversations.newButton }),
    ).toBeInTheDocument();
  });

  it("renames a conversation from the sidebar and refreshes the list", async () => {
    mockBackend({ projects: [baseSummary, otherSummary] });
    render(<App />);
    await waitForWorkspace();

    const renameButton = screen.getAllByRole("button", {
      name: messages.conversations.renameAriaLabel,
    })[0];
    await userEvent.click(renameButton);

    const input = screen.getByLabelText(messages.conversations.renameLabel);
    await userEvent.clear(input);
    await userEvent.type(input, "Renombrado");
    await userEvent.click(screen.getByRole("button", { name: messages.common.save }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("project_rename", {
        projectId: baseSummary.id,
        name: "Renombrado",
      }),
    );
  });

  it("opens settings from the gear button and restores the conversation on close", async () => {
    mockBackend({ projects: [baseSummary] });
    render(<App />);
    await waitForWorkspace();

    await userEvent.click(screen.getByRole("button", { name: messages.app.settings }));
    expect(
      await screen.findByRole("dialog", { name: messages.provider.heading }),
    ).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: messages.common.close }));
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: messages.provider.heading }),
      ).not.toBeInTheDocument(),
    );
    expect(screen.getByRole("heading", { name: projectView.name })).toBeInTheDocument();
  });

  it("creates a conversation with Ctrl+N", async () => {
    mockBackend({ projects: [baseSummary] });
    render(<App />);
    await waitForWorkspace();
    await userEvent.keyboard("{Control>}n{/Control}");
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("project_create", {
        name: messages.conversation.defaultName,
      }),
    );
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
    await waitForWorkspace();
    await act(async () => {
      taskHandler?.({
        event: "agent://task",
        id: 1,
        payload: {
          projectId: baseSummary.id,
          status: "completed",
          message: null,
          registeredCreationIds: [],
        },
      });
    });
    expect(
      await screen.findByText(messages.agent.ready, {}, { timeout: 5000 }),
    ).toBeInTheDocument();
  });

  it("shows free-model state only in the compact selector, never as a banner", async () => {
    mockBackend();
    render(<App />);
    await waitForWorkspace();
    expect(screen.queryByText("Modelo gratuito")).not.toBeInTheDocument();
    expect(screen.queryByText(/No hay una IA conectada/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Conectar IA" })).not.toBeInTheDocument();
    const modelSelect = screen.getByLabelText(messages.model.label) as HTMLSelectElement;
    expect(modelSelect).toBeInTheDocument();
    await waitFor(() =>
      expect(Array.from(modelSelect.options).map((option) => option.textContent)).toContain(
        "big-pickle / Gratis",
      ),
    );
    expect(screen.queryByText(/::/)).not.toBeInTheDocument();
  });

  it("shows the requires-choice banner and opens the panel from Conectar IA", async () => {
    mockBackend({ requiresChoice: true });
    render(<App />);
    const bannerText = await screen.findByText(/No hay una IA conectada/, {
      selector: ".provider-status-banner p",
    });
    expect(bannerText).toBeInTheDocument();
    const connectButton = bannerText.closest(".provider-status-banner")!.querySelector("button");
    expect(connectButton).toHaveTextContent("Conectar IA");
    await userEvent.click(connectButton!);
    expect(
      await screen.findByRole("dialog", { name: messages.provider.heading }),
    ).toBeInTheDocument();
  });

  it("disables the composer when a model choice is required", async () => {
    mockBackend({ requiresChoice: true });
    render(<App />);
    await waitForWorkspace();
    expect(screen.getByLabelText("Pedido a la IA")).toBeDisabled();
  });

  it("shows the needs-reconnect banner after a connect-ai guidance error", async () => {
    mockBackend({ agentSendError: { code: "credential_revoked", message: "raw" } });
    render(<App />);
    await waitForWorkspace();
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

  it("shows a guided toast when opening a conversation fails", async () => {
    mockBackend({ openError: { code: "open_failed", message: "detalle interno" } });
    render(<App />);
    expect(await screen.findByText("No pudimos abrir el recurso.")).toBeInTheDocument();
    expect(screen.queryByText("detalle interno")).not.toBeInTheDocument();
  });

  it("shows no preparing state and enables the composer when the backend is ready immediately", async () => {
    mockBackend();
    render(<App />);
    await waitFor(() => expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled());
    expect(screen.queryByText(messages.assistant.starting)).not.toBeInTheDocument();
  });

  it("shows preparing during a delayed cold startup and enables the composer once ready", async () => {
    vi.useFakeTimers();
    mockBackend({ agentStatus: ["starting", "ready"] });
    const { unmount } = render(<App />);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });

    expect(screen.getByRole("heading", { name: projectView.name })).toBeInTheDocument();
    expect(screen.getByText(messages.assistant.starting)).toBeInTheDocument();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(600);
    });

    expect(screen.queryByText(messages.assistant.starting)).not.toBeInTheDocument();
    expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled();

    unmount();
    vi.useRealTimers();
  });

  it("shows a terminal error when the backend reports a genuine startup failure", async () => {
    mockBackend({ agentStatus: "failed" });
    render(<App />);
    await waitForWorkspace();
    expect(await screen.findByText(messages.error.aiUnavailable.title)).toBeInTheDocument();
    expect(screen.queryByText(messages.assistant.starting)).not.toBeInTheDocument();
    expect(screen.getByLabelText("Pedido a la IA")).toBeDisabled();
  });

  it("recovers automatically when a failed backend later reports ready", async () => {
    mockBackend({ agentStatus: ["failed", "failed", "ready"] });
    render(<App />);
    await waitFor(() =>
      expect(screen.getByText(messages.error.aiUnavailable.title)).toBeInTheDocument(),
    );
    await waitFor(
      () => expect(screen.queryByText(messages.error.aiUnavailable.title)).not.toBeInTheDocument(),
      { timeout: 3000 },
    );
    expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled();
  });

  it("clicking Reintentar after a terminal failure re-checks the backend", async () => {
    mockBackend({ agentStatus: "failed" });
    render(<App />);
    await waitForWorkspace();
    expect(await screen.findByText(messages.error.aiUnavailable.title)).toBeInTheDocument();

    const retry = screen.getByRole("button", { name: messages.error.actionRetry });
    expect(retry).toBeEnabled();
    await userEvent.click(retry);

    expect(await screen.findByText(messages.assistant.starting)).toBeInTheDocument();
  });

  it("renders legacy default names as Conversación nueva in the sidebar and header", async () => {
    const legacySummary: ProjectSummary = {
      ...baseSummary,
      name: "Proyecto sin título 1",
    };
    mockBackend({ projects: [legacySummary] });
    render(<App />);

    await waitFor(() =>
      expect(
        screen.getByRole("heading", { name: messages.conversation.defaultName }),
      ).toBeInTheDocument(),
    );
    expect(
      screen.getByRole("button", { name: new RegExp(messages.conversation.defaultName) }),
    ).toBeInTheDocument();
    expect(screen.queryByText("Proyecto sin título")).not.toBeInTheDocument();
    expect(screen.queryAllByText(/Proyecto sin título/)).toHaveLength(0);
  });

  it("renders user-renamed conversation names unchanged in the sidebar and header", async () => {
    mockBackend({ projects: [baseSummary] });
    render(<App />);

    await waitFor(() =>
      expect(screen.getByRole("heading", { name: "Fotosíntesis" })).toBeInTheDocument(),
    );
    expect(screen.getByRole("button", { name: /Fotosíntesis/ })).toBeInTheDocument();
  });

  it("preserves sidebar ordering when legacy names are present", async () => {
    const legacyNewest: ProjectSummary = {
      ...baseSummary,
      id: "legacy-newest-id",
      name: "Proyecto sin título",
      updatedAt: "2026-08-31T12:00:00Z",
    };
    const legacyOlder: ProjectSummary = {
      ...otherSummary,
      id: "legacy-older-id",
      name: "Proyecto sin título 2",
      updatedAt: "2026-08-31T11:00:00Z",
    };
    const userNamed: ProjectSummary = {
      id: "user-named-id",
      name: "Fotosíntesis",
      createdAt: "2026-08-31T08:00:00Z",
      updatedAt: "2026-08-31T10:00:00Z",
      shared: false,
    };
    mockBackend({ projects: [legacyNewest, legacyOlder, userNamed] });
    render(<App />);

    const sidebar = await screen.findByRole("navigation", {
      name: messages.conversations.listAriaLabel,
    });
    const buttons = sidebar.querySelectorAll(".conversation-select");
    expect(buttons).toHaveLength(3);
    expect(buttons[0]).toHaveTextContent(messages.conversation.defaultName);
    expect(buttons[1]).toHaveTextContent(messages.conversation.defaultName);
    expect(buttons[2]).toHaveTextContent("Fotosíntesis");
  });

  it("normalizes legacy names on restart without leaking project terminology", async () => {
    const legacySummary: ProjectSummary = {
      ...baseSummary,
      name: "Proyecto sin título 1",
    };

    for (let mount = 0; mount < 2; mount++) {
      mockBackend({ projects: [legacySummary] });
      const { unmount } = render(<App />);

      await waitFor(() =>
        expect(
          screen.getByRole("heading", { name: messages.conversation.defaultName }),
        ).toBeInTheDocument(),
      );
      expect(screen.queryByText("Proyecto sin título")).not.toBeInTheDocument();
      expect(screen.queryByText("Project")).not.toBeInTheDocument();
      expect(screen.queryByText("ProjectId")).not.toBeInTheDocument();

      unmount();
    }
  });
});
