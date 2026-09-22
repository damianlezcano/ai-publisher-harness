import { describe, expect, it } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import AssistantMetrics from "./AssistantMetrics";
import { messages } from "../messages";
import type { TurnMetrics } from "../types";

const messageId = "assistant-1";
const createdAt = "2026-09-15T22:18:00";

function metrics(overrides: Partial<TurnMetrics> = {}): TurnMetrics {
  return {
    provider: null,
    model: null,
    inputTokens: null,
    outputTokens: null,
    cacheReadTokens: null,
    cacheWriteTokens: null,
    totalTokens: null,
    costUsd: null,
    turnDurationMs: null,
    source: null,
    remoteCalls: null,
    materialCount: null,
    corpusBytes: null,
    corpusUtf8Chars: null,
    corpusEstTokens: null,
    retrievalCandidateCount: null,
    selectedEvidenceCount: null,
    selectedEvidenceBytes: null,
    selectedEvidenceUtf8Chars: null,
    evidenceEstTokens: null,
    contextReductionPct: null,
    semanticProviderState: null,
    requestPreparationMs: null,
    retrievalMode: null,
    eligibleMaterials: null,
    materialsInspected: null,
    chunksInspected: null,
    exhaustiveCoverage: null,
    lexicalHits: null,
    semanticHits: null,
    localMode: null,
    sourceNames: [],
    ...overrides,
  };
}

function renderMetrics(m: TurnMetrics) {
  return render(<AssistantMetrics messageId={messageId} createdAt={createdAt} turnMetrics={m} />);
}

function infoButton() {
  return screen.getByRole("button", { name: messages.turnMetrics.infoAria });
}

describe("AssistantMetrics compact line", () => {
  it("renders a compact line with provider-actual tokens", () => {
    renderMetrics(
      metrics({
        source: "provider_actual",
        inputTokens: 2173,
        outputTokens: 487,
        turnDurationMs: 9900,
      }),
    );
    const segments = document.querySelector(".turn-metrics-segments");
    expect(segments?.textContent).toContain("↑ 2.173");
    expect(segments?.textContent).toContain("↓ 487");
    expect(segments?.textContent).toContain("9,9 s");
  });

  it("omits input/output tokens when provider usage is not provider-actual", () => {
    renderMetrics(metrics({ source: "unavailable" }));
    const segments = document.querySelector(".turn-metrics-segments");
    expect(segments?.textContent).not.toContain("↑");
    expect(segments?.textContent).not.toContain("↓");
  });

  it("does not show provider tokens as a fabricated estimate when absent", () => {
    renderMetrics(
      metrics({
        source: "provider_actual",
        inputTokens: null,
        outputTokens: null,
      }),
    );
    const segments = document.querySelector(".turn-metrics-segments")?.textContent ?? "";
    expect(segments).not.toContain("↑");
    expect(segments).not.toContain("↓");
  });

  it("never renders a Fuentes block in the assistant metrics line", () => {
    const { container } = renderMetrics(
      metrics({ sourceNames: ["file-a.md", "file-b.md"], source: "provider_actual" }),
    );
    expect(container.textContent).not.toContain("Fuentes:");
  });

  it("does not render Knowledge reduction in the compact line", () => {
    renderMetrics(
      metrics({
        source: "provider_actual",
        retrievalMode: "normal",
        corpusEstTokens: 1000,
        evidenceEstTokens: 250,
      }),
    );
    const segments = document.querySelector(".turn-metrics-segments")?.textContent ?? "";
    expect(segments).not.toContain("Knowledge");
  });
});

