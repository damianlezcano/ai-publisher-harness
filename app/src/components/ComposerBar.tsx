import { useEffect, useRef, useState } from "react";
import { api, type StagedImagePayload } from "../api";
import type { AgentPhase, MaterialView, StagedAttachmentView } from "../types";
import { messages } from "../messages";
import ErrorNotice from "./ui/ErrorNotice";

export interface ComposerBarProps {
  materials: MaterialView[];
  agentPhase: AgentPhase;
  aiUsable: boolean;
  onSend: (
    prompt: string,
    attachmentIds: string[],
    stagedImages?: StagedImagePayload[],
  ) => void | Promise<void>;
  onCancel: () => void | Promise<void>;
  onOpenProvider?: () => void;
  shareAction?: React.ReactNode;
  attachmentIds?: string[];
  attachmentNames?: Map<string, StagedAttachmentView>;
  onAttachmentIdsChange?: (ids: string[]) => void;
  prompt?: string;
  onPromptChange?: (value: string) => void;
}

const TEXTAREA_MAX_HEIGHT_PX = 150;
const ATTACHMENT_COLLAPSE_THRESHOLD = 5;

function clipboardHasImage(items: DataTransferItemList): DataTransferItem | null {
  for (let i = 0; i < items.length; i++) {
    const item = items[i];
    if (item.kind === "file" && item.type.startsWith("image/")) {
      return item;
    }
  }
  return null;
}

