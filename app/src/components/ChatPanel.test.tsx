import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import ChatPanel from "./ChatPanel";
import { messages } from "../messages";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

const projectId = "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22";

const materials = [
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

const creations = [
  {
    id: "c1",
    displayName: "actividad",
    kind: "web",
    visibility: "private" as const,
    byteSize: 1024,
    createdAt: "2026-08-28T15:00:00Z",
    revision: 1,
  },
];

const base = {
  projectId,
  materials,
  creations,
  messages: [] as {
    id: string;
    role: "user" | "assistant";
    text: string;
    status: "ok" | "failed" | "cancelled";
    createdAt: string;
    materialIds: string[];
    creationIds: string[];
  }[],
  agentPhase: "idle" as const,
  agentMessage: null as string | null,
  onRefresh: vi.fn(),
};

beforeEach(() => {
  invokeMock.mockReset();
  base.messages = [];
  base.onRefresh.mockReset();
});

describe("ChatPanel timeline", () => {
  it("renders the empty hint when there are no messages", () => {
    render(<ChatPanel {...base} />);
    expect(screen.getByText(messages.assistant.emptyHint)).toBeInTheDocument();
  });

  it("renders a user message with its text and role label", () => {
    render(
      <ChatPanel
        {...base}
        messages={[
          {
            id: "msg-1",
            role: "user",
            text: "Creá una actividad",
            status: "ok",
            createdAt: "2026-08-28T15:00:00Z",
            materialIds: [],
            creationIds: [],
          },
        ]}
      />,
    );
    expect(screen.getByText("Creá una actividad")).toBeInTheDocument();
    expect(screen.getByText(messages.timeline.userLabel)).toBeInTheDocument();
  });

  it("renders material chips on a user message and opens a material on click", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    render(
      <ChatPanel
        {...base}
        messages={[
          {
            id: "msg-1",
            role: "user",
            text: "Usá este material",
            status: "ok",
            createdAt: "2026-08-28T15:00:00Z",
            materialIds: ["m1"],
            creationIds: [],
          },
        ]}
      />,
    );
    const chip = screen.getByRole("button", { name: `Abrir ${materials[0].displayName}` });
    expect(chip).toBeInTheDocument();
    await userEvent.click(chip);
    expect(invokeMock).toHaveBeenCalledWith("material_open", {
      projectId,
      materialId: "m1",
    });
  });

  it("renders an assistant message with inline creation cards", () => {
    render(
      <ChatPanel
        {...base}
        messages={[
          {
            id: "msg-2",
            role: "assistant",
            text: "Acá tenés la actividad",
            status: "ok",
            createdAt: "2026-08-28T15:01:00Z",
            materialIds: [],
            creationIds: ["c1"],
          },
        ]}
      />,
    );
    expect(screen.getByText("Acá tenés la actividad")).toBeInTheDocument();
    expect(screen.getByText(messages.timeline.assistantLabel)).toBeInTheDocument();
    expect(screen.getByText(creations[0].displayName)).toBeInTheDocument();
  });

  it("renders a failed assistant message as an alert without creation cards", () => {
    render(
      <ChatPanel
        {...base}
        messages={[
          {
            id: "msg-3",
            role: "assistant",
            text: "No se pudo completar.",
            status: "failed",
            createdAt: "2026-08-28T15:02:00Z",
            materialIds: [],
            creationIds: ["c1"],
          },
        ]}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("No se pudo completar.");
    expect(screen.queryByText(creations[0].displayName)).not.toBeInTheDocument();
  });

  it("renders a cancelled assistant message as an alert", () => {
    render(
      <ChatPanel
        {...base}
        messages={[
          {
            id: "msg-4",
            role: "assistant",
            text: "Cancelado.",
            status: "cancelled",
            createdAt: "2026-08-28T15:03:00Z",
            materialIds: [],
            creationIds: [],
          },
        ]}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("Cancelado.");
  });

  it("shows the working status with a spinner", () => {
    render(<ChatPanel {...base} agentPhase="working" />);
    expect(screen.getByText(messages.agent.creating)).toBeInTheDocument();
    expect(document.querySelector(".spinner")).toHaveAttribute("aria-hidden", "true");
  });

  it("shows a completed status line", () => {
    render(<ChatPanel {...base} agentPhase="completed" agentMessage="Listo." />);
    expect(screen.getByText("Listo.")).toHaveClass("ok");
  });

  it("shows a failed status line as an alert", () => {
    render(<ChatPanel {...base} agentPhase="failed" agentMessage="Falló." />);
    expect(screen.getByRole("alert")).toHaveTextContent("Falló.");
  });

  it("renders a pending user message until it matches a persisted message", () => {
    const { rerender } = render(
      <ChatPanel
        {...base}
        pendingUser={{ text: "Pendiente", materialIds: ["m1"] }}
        messages={[]}
      />,
    );
    expect(screen.getByText("Pendiente")).toBeInTheDocument();
    expect(screen.getByText(materials[0].displayName)).toBeInTheDocument();

    rerender(
      <ChatPanel
        {...base}
        pendingUser={{ text: "Pendiente", materialIds: ["m1"] }}
        messages={[
          {
            id: "msg-5",
            role: "user",
            text: "Pendiente",
            status: "ok",
            createdAt: "2026-08-28T15:04:00Z",
            materialIds: ["m1"],
            creationIds: [],
          },
        ]}
      />,
    );
    expect(screen.getByText("Pendiente")).toBeInTheDocument();
    // The pending duplicate is suppressed; only the persisted message chip is rendered.
    const chips = screen.getAllByRole("button", { name: `Abrir ${materials[0].displayName}` });
    expect(chips.length).toBe(1);
  });
});