describe("AssistantMetrics popover", () => {
  it("opens on click and shows response details", async () => {
    renderMetrics(
      metrics({
        source: "provider_actual",
        inputTokens: 2173,
        outputTokens: 487,
        provider: "opencode",
        model: "big-pickle",
      }),
    );
    await userEvent.click(infoButton());
    const panel = screen.getByRole("region", { name: messages.turnMetrics.detailsLabel });
    expect(panel).toHaveTextContent("2.173 tokens");
    expect(panel).toHaveTextContent("487 tokens");
    expect(panel).toHaveTextContent("opencode");
    expect(panel).toHaveTextContent("big-pickle");
  });

  it("opens on keyboard focus", () => {
    renderMetrics(metrics({ source: "provider_actual", inputTokens: 10 }));
    fireEvent.focus(infoButton());
    expect(
      screen.getByRole("region", { name: messages.turnMetrics.detailsLabel }),
    ).toBeInTheDocument();
    expect(infoButton()).toHaveAttribute("aria-expanded", "true");
  });

  it("closes on Escape", async () => {
    renderMetrics(metrics({ source: "provider_actual" }));
    await userEvent.click(infoButton());
    expect(
      screen.getByRole("region", { name: messages.turnMetrics.detailsLabel }),
    ).toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("region", { name: messages.turnMetrics.detailsLabel })).toBeNull();
  });

  it("closes on an outside click", async () => {
    renderMetrics(metrics({ source: "provider_actual" }));
    await userEvent.click(infoButton());
    expect(
      screen.getByRole("region", { name: messages.turnMetrics.detailsLabel }),
    ).toBeInTheDocument();
    await userEvent.click(document.body);
    expect(screen.queryByRole("region", { name: messages.turnMetrics.detailsLabel })).toBeNull();
  });

  it("renders the popover through a portal into document.body", async () => {
    const { container } = renderMetrics(metrics({ source: "provider_actual" }));
    await userEvent.click(infoButton());
    const panel = screen.getByRole("region", { name: messages.turnMetrics.detailsLabel });
    expect(panel.parentElement).toBe(document.body);
    expect(container.contains(panel)).toBe(false);
  });

  it("positions the popover fixed within the viewport", async () => {
    const originalWidth = window.innerWidth;
    const originalHeight = window.innerHeight;
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 400 });
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 300 });
    const proto = HTMLElement.prototype;
    const widthDescriptor = Object.getOwnPropertyDescriptor(proto, "offsetWidth");
    const heightDescriptor = Object.getOwnPropertyDescriptor(proto, "offsetHeight");
    Object.defineProperty(proto, "offsetWidth", { configurable: true, get: () => 280 });
    Object.defineProperty(proto, "offsetHeight", { configurable: true, get: () => 150 });
    try {
      renderMetrics(metrics({ source: "provider_actual" }));
      const button = infoButton();
      button.getBoundingClientRect = () =>
        ({
          top: 100,
          left: 380,
          right: 396,
          bottom: 116,
          width: 16,
          height: 16,
          x: 380,
          y: 100,
          toJSON: () => ({}),
        }) as DOMRect;
      await userEvent.click(button);
      const panel = screen.getByRole("region", { name: messages.turnMetrics.detailsLabel });
      expect(panel.style.position).toBe("fixed");
      expect(panel.style.left).toBe("112px");
    } finally {
      if (widthDescriptor) Object.defineProperty(proto, "offsetWidth", widthDescriptor);
      else delete (proto as { offsetWidth?: unknown }).offsetWidth;
      if (heightDescriptor) Object.defineProperty(proto, "offsetHeight", heightDescriptor);
      else delete (proto as { offsetHeight?: unknown }).offsetHeight;
      Object.defineProperty(window, "innerWidth", {
        configurable: true,
        value: originalWidth,
      });
      Object.defineProperty(window, "innerHeight", {
        configurable: true,
        value: originalHeight,
      });
    }
  });

  it("shows No disponible for missing fields instead of inventing values", async () => {
    renderMetrics(metrics({ source: "provider_actual", remoteCalls: 0 }));
    await userEvent.click(infoButton());
    const panel = screen.getByRole("region", { name: messages.turnMetrics.detailsLabel });
    expect(panel).toHaveTextContent(messages.conversationDetails.metrics.unavailable);
  });

  it("shows Knowledge used when retrievalMode is set", async () => {
    renderMetrics(
      metrics({
        source: "provider_actual",
        retrievalMode: "normal",
      }),
    );
    await userEvent.click(infoButton());
    const panel = screen.getByRole("region", { name: messages.turnMetrics.detailsLabel });
    expect(panel).toHaveTextContent(messages.turnMetrics.knowledgeUsed);
    expect(panel).not.toHaveTextContent(messages.turnMetrics.knowledgeNotUsed);
  });

  it("shows Knowledge used when localMode is set", async () => {
    renderMetrics(
      metrics({
        source: "provider_actual",
        localMode: "inventory",
      }),
    );
    await userEvent.click(infoButton());
    const panel = screen.getByRole("region", { name: messages.turnMetrics.detailsLabel });
    expect(panel).toHaveTextContent(messages.turnMetrics.knowledgeUsed);
  });

  it("shows Knowledge not used when no retrievalMode or localMode", async () => {
    renderMetrics(
      metrics({
        source: "provider_actual",
        retrievalMode: null,
        localMode: null,
      }),
    );
    await userEvent.click(infoButton());
    const panel = screen.getByRole("region", { name: messages.turnMetrics.detailsLabel });
    expect(panel).toHaveTextContent(messages.turnMetrics.knowledgeNotUsed);
    expect(panel).not.toHaveTextContent(messages.turnMetrics.knowledgeUsed);
  });

  it("does not show retrieval mode, candidates, evidence, or corpus in the popover", async () => {
    renderMetrics(
      metrics({
        source: "provider_actual",
        retrievalMode: "exhaustive",
        retrievalCandidateCount: 10,
        selectedEvidenceCount: 5,
        selectedEvidenceBytes: 1000,
        selectedEvidenceUtf8Chars: 900,
        corpusEstTokens: 50000,
        evidenceEstTokens: 2000,
        contextReductionPct: 96,
      }),
    );
    await userEvent.click(infoButton());
    const panel = screen.getByRole("region", { name: messages.turnMetrics.detailsLabel });
    expect(panel).not.toHaveTextContent("Modo de recuperación");
    expect(panel).not.toHaveTextContent("Candidatos de recuperación");
    expect(panel).not.toHaveTextContent("Evidencias seleccionadas");
    expect(panel).not.toHaveTextContent("Corpus estimado");
    expect(panel).not.toHaveTextContent("Reducción estimada de contexto");
  });

  it("does not show materialCount in the popup even when present in turn metrics", async () => {
    renderMetrics(
      metrics({
        source: "provider_actual",
        materialCount: 5,
        retrievalMode: "normal",
      }),
    );
    await userEvent.click(infoButton());
    const panel = screen.getByRole("region", { name: messages.turnMetrics.detailsLabel });
    expect(panel).not.toHaveTextContent("Materiales del proyecto");
    expect(panel).toHaveTextContent(messages.turnMetrics.knowledgeUsed);
  });

  it("does not render inventory or internal-only Knowledge fields", async () => {
    renderMetrics(
      metrics({
        source: "provider_actual",
        retrievalMode: "exhaustive",
        semanticProviderState: "available",
        requestPreparationMs: 42,
        exhaustiveCoverage: "complete",
        eligibleMaterials: 10,
        materialsInspected: 10,
        chunksInspected: 40,
        lexicalHits: 3,
        semanticHits: 1,
      }),
    );
    await userEvent.click(infoButton());
    const panel = screen.getByRole("region", { name: messages.turnMetrics.detailsLabel });
    expect(panel).not.toHaveTextContent("Estado del proveedor semántico");
    expect(panel).not.toHaveTextContent("Preparación de la solicitud");
    expect(panel).not.toHaveTextContent("Cobertura exhaustiva");
    expect(panel).not.toHaveTextContent("Materiales elegibles");
    expect(panel).not.toHaveTextContent("Materiales inspeccionados");
    expect(panel).not.toHaveTextContent("Fragmentos inspeccionados");
    expect(panel).not.toHaveTextContent("Coincidencias léxicas");
    expect(panel).not.toHaveTextContent("Coincidencias semánticas");
  });

  it("P2: stays open when the user clicks inside the panel", async () => {
    renderMetrics(metrics({ source: "provider_actual" }));
    await userEvent.click(infoButton());
    const panel = screen.getByRole("region", { name: messages.turnMetrics.detailsLabel });
    await userEvent.click(panel);
    expect(
      screen.getByRole("region", { name: messages.turnMetrics.detailsLabel }),
    ).toBeInTheDocument();
  });

  it("P3: stays open when focus leaves the trigger (no blur-close)", async () => {
    renderMetrics(metrics({ source: "provider_actual" }));
    await userEvent.click(infoButton());
    fireEvent.blur(infoButton());
    expect(
      screen.getByRole("region", { name: messages.turnMetrics.detailsLabel }),
    ).toBeInTheDocument();
  });

  it("P4: stays open and repositions on scroll", async () => {
    renderMetrics(metrics({ source: "provider_actual" }));
    await userEvent.click(infoButton());
    const panel = screen.getByRole("region", { name: messages.turnMetrics.detailsLabel });
    fireEvent.scroll(window);
    expect(
      screen.getByRole("region", { name: messages.turnMetrics.detailsLabel }),
    ).toBeInTheDocument();
    expect(panel.style.visibility).toBe("visible");
  });

  it("P5: stays open during text selection inside the panel", async () => {
    renderMetrics(metrics({ source: "provider_actual" }));
    await userEvent.click(infoButton());
    const panel = screen.getByRole("region", { name: messages.turnMetrics.detailsLabel });
    fireEvent.mouseDown(panel);
    expect(
      screen.getByRole("region", { name: messages.turnMetrics.detailsLabel }),
    ).toBeInTheDocument();
  });

  it("P8: clicking the trigger again closes the panel", async () => {
    renderMetrics(metrics({ source: "provider_actual" }));
    await userEvent.click(infoButton());
    expect(
      screen.getByRole("region", { name: messages.turnMetrics.detailsLabel }),
    ).toBeInTheDocument();
    await userEvent.click(infoButton());
    expect(screen.queryByRole("region", { name: messages.turnMetrics.detailsLabel })).toBeNull();
  });

  it("P10: two metrics popovers keep independent state", async () => {
    render(
      <>
        <AssistantMetrics
          messageId="assistant-1"
          createdAt={createdAt}
          turnMetrics={metrics({ source: "provider_actual", inputTokens: 111 })}
        />
        <AssistantMetrics
          messageId="assistant-2"
          createdAt={createdAt}
          turnMetrics={metrics({ source: "provider_actual", inputTokens: 222 })}
        />
      </>,
    );
    const buttons = screen.getAllByRole("button", { name: messages.turnMetrics.infoAria });
    expect(buttons).toHaveLength(2);

    await userEvent.click(buttons[0]);
    expect(screen.getByRole("region", { name: messages.turnMetrics.detailsLabel })).toHaveAttribute(
      "id",
      "turn-metrics-assistant-1",
    );

    // Clicking inside the first panel does not open or close the second.
    await userEvent.click(screen.getByRole("region", { name: messages.turnMetrics.detailsLabel }));
    expect(screen.getAllByRole("region", { name: messages.turnMetrics.detailsLabel })).toHaveLength(
      1,
    );

    fireEvent.keyDown(document, { key: "Escape" });
    expect(
      screen.queryAllByRole("region", { name: messages.turnMetrics.detailsLabel }),
    ).toHaveLength(0);

    // The second popover opens independently afterward.
    await userEvent.click(buttons[1]);
    expect(screen.getByRole("region", { name: messages.turnMetrics.detailsLabel })).toHaveAttribute(
      "id",
      "turn-metrics-assistant-2",
    );
  });
});