export default function ComposerBar({
  materials,
  agentPhase,
  aiUsable,
  onSend,
  onCancel,
  shareAction,
  attachmentIds: attachmentIdsProp,
  attachmentNames,
  onAttachmentIdsChange,
  prompt: promptProp,
  onPromptChange,
}: ComposerBarProps) {
  const [internalPrompt, setInternalPrompt] = useState("");
  const [internalAttachmentIds, setInternalAttachmentIds] = useState<string[]>([]);
  const [internalAttachmentNames, setInternalAttachmentNames] = useState<
    Map<string, StagedAttachmentView>
  >(new Map());
  // Clipboard bytes remain in this renderer-owned map until Send. The opaque
  // key is only a composer selection handle, never a project Material ID.
  const [stagedImages, setStagedImages] = useState<Map<string, StagedImagePayload>>(new Map());
  const [pasteBusy, setPasteBusy] = useState(false);
  const [attachmentsExpanded, setAttachmentsExpanded] = useState(false);

  const controlled = promptProp !== undefined && onPromptChange !== undefined;
  const prompt = controlled ? promptProp : internalPrompt;

  function setPrompt(value: string) {
    if (controlled) {
      onPromptChange(value);
    } else {
      setInternalPrompt(value);
    }
  }

  const controlledAttachments = attachmentIdsProp !== undefined;
  const attachmentIds = controlledAttachments ? attachmentIdsProp : internalAttachmentIds;

  function setAttachmentIds(next: string[] | ((prev: string[]) => string[])) {
    const resolved = typeof next === "function" ? next(attachmentIds) : next;
    if (controlledAttachments) {
      onAttachmentIdsChange?.(resolved);
    } else {
      setInternalAttachmentIds(resolved);
    }
  }
  const [pickError, setPickError] = useState<unknown | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const working = agentPhase === "working";
  const composerDisabled = working || pasteBusy || !aiUsable;

  function resizeTextarea() {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    const nextHeight = Math.min(el.scrollHeight, TEXTAREA_MAX_HEIGHT_PX);
    el.style.height = `${nextHeight}px`;
    el.style.overflowY = el.scrollHeight > TEXTAREA_MAX_HEIGHT_PX ? "auto" : "hidden";
  }

  useEffect(() => {
    resizeTextarea();
  }, [prompt]);

  const materialById = new Map(materials.map((m) => [m.id, m]));

  function removeAttachment(materialId: string) {
    setAttachmentIds((prev) => prev.filter((id) => id !== materialId));
    setStagedImages((previous) => {
      if (!previous.has(materialId)) return previous;
      const next = new Map(previous);
      next.delete(materialId);
      return next;
    });
    setInternalAttachmentNames((previous) => {
      if (!previous.has(materialId)) return previous;
      const next = new Map(previous);
      next.delete(materialId);
      return next;
    });
  }

  async function pickFile() {
    if (composerDisabled) return;
    setPickError(null);
    try {
      const path = await api.pickFile();
      if (!path) return;
      const staged = await api.attachmentsStagePaths([path]);
      if (staged.items[0]?.status === "ready") {
        setInternalAttachmentNames((previous) => {
          const next = new Map(previous);
          next.set(path, staged.items[0]!);
          return next;
        });
        setAttachmentIds((prev) => (prev.includes(path) ? prev : [...prev, path]));
      } else {
        setPickError({ code: "attachment_invalid", message: staged.items[0]?.reason ?? "" });
      }
    } catch (err) {
      setPickError(err);
    }
  }

  async function handlePaste(event: React.ClipboardEvent<HTMLTextAreaElement>) {
    const imageItem = clipboardHasImage(event.clipboardData.items);
    if (!imageItem) return;

    event.preventDefault();
    const file = imageItem.getAsFile();
    if (!file) return;

    setPasteBusy(true);
    try {
      const buffer = await file.arrayBuffer();
      const fileName = file.name || `captura-${Date.now()}.png`;
      const stagingId = `clipboard-${crypto.randomUUID()}`;
      const staged: StagedImagePayload = {
        stagingId,
        fileName,
        contentType: file.type || imageItem.type,
        data: new Uint8Array(buffer),
      };
      setStagedImages((previous) => new Map(previous).set(stagingId, staged));
      setInternalAttachmentNames((previous) => {
        const next = new Map(previous);
        next.set(stagingId, { sourceName: fileName, status: "ready" });
        return next;
      });
      setAttachmentIds((prev) => [...prev, stagingId]);
    } catch (err) {
      setPickError(err);
    } finally {
      setPasteBusy(false);
    }
  }

  async function send() {
    const text = prompt.trim();
    if (text === "" || composerDisabled) return;
    const ids = attachmentIds;
    const images = ids.flatMap((id) => {
      const image = stagedImages.get(id);
      return image ? [image] : [];
    });
    const submitted = prompt;
    setPrompt("");
    try {
      if (images.length > 0) {
        await onSend(text, ids, images);
      } else {
        await onSend(text, ids);
      }
      // `agent_send` resolves only after the user turn is durably accepted.
      // Clear the prompt optimistically; keep the pending selection on a
      // pre-acceptance failure so the person can retry. Once accepted,
      // Materials are already durable project state; these ids must no longer
      // describe attachments for the next turn.
      setAttachmentIds([]);
      setStagedImages(new Map());
      setInternalAttachmentNames(new Map());
      setAttachmentsExpanded(false);
    } catch {
      // WorkspaceView owns the user-facing failure and retry state. The pending
      // attachment selection intentionally remains untouched here. Restore the
      // draft text too so a failed send never silently destroys it.
      setPrompt(submitted);
    }
  }

  function handlePromptKeyDown(event: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key !== "Enter") return;
    if (event.nativeEvent.isComposing || event.keyCode === 229) return;
    if (event.shiftKey) return;
    event.preventDefault();
    void send();
  }

  function renderAttachmentList() {
    const compact = !attachmentsExpanded && attachmentIds.length > ATTACHMENT_COLLAPSE_THRESHOLD;
    const visibleIds = compact ? [] : attachmentIds;
    return (
      <div
        className={`composer-attachments${compact ? " is-collapsed" : ""}`}
        role="group"
        aria-label={messages.assistant.selectedCount(attachmentIds.length)}
      >
        {attachmentIds.length > ATTACHMENT_COLLAPSE_THRESHOLD && (
          <button
            type="button"
            className="ghost composer-attachment-toggle"
            onClick={() => setAttachmentsExpanded((expanded) => !expanded)}
          >
            {compact
              ? `${messages.assistant.selectedCount(attachmentIds.length)} · ${messages.assistant.showAll}`
              : messages.assistant.hideAll}
          </button>
        )}
        <ul className="chip-list" aria-label={messages.assistant.attachmentsAriaLabel}>
          {visibleIds.map((id) => {
            const material = materialById.get(id);
            const staged = attachmentNames?.get(id) ?? internalAttachmentNames.get(id);
            const name =
              material?.displayName ?? staged?.sourceName ?? messages.assistant.attachmentFallback;
            return (
              <li key={id} className="chip">
                <span>
                  {name}
                  {staged?.status === "duplicate_in_selection"
                    ? " · repetido en esta selección"
                    : ""}
                </span>
                <button
                  type="button"
                  className="chip-remove"
                  aria-label={messages.assistant.removeAttachment(name)}
                  disabled={composerDisabled}
                  onClick={() => removeAttachment(id)}
                >
                  ×
                </button>
              </li>
            );
          })}
        </ul>
      </div>
    );
  }

  return (
    <div className="composer-bar" role="region" aria-label={messages.assistant.panelLabel}>
      {pickError !== null && <ErrorNotice error={pickError} />}
      {attachmentIds.length > 0 && renderAttachmentList()}

      <form
        className="composer-form"
        onSubmit={(e) => {
          e.preventDefault();
          void send();
        }}
      >
        <div className="composer-primary">
          <button
            type="button"
            className="ghost composer-attach"
            aria-label={messages.assistant.attachMaterial}
            disabled={composerDisabled}
            onClick={() => void pickFile()}
          >
            <span aria-hidden="true">📎</span>
          </button>
          <label className="sr-only" htmlFor="composer-prompt">
            {messages.assistant.promptLabel}
          </label>
          <textarea
            ref={textareaRef}
            id="composer-prompt"
            className="composer-textarea"
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            onKeyDown={handlePromptKeyDown}
            onPaste={(e) => void handlePaste(e)}
            placeholder={messages.assistant.placeholder}
            rows={1}
            disabled={composerDisabled}
          />
          {working ? (
            <button type="button" className="danger" onClick={() => void onCancel()}>
              {messages.common.cancel}
            </button>
          ) : (
            <button
              type="submit"
              className="primary composer-send"
              disabled={prompt.trim() === "" || composerDisabled}
            >
              {messages.common.send}
            </button>
          )}
        </div>

        {shareAction && (
          <div className="composer-secondary">
            <div className="composer-share-slot">{shareAction}</div>
          </div>
        )}
      </form>
    </div>
  );
}
