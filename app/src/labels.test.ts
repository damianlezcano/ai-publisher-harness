import { describe, expect, it } from "vitest";
import { kindIcon, kindLabel, visibilityLabel } from "./labels";

describe("labels", () => {
  it("maps creation kinds to human-readable Spanish labels", () => {
    expect(kindLabel("web")).toBe("Actividad interactiva");
    expect(kindLabel("document")).toBe("Documento");
    expect(kindLabel("image")).toBe("Imagen");
    expect(kindLabel("file")).toBe("Archivo");
    expect(kindLabel("unknown")).toBe("Archivo");
    expect(kindIcon("web")).toBe("🎮");
    expect(kindIcon("image")).toBe("🖼️");
    expect(kindIcon("unknown")).toBe("📎");
  });

  it("maps visibility to product language", () => {
    expect(visibilityLabel("public")).toBe("Se compartirá");
    expect(visibilityLabel("private")).toBe("Privado");
  });
});
