const DRAFT_STORAGE_KEY = "educai.drafts.v1";

export function loadDrafts(): Record<string, string> {
  try {
    const raw = localStorage.getItem(DRAFT_STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return {};
    const drafts: Record<string, string> = {};
    for (const [key, value] of Object.entries(parsed)) {
      if (typeof value === "string" && value !== "") drafts[key] = value;
    }
    return drafts;
  } catch {
    return {};
  }
}

export function saveDrafts(drafts: Record<string, string>): void {
  try {
    localStorage.setItem(DRAFT_STORAGE_KEY, JSON.stringify(drafts));
  } catch {
    // Storage failures (quota, private mode) must not break drafting.
  }
}
