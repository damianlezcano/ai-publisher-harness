import { describe, expect, it } from "vitest";
import { errorGuidance, guidanceFromError } from "./guidance";
import type { GuidanceActionKind } from "./guidance";

const EXPECTED_ACTIONS: Array<[string, GuidanceActionKind[]]> = [
  ["ai_unavailable", ["retry"]],
  ["ai_task_failed", ["retry"]],
  ["publish_failed", ["retry"]],
  ["network_error", ["retry"]],
  ["material_failed", []],
  ["material_unsupported", []],
  ["material_duplicate", []],
  ["preview_unavailable", ["open-with-app"]],
  ["preview_too_large", ["open-with-app"]],
  ["credential_revoked", ["connect-ai"]],
  ["credential_invalid", ["connect-ai"]],
  ["provider_unavailable", ["connect-ai"]],
  ["no_compatible_model", ["connect-ai"]],
  ["model_unavailable", ["connect-ai"]],
  ["open_failed", ["retry"]],
  ["storage_unavailable", ["retry"]],
  ["internal", ["retry"]],
];

describe("errorGuidance", () => {
  it.each(EXPECTED_ACTIONS)("maps %s to actions %j", (code, actions) => {
    const guidance = errorGuidance(code);
    expect(guidance.actions).toEqual(actions);
    expect(guidance.title).toBeTruthy();
    expect(guidance.message).toBeTruthy();
  });

  it("composes the hint into the message when present", () => {
    expect(errorGuidance("ai_unavailable").message).toContain("reiniciá la aplicación");
    expect(errorGuidance("publish_failed").message).toContain("comprobá tu conexión a Internet");
    expect(errorGuidance("internal").message).toContain("reiniciá la aplicación");
  });

  it("maps storage_unavailable and internal to the generic copy", () => {
    expect(errorGuidance("storage_unavailable").title).toBe("Algo salió mal.");
    expect(errorGuidance("internal").title).toBe("Algo salió mal.");
  });

  it("keeps per-file material guidance free of a global retry", () => {
    expect(errorGuidance("material_failed").title).toBe("No pudimos agregar ese archivo.");
    expect(errorGuidance("material_failed").actions).toEqual([]);
  });

  it("falls back to the internal guidance for unknown codes", () => {
    const guidance = errorGuidance("some_unknown_code");
    expect(guidance.title).toBe("Algo salió mal.");
    expect(guidance.actions).toEqual(["retry"]);
  });
});

describe("guidanceFromError", () => {
  it("reads a string code field", () => {
    const guidance = guidanceFromError({ code: "publish_failed", message: "x" });
    expect(guidance.actions).toEqual(["retry"]);
    expect(guidance.title).toBe("No pudimos compartir en este momento.");
  });

  it("falls back when the error has no code field", () => {
    expect(guidanceFromError({ message: "no code" }).title).toBe("Algo salió mal.");
    expect(guidanceFromError("plain string").title).toBe("Algo salió mal.");
    expect(guidanceFromError(null).title).toBe("Algo salió mal.");
    expect(guidanceFromError(undefined).title).toBe("Algo salió mal.");
    expect(guidanceFromError(42).title).toBe("Algo salió mal.");
  });
});
