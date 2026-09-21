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
  it("renders a compact line with provider-actual tokens and Knowledge reduction", () => {
    renderMetrics(
      metrics({
        source: "provider_actual",
        inputTokens: 2173,
        outputTokens: 487,
        turnDurationMs: 9900,
        corpusEstTokens: 893108,
        evidenceEstTokens: 2696,
        retrievalMode: "normal",
      }),
    );
    const segments = screen.getByText(/Knowledge −/);
    expect(segments.textContent).toContain("2.173");
    expect(segments.textContent).toContain("487");
    expect(segments.textContent).toContain("9,9 s");
  });

  it("omits input/output tokens when provider usage is not provider-actual", () => {
    renderMetrics(metrics({ source: "unavailable" }));
    const segments = document.querySelector(".turn-metrics-segments");
    expect(segments?.textContent).not.toContain("↑");
    expect(segments?.textContent).not.toContain("↓");
  });

  it("omits the Knowledge reduction segment for a non-RAG local turn", () => {
    renderMetrics(metrics({ localMode: "inventory", source: "unavailable" }));
    expect(document.querySelector(".turn-metrics-segments")?.textContent).not.toContain(
      "Knowledge",
    );
  });

  it("does not show provider tokens as a fabricated estimate when absent", () => {
    renderMetrics(
      metrics({
        source: "provider_actual",
        inputTokens: null,
        outputTokens: null,
        corpusEstTokens: 100,
        evidenceEstTokens: 10,
        retrievalMode: "normal",
      }),
    );
    const segments = document.querySelector(".turn-metrics-segments")?.textContent ?? "";
    expect(segments).not.toContain("↑");
    expect(segments).not.toContain("↓");
    expect(segments).toContain("Knowledge −");
  });

  it("never renders a Fuentes block in the assistant metrics line", () => {
    const { container } = renderMetrics(
      metrics({ sourceNames: ["file-a.md", "file-b.md"], source: "provider_actual" }),
    );
    expect(container.textContent).not.toContain("Fuentes:");
  });
});

describe("Knowledge reduction gating", () => {
  it("R1: omits Knowledge −100% for an inventory/local non-RAG turn with corpus>0 and evidence=0", () => {
    renderMetrics(
      metrics({
        source: "unavailable",
        localMode: "inventory",
        retrievalMode: null,
        corpusEstTokens: 5000,
        evidenceEstTokens: 0,
        contextReductionPct: 0,
      }),
    );
    const segments = document.querySelector(".turn-metrics-segments")?.textContent ?? "";
    expect(segments).not.toContain("Knowledge −100%");
    expect(segments).not.toContain("Knowledge");
  });

  it("R1b: popover does not call an inventory turn a context reduction", async () => {
    renderMetrics(
      metrics({
        source: "unavailable",
        localMode: "inventory",
        retrievalMode: null,
        corpusEstTokens: 5000,
        evidenceEstTokens: 0,
        contextReductionPct: 0,
      }),
    );
    await userEvent.click(infoButton());
    const panel = screen.getByRole("region", { name: messages.turnMetrics.detailsLabel });
    const percentages = Array.from(panel.querySelectorAll("dd")).filter((dd) =>
      dd.textContent?.includes("%"),
    );
    expect(percentages).toHaveLength(0);
  });

  it("R2: NormalSemantic with corpus > context shows the correct reduction", () => {
    renderMetrics(
      metrics({
        source: "provider_actual",
        retrievalMode: "normal",
        corpusEstTokens: 1000,
        evidenceEstTokens: 250,
      }),
    );
    const segments = document.querySelector(".turn-metrics-segments")?.textContent ?? "";
    expect(segments).toContain("Knowledge −75%");
  });

  it("R3: K6 selected_batch_aggregate with corpus present shows no reduction", () => {
    renderMetrics(
      metrics({
        source: "provider_actual",
        retrievalMode: null,
        localMode: "selected_batch_aggregate",
        corpusEstTokens: 5000,
        evidenceEstTokens: 0,
      }),
    );
    const segments = document.querySelector(".turn-metrics-segments")?.textContent ?? "";
    expect(segments).not.toContain("Knowledge");
  });

  it("R4: zero/None corpus yields no reduction and no divide-by-zero", () => {
    renderMetrics(
      metrics({
        source: "provider_actual",
        retrievalMode: "normal",
        corpusEstTokens: 0,
        evidenceEstTokens: 0,
      }),
    );
    const segments = document.querySelector(".turn-metrics-segments")?.textContent ?? "";
    expect(segments).not.toContain("Knowledge");
    expect(segments).not.toContain("NaN");
  });

  it("R5: prefers the persisted contextReductionPct for a RAG turn", () => {
    renderMetrics(
      metrics({
        source: "provider_actual",
        retrievalMode: "normal",
        corpusEstTokens: 1000,
        evidenceEstTokens: 250,
        contextReductionPct: 75,
      }),
    );
    const segments = document.querySelector(".turn-metrics-segments")?.textContent ?? "";
    expect(segments).toContain("Knowledge −75%");
  });
});

