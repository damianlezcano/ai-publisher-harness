import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import WorkspaceView from "./WorkspaceView";
import { messages } from "../messages";
import type { CreationView, MaterialView, MessageView, ProjectView } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);
const getCurrentWebviewMock = vi.mocked(getCurrentWebview);

function mockDragDrop(): {
  dropHandler: (event: { payload: { type: string; paths?: string[] } }) => void;
} {
  let dropHandler: ((event: { payload: { type: string; paths?: string[] } }) => void) | undefined;
  getCurrentWebviewMock.mockReturnValue({
    onDragDropEvent: vi.fn().mockImplementation((handler) => {
      dropHandler = handler;
      return Promise.resolve(() => {});
    }),
  } as never);
  return {
    dropHandler: (event) => dropHandler?.(event),
  };
}

const projectId = "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22";

const materials: MaterialView[] = [
  {
    id: "m1",
    displayName: "diagrama.png",
    originalFileName: "diagrama.png",
    kind: "image",
    byteSize: 1024,
    createdAt: "2026-08-28T15:00:00Z",
  },
  {
    id: "m2",
    displayName: "manual.pdf",
    originalFileName: "manual.pdf",
    kind: "pdf",
    byteSize: 2048,
    createdAt: "2026-08-28T15:00:00Z",
  },
];

const creations: CreationView[] = [
  {
    id: "c1",
    displayName: "actividad",
    kind: "web",
    visibility: "private",
    byteSize: 1024,
    createdAt: "2026-08-28T15:00:00Z",
    revision: 1,
  },
];

const messagesList: MessageView[] = [
  {
    id: "msg-1",
    role: "user",
    text: "Creá algo",
    status: "ok",
    createdAt: "2026-08-28T15:00:00Z",
    materialIds: ["m1"],
    creationIds: [],
  },
  {
    id: "msg-2",
    role: "assistant",
    text: "Acá está",
    status: "ok",
    createdAt: "2026-08-28T15:01:00Z",
    materialIds: [],
    creationIds: ["c1"],
  },
  {
    id: "msg-3",
    role: "assistant",
    text: "No se pudo.",
    status: "failed",
    createdAt: "2026-08-28T15:02:00Z",
    materialIds: [],
    creationIds: [],
  },
];

function makeProject(extraMaterials?: MaterialView[]): ProjectView {
  return {
    id: projectId,
    name: "Fotosíntesis",
    materials: extraMaterials ? [...materials, ...extraMaterials] : materials,
    creations,
    publication: { state: "local", publicUrl: null },
    messages: messagesList,
  };
}

const baseProps = {
  agentPhase: "idle" as const,
  agentMessage: null as string | null,
  onBack: vi.fn(),
  onRefresh: vi.fn(),
  aiUsable: true,
  onOpenProvider: vi.fn(),
  onProviderError: vi.fn(),
};

function setupApi(options: { agentSendResult?: unknown; agentSendError?: unknown } = {}) {
  const { agentSendResult = undefined, agentSendError } = options;
  invokeMock.mockImplementation((cmd: string) => {
    switch (cmd) {
      case "model_list":
        return Promise.resolve([
          {
            providerId: "opencode",
            modelId: "big-pickle",
            name: "Big Pickle",
            free: true,
            recommended: true,
            deprecated: false,
          },
        ]);
      case "provider_list":
        return Promise.resolve([]);
      case "model_get_selected":
        return Promise.resolve({
          model: {
            providerId: "opencode",
            modelId: "big-pickle",
            name: "Big Pickle",
            free: true,
            recommended: true,
            deprecated: false,
          },
          notice: null,
          requiresChoice: false,
        });
      case "agent_send":
        return agentSendError ? Promise.reject(agentSendError) : Promise.resolve(agentSendResult);
      default:
        return Promise.resolve(undefined);
    }
  });
}

beforeEach(() => {
  invokeMock.mockReset();
  baseProps.onRefresh.mockReset();
  baseProps.onOpenProvider.mockReset();
  baseProps.onProviderError.mockReset();
  baseProps.onBack.mockReset();
  getCurrentWebviewMock.mockReset();
  getCurrentWebviewMock.mockReturnValue({
    onDragDropEvent: vi.fn().mockResolvedValue(() => {}),
  } as never);
});

