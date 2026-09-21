import { describe, expect, it } from "vitest";
import { turnMetricsByAssistantId } from "./turnMetricsBinding";
import type { MessageView, TurnMetrics } from "./types";

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

let tick = 0;
function stamp(): string {
  tick += 1;
  return `2026-08-28T15:00:${String(tick).padStart(2, "0")}Z`;
}

function user(id: string, overrides: Partial<MessageView> = {}): MessageView {
  return {
    id,
    role: "user",
    text: `pregunta ${id}`,
    status: "ok",
    createdAt: stamp(),
    materialIds: [],
    creationIds: [],
    ...overrides,
  };
}

function assistant(id: string, overrides: Partial<MessageView> = {}): MessageView {
  return {
    id,
    role: "assistant",
    text: `respuesta ${id}`,
    status: "ok",
    createdAt: stamp(),
    materialIds: [],
    creationIds: [],
    ...overrides,
  };
}

describe("turnMetricsByAssistantId durable binding", () => {
  it("M1: binds three turns exactly by durable turn identity", () => {
    const metricsA = metrics({ inputTokens: 111, sourceNames: ["source-A"] });
    const metricsB = metrics({ inputTokens: 222, sourceNames: ["source-B"] });
    const metricsC = metrics({ inputTokens: 333, sourceNames: ["source-C"] });
    const messages: MessageView[] = [
      user("uA", { turnMetrics: metricsA }),
      assistant("aA", { turnId: "uA" }),
      user("uB", { turnMetrics: metricsB }),
      assistant("aB", { turnId: "uB" }),
      user("uC", { turnMetrics: metricsC }),
      assistant("aC", { turnId: "uC" }),
    ];
    const map = turnMetricsByAssistantId(messages);
    expect(map.get("aA")).toBe(metricsA);
    expect(map.get("aB")).toBe(metricsB);
    expect(map.get("aC")).toBe(metricsC);
    expect(map.get("aA")?.inputTokens).toBe(111);
    expect(map.get("aA")?.sourceNames).toEqual(["source-A"]);
    expect(map.get("aB")?.inputTokens).toBe(222);
    expect(map.get("aB")?.sourceNames).toEqual(["source-B"]);
    expect(map.get("aC")?.inputTokens).toBe(333);
    expect(map.get("aC")?.sourceNames).toEqual(["source-C"]);
  });

  it("M2: fail→recover same turn keeps the recovered answer's metrics", () => {
    const metricsA = metrics({ inputTokens: 111 });
    const metricsB = metrics({ inputTokens: 222 });
    const messages: MessageView[] = [
      user("uA", { turnMetrics: metricsA }),
      assistant("aFail", { turnId: "uA", status: "failed", text: "falló" }),
      assistant("aOk", { turnId: "uA", text: "respuesta real" }),
      user("uB", { turnMetrics: metricsB }),
      assistant("aB", { turnId: "uB" }),
    ];
    const map = turnMetricsByAssistantId(messages);
    // The failed assistant must not consume the metrics; the recovered one gets
    // them and the next turn stays B.
    expect(map.get("aFail")).toBeUndefined();
    expect(map.get("aOk")).toBe(metricsA);
    expect(map.get("aB")).toBe(metricsB);
  });

  it("M3: a cancelled turn never lends metrics to an unrelated assistant", () => {
    const metricsA = metrics({ inputTokens: 111 });
    const messages: MessageView[] = [
      user("uA", { turnMetrics: metricsA }),
      assistant("aCancel", { turnId: "uA", status: "cancelled" }),
      assistant("aReal", { turnId: "uA", status: "ok" }),
    ];
    const map = turnMetricsByAssistantId(messages);
    expect(map.get("aCancel")).toBeUndefined();
    expect(map.get("aReal")).toBe(metricsA);
  });

  it("M4: explicit retry as a new user turn binds to the retry turn's metrics", () => {
    const original = metrics({ inputTokens: 111 });
    const retry = metrics({ inputTokens: 999 });
    const messages: MessageView[] = [
      user("uOrig", { turnMetrics: original }),
      assistant("aOrig", { turnId: "uOrig" }),
      user("uRetry", { turnMetrics: retry }),
      assistant("aRetry", { turnId: "uRetry" }),
    ];
    const map = turnMetricsByAssistantId(messages);
    expect(map.get("aRetry")?.inputTokens).toBe(999);
    expect(map.get("aOrig")?.inputTokens).toBe(111);
    expect(map.get("aRetry")).toBe(retry);
  });

  it("M5: K6 → NormalSemantic keeps each answer's own mode and sources", () => {
    const k6 = metrics({
      localMode: "selected_batch_aggregate",
      retrievalMode: null,
      sourceNames: ["k6-src.md"],
    });
    const rag = metrics({
      localMode: null,
      retrievalMode: "normal",
      sourceNames: ["rag-src.md"],
    });
    const messages: MessageView[] = [
      user("uK6", { turnMetrics: k6 }),
      assistant("aK6", { turnId: "uK6" }),
      user("uRag", { turnMetrics: rag }),
      assistant("aRag", { turnId: "uRag" }),
    ];
    const map = turnMetricsByAssistantId(messages);
    expect(map.get("aK6")?.localMode).toBe("selected_batch_aggregate");
    expect(map.get("aK6")?.retrievalMode).toBeNull();
    expect(map.get("aK6")?.sourceNames).toEqual(["k6-src.md"]);
    expect(map.get("aRag")?.retrievalMode).toBe("normal");
    expect(map.get("aRag")?.sourceNames).toEqual(["rag-src.md"]);
  });

  it("M6: NormalSemantic → K6 keeps each answer's own mode (no crossover)", () => {
    const rag = metrics({ retrievalMode: "normal", localMode: null });
    const k6 = metrics({ retrievalMode: null, localMode: "selected_batch_aggregate" });
    const messages: MessageView[] = [
      user("uRag", { turnMetrics: rag }),
      assistant("aRag", { turnId: "uRag" }),
      user("uK6", { turnMetrics: k6 }),
      assistant("aK6", { turnId: "uK6" }),
    ];
    const map = turnMetricsByAssistantId(messages);
    expect(map.get("aRag")?.retrievalMode).toBe("normal");
    expect(map.get("aK6")?.localMode).toBe("selected_batch_aggregate");
    expect(map.get("aK6")?.retrievalMode).toBeNull();
  });

  it("M7: local inventory cannot lend corpus/reduction to the next provider turn", () => {
    const inventory = metrics({
      source: "unavailable",
      localMode: "inventory",
      retrievalMode: null,
      corpusEstTokens: 5000,
      evidenceEstTokens: 0,
      inputTokens: null,
    });
    const provider = metrics({
      source: "provider_actual",
      retrievalMode: "normal",
      inputTokens: 777,
      corpusEstTokens: 1000,
      evidenceEstTokens: 200,
    });
    const messages: MessageView[] = [
      user("uInv", { turnMetrics: inventory }),
      assistant("aInv", { turnId: "uInv" }),
      user("uProv", { turnMetrics: provider }),
      assistant("aProv", { turnId: "uProv" }),
    ];
    const map = turnMetricsByAssistantId(messages);
    expect(map.get("aInv")).toBe(inventory);
    expect(map.get("aInv")?.inputTokens).toBeNull();
    expect(map.get("aProv")).toBe(provider);
    expect(map.get("aProv")?.inputTokens).toBe(777);
  });

  it("M8: provider turn cannot lend tokens to the following local inventory turn", () => {
    const provider = metrics({
      source: "provider_actual",
      inputTokens: 888,
      retrievalMode: "normal",
    });
    const inventory = metrics({
      source: "unavailable",
      localMode: "inventory",
      retrievalMode: null,
      inputTokens: null,
    });
    const messages: MessageView[] = [
      user("uProv", { turnMetrics: provider }),
      assistant("aProv", { turnId: "uProv" }),
      user("uInv", { turnMetrics: inventory }),
      assistant("aInv", { turnId: "uInv" }),
    ];
    const map = turnMetricsByAssistantId(messages);
    expect(map.get("aProv")?.inputTokens).toBe(888);
    expect(map.get("aInv")?.inputTokens).toBeNull();
    expect(map.get("aInv")).toBe(inventory);
  });

  it("M9: assistant↔turn association is exact across a reload round-trip", () => {
    const m1 = metrics({ inputTokens: 111, sourceNames: ["a.md"] });
    const m2 = metrics({ inputTokens: 222, sourceNames: ["b.md"] });
    const m3 = metrics({ inputTokens: 333, sourceNames: ["c.md"] });
    const messages: MessageView[] = [
      user("u1", { turnMetrics: m1 }),
      assistant("a1", { turnId: "u1" }),
      user("u2", { turnMetrics: m2 }),
      assistant("a2", { turnId: "u2" }),
      user("u3", { turnMetrics: m3 }),
      assistant("a3", { turnId: "u3" }),
    ];
    const before = turnMetricsByAssistantId(messages);
    // Simulate persist + fresh reload: JSON serialization keeps the durable
    // turnId identity, and the binding is a pure function of persisted data.
    const reloaded = JSON.parse(JSON.stringify(messages)) as MessageView[];
    const after = turnMetricsByAssistantId(reloaded);
    expect(after.get("a1")?.inputTokens).toBe(111);
    expect(after.get("a2")?.inputTokens).toBe(222);
    expect(after.get("a3")?.inputTokens).toBe(333);
    expect(after.get("a1")?.sourceNames).toEqual(["a.md"]);
    expect(after.get("a2")?.sourceNames).toEqual(["b.md"]);
    expect(after.get("a3")?.sourceNames).toEqual(["c.md"]);
    expect(before.get("a1")?.inputTokens).toBe(after.get("a1")?.inputTokens);
  });

  it("M10: legacy records without assistant turn id fall back conservatively", () => {
    const mA = metrics({ inputTokens: 111 });
    const mB = metrics({ inputTokens: 222 });
    // A single unambiguous legacy assistant after a user binds.
    const simple = turnMetricsByAssistantId([user("uA", { turnMetrics: mA }), assistant("aA")]);
    expect(simple.get("aA")).toBe(mA);

    // A legacy fail→recover ambiguity binds nothing.
    const ambiguous = turnMetricsByAssistantId([
      user("uA", { turnMetrics: mA }),
      assistant("aFail", { status: "failed" }),
      assistant("aOk"),
      user("uB", { turnMetrics: mB }),
      assistant("aB"),
    ]);
    expect(ambiguous.get("aFail")).toBeUndefined();
    expect(ambiguous.get("aOk")).toBeUndefined();
    expect(ambiguous.get("aB")).toBe(mB);
  });
});