describe("AssistantMetrics popover", () => {
  it("opens on click and lists the grounded sources for that response", async () => {
    renderMetrics(
      metrics({
        source: "provider_actual",
        inputTokens: 2173,
        outputTokens: 487,
        retrievalMode: "normal",
        selectedEvidenceCount: 8,
        sourceNames: ["file-a.md", "file-b.md"],
      }),
    );
    await userEvent.click(infoButton());
    const panel = screen.getByRole("region", { name: messages.turnMetrics.detailsLabel });
    expect(panel).toHaveTextContent("file-a.md");
    expect(panel).toHaveTextContent("file-b.md");
    expect(panel).toHaveTextContent("2.173 tokens");
    expect(panel).toHaveTextContent("487 tokens");
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

  it("treats an omitted sourceNames field as an empty source list", async () => {
    const m = metrics({ source: "provider_actual" });
    delete (m as Partial<TurnMetrics>).sourceNames;
    renderMetrics(m);
    await userEvent.click(infoButton());
    const panel = screen.getByRole("region", { name: messages.turnMetrics.detailsLabel });
    expect(panel).toHaveTextContent(messages.turnMetrics.sourcesHeading);
    expect(panel).toHaveTextContent(messages.conversationDetails.metrics.unavailable);
  });

  it("labels Knowledge reduction as context reduction, never monetary savings", async () => {
    renderMetrics(
      metrics({
        source: "provider_actual",
        costUsd: 0.012,
        corpusEstTokens: 893108,
        evidenceEstTokens: 2696,
        retrievalMode: "normal",
      }),
    );
    const line = document.querySelector(".turn-metrics-segments");
    expect(line?.textContent).toContain("Knowledge −");
    expect(line?.textContent).not.toMatch(/\$/);
    await userEvent.click(infoButton());
    const panel = screen.getByRole("region", { name: messages.turnMetrics.detailsLabel });
    expect(panel).toHaveTextContent(messages.turnMetrics.contextReduction);
    // The reduction value is a percentage, not a currency amount.
    const reductionValue = Array.from(panel.querySelectorAll("dd")).find((dd) =>
      dd.textContent?.includes("%"),
    );
    expect(reductionValue?.textContent).not.toMatch(/\$|USD/);
  });

  it("shows the retrieval mode truthfully and never mislabels a K6 turn as normal", async () => {
    renderMetrics(
      metrics({
        source: "provider_actual",
        remoteCalls: 5,
        localMode: "selected_batch_aggregate",
        retrievalMode: null,
      }),
    );
    await userEvent.click(infoButton());
    const panel = screen.getByRole("region", { name: messages.turnMetrics.detailsLabel });
    expect(panel).toHaveTextContent("selected_batch_aggregate");
    // No retrieval mode "normal" is claimed for a K6 summary turn.
    expect(panel).not.toHaveTextContent("normal");
  });

  it("P2: stays open when the user clicks inside the panel", async () => {
    renderMetrics(metrics({ source: "provider_actual", sourceNames: ["file-a.md"] }));
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
    renderMetrics(metrics({ source: "provider_actual", sourceNames: ["file-a.md"] }));
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