describe("WorkspaceView", () => {
  it("renders the project name as an h1", () => {
    render(<WorkspaceView project={makeProject()} {...baseProps} />);
    expect(screen.getByRole("heading", { name: "Fotosíntesis" })).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: messages.project.backToList }),
    ).not.toBeInTheDocument();
  });

  it("renders user and assistant messages from project.messages", () => {
    render(<WorkspaceView project={makeProject()} {...baseProps} />);
    expect(screen.getByText("Creá algo")).toBeInTheDocument();
    expect(screen.getByText("Acá está")).toBeInTheDocument();
    expect(screen.getByText(messages.timeline.userLabel)).toBeInTheDocument();
    expect(screen.getAllByText(messages.timeline.assistantLabel).length).toBeGreaterThan(0);
  });

  it("renders a failed assistant message as an error alert", () => {
    render(<WorkspaceView project={makeProject()} {...baseProps} />);
    const alerts = screen.getAllByRole("alert");
    expect(alerts.some((alert) => alert.textContent?.includes("No se pudo."))).toBe(true);
  });

  it("renders material chips on user messages", () => {
    render(<WorkspaceView project={makeProject()} {...baseProps} />);
    expect(
      screen.getByRole("button", { name: `Abrir ${materials[0].displayName}` }),
    ).toBeInTheDocument();
  });

  it("renders inline creation cards on assistant messages", () => {
    render(<WorkspaceView project={makeProject()} {...baseProps} />);
    expect(screen.getByText(creations[0].displayName)).toBeInTheDocument();
  });

  it("sends a prompt with attachment ids and refreshes the conversation", async () => {
    setupApi();
    render(<WorkspaceView project={makeProject()} {...baseProps} />);

    await waitFor(() => expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled());

    await userEvent.click(screen.getByRole("button", { name: messages.assistant.attachMaterial }));
    const materialButton = screen.getByRole("button", { name: materials[0].displayName });
    await userEvent.click(materialButton);

    const textarea = screen.getByLabelText("Pedido a la IA");
    await userEvent.type(textarea, "Creá una actividad");
    await userEvent.click(screen.getByRole("button", { name: messages.common.send }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("agent_send", {
        projectId,
        prompt: "Creá una actividad",
        attachmentIds: ["m1"],
      }),
    );
    expect(baseProps.onRefresh).toHaveBeenCalled();
  });

  it("calls onProviderError when a send error guides to connect-ai", async () => {
    setupApi({ agentSendError: { code: "credential_revoked", message: "raw" } });
    render(<WorkspaceView project={makeProject()} {...baseProps} />);

    await waitFor(() => expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled());
    const textarea = screen.getByLabelText("Pedido a la IA");
    await userEvent.type(textarea, "Creá algo");
    await userEvent.click(screen.getByRole("button", { name: messages.common.send }));

    await waitFor(() => expect(baseProps.onProviderError).toHaveBeenCalledTimes(1));
  });

  it("clears the optimistic pending bubble after a send error", async () => {
    setupApi({ agentSendError: { code: "credential_revoked", message: "raw" } });
    render(<WorkspaceView project={makeProject()} {...baseProps} />);

    await waitFor(() => expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled());
    const textarea = screen.getByLabelText("Pedido a la IA");
    await userEvent.type(textarea, "Este mensaje falla");
    await userEvent.click(screen.getByRole("button", { name: messages.common.send }));

    await waitFor(() => expect(baseProps.onProviderError).toHaveBeenCalledTimes(1));
    expect(screen.queryByText("Este mensaje falla")).not.toBeInTheDocument();
  });

  it("cancels an in-flight task and refreshes", async () => {
    setupApi();
    render(<WorkspaceView project={makeProject()} {...baseProps} agentPhase="working" />);

    await userEvent.click(screen.getByRole("button", { name: messages.common.cancel }));

    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("agent_cancel", { projectId }));
    expect(baseProps.onRefresh).toHaveBeenCalled();
  });

  it("lists unattached materials in the timeline and excludes attached materials from a second copy", () => {
    const project = makeProject();
    render(<WorkspaceView project={project} {...baseProps} />);
    expect(screen.getByText(messages.timeline.resourceLabel)).toBeInTheDocument();
    expect(screen.getByText(materials[1].displayName)).toBeInTheDocument();
    expect(
      screen.queryAllByRole("button", { name: `Abrir ${materials[0].displayName}` }).length,
    ).toBe(1);
    expect(screen.queryByText(messages.timeline.unattachedTitle)).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: messages.material.addFile }),
    ).not.toBeInTheDocument();
  });

  it("shows a lightweight drop overlay only while dragging over the conversation", async () => {
    const { dropHandler } = mockDragDrop();
    setupApi();
    render(<WorkspaceView project={makeProject()} {...baseProps} />);
    expect(screen.queryByText(messages.material.dropOverlay)).not.toBeInTheDocument();
    await waitFor(() => expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled());
    act(() => {
      dropHandler({ payload: { type: "over" } });
    });
    expect(await screen.findByText(messages.material.dropOverlay)).toBeInTheDocument();
    act(() => {
      dropHandler({ payload: { type: "leave" } });
    });
    await waitFor(() =>
      expect(screen.queryByText(messages.material.dropOverlay)).not.toBeInTheDocument(),
    );
  });

  it("shows the creating status while the agent is working", () => {
    render(<WorkspaceView project={makeProject()} {...baseProps} agentPhase="working" />);
    expect(screen.getByText(messages.agent.creating)).toBeInTheDocument();
  });

  it("renders Compartir controls in the composer bar and on each creation card", () => {
    render(<WorkspaceView project={makeProject()} {...baseProps} />);
    const shareButtons = screen.getAllByRole("button", { name: messages.sharing.shareAction });
    expect(shareButtons.length).toBe(2);
  });
});
