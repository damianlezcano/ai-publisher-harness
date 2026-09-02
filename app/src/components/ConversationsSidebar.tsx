import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorMessage } from "../api";
import type { AgentPhase, ProjectSummary } from "../types";
import { humanDate, messages } from "../messages";
import ConfirmDialog from "./ConfirmDialog";

interface ConversationsSidebarProps {
  conversations: ProjectSummary[];
  selectedId: string | null;
  agentPhase?: AgentPhase;
  onSelect: (id: string) => void;
  onRefresh: () => Promise<void>;
  onDelete?: (id: string) => Promise<void>;
}

export default function ConversationsSidebar({
  conversations,
  selectedId,
  agentPhase,
  onSelect,
  onRefresh,
  onDelete,
}: ConversationsSidebarProps) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [menuOpenId, setMenuOpenId] = useState<string | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const startRename = useCallback((project: ProjectSummary) => {
    setEditingId(project.id);
    setEditingName(project.name);
    setError(null);
    setMenuOpenId(null);
  }, []);

  const cancelRename = useCallback(() => {
    setEditingId(null);
    setEditingName("");
  }, []);

  const commitRename = useCallback(
    async (id: string) => {
      const name = editingName.trim();
      if (name === "" || busy) return;
      setBusy(true);
      setError(null);
      try {
        await api.projectRename(id, name);
        setEditingId(null);
        await onRefresh();
      } catch (err) {
        setError(errorMessage(err));
      } finally {
        setBusy(false);
      }
    },
    [editingName, busy, onRefresh],
  );

  const createConversation = useCallback(async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const created = await api.projectCreate(messages.conversation.defaultName);
      await onRefresh();
      onSelect(created.id);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }, [busy, onRefresh, onSelect]);

  const startDelete = useCallback((project: ProjectSummary) => {
    setConfirmDeleteId(project.id);
    setMenuOpenId(null);
  }, []);

  const commitDelete = useCallback(
    async (id: string) => {
      if (busy) return;
      setBusy(true);
      setError(null);
      try {
        if (onDelete) {
          await onDelete(id);
        } else {
          await api.projectDelete(id);
          await onRefresh();
        }
        setConfirmDeleteId(null);
      } catch (err) {
        setError(errorMessage(err));
      } finally {
        setBusy(false);
      }
    },
    [busy, onDelete, onRefresh],
  );

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "n" || !(event.ctrlKey || event.metaKey) || event.repeat) return;
      const target = event.target as HTMLElement | null;
      if (target?.closest("input, textarea, select, [role='dialog']")) return;
      event.preventDefault();
      void createConversation();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [createConversation]);

  useEffect(() => {
    if (!menuOpenId) return;
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") setMenuOpenId(null);
    }
    function handleClick(event: MouseEvent) {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        setMenuOpenId(null);
      }
    }
    document.addEventListener("keydown", handleKeyDown);
    document.addEventListener("mousedown", handleClick);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
      document.removeEventListener("mousedown", handleClick);
    };
  }, [menuOpenId]);

  const confirmProject = conversations.find((c) => c.id === confirmDeleteId) ?? null;

  return (
    <nav className="conversations-sidebar" aria-label={messages.conversations.listAriaLabel}>
      <div className="conversations-sidebar-header">
        <h2 className="conversations-sidebar-title">{messages.conversations.title}</h2>
        <button
          type="button"
          className="ghost conversations-new-button"
          onClick={() => void createConversation()}
          disabled={busy}
          aria-label={messages.conversations.newButton}
          title={messages.conversations.newButton}
        >
          <span aria-hidden="true">+</span>
        </button>
      </div>

      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}

      <ul className="conversations-list">
        {conversations.map((conversation) => {
          const isSelected = conversation.id === selectedId;
          const isEditing = editingId === conversation.id;
          const isMenuOpen = menuOpenId === conversation.id;
          const isGenerating = isSelected && agentPhase === "working";

          return (
            <li key={conversation.id} className="conversation-item">
              {isEditing ? (
                <form
                  className="conversation-rename-form"
                  onSubmit={(e) => {
                    e.preventDefault();
                    void commitRename(conversation.id);
                  }}
                >
                  <label className="sr-only" htmlFor={`rename-${conversation.id}`}>
                    {messages.conversations.renameLabel}
                  </label>
                  <input
                    id={`rename-${conversation.id}`}
                    value={editingName}
                    onChange={(e) => setEditingName(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Escape") {
                        e.preventDefault();
                        cancelRename();
                      }
                    }}
                    onFocus={(e) => e.target.select()}
                    disabled={busy}
                    autoFocus
                  />
                  <button
                    type="submit"
                    className="primary"
                    disabled={busy || editingName.trim() === ""}
                  >
                    {messages.common.save}
                  </button>
                  <button
                    type="button"
                    className="secondary"
                    onClick={cancelRename}
                    disabled={busy}
                  >
                    {messages.common.cancel}
                  </button>
                </form>
              ) : (
                <>
                  <button
                    type="button"
                    className={`conversation-select${isSelected ? " selected" : ""}`}
                    aria-current={isSelected ? "page" : undefined}
                    onClick={() => onSelect(conversation.id)}
                    onDoubleClick={() => startRename(conversation)}
                  >
                    <span className="conversation-name">{conversation.name}</span>
                    <span className="conversation-meta">
                      {conversation.shared && (
                        <span className="badge ok conversation-shared-badge">
                          {messages.conversations.sharedLabel}
                        </span>
                      )}
                      <span className="conversation-timestamp">
                        {humanDate(conversation.updatedAt)}
                      </span>
                    </span>
                  </button>
                  <div className="conversation-menu" ref={isMenuOpen ? menuRef : undefined}>
                    <button
                      type="button"
                      className="ghost conversation-menu-button"
                      aria-label={messages.conversations.menuAriaLabel}
                      aria-expanded={isMenuOpen}
                      aria-haspopup="menu"
                      title={messages.conversations.menuAriaLabel}
                      disabled={busy}
                      onClick={() =>
                        setMenuOpenId((id) => (id === conversation.id ? null : conversation.id))
                      }
                    >
                      <span aria-hidden="true">…</span>
                    </button>
                    {isMenuOpen && (
                      <div
                        className="conversation-menu-dropdown"
                        role="menu"
                        aria-label={messages.conversations.menuAriaLabel}
                      >
                        <button
                          type="button"
                          role="menuitem"
                          onClick={() => startRename(conversation)}
                        >
                          {messages.conversations.renameAction}
                        </button>
                        <button
                          type="button"
                          role="menuitem"
                          className="danger"
                          disabled={isGenerating}
                          title={
                            isGenerating
                              ? messages.conversations.deleteDisabledGenerating
                              : undefined
                          }
                          onClick={() => startDelete(conversation)}
                        >
                          {messages.conversations.deleteAction}
                        </button>
                      </div>
                    )}
                  </div>
                </>
              )}
            </li>
          );
        })}
      </ul>

      {confirmProject && (
        <ConfirmDialog
          title={messages.conversations.deleteConfirmTitle}
          message={messages.conversations.deleteConfirmBody}
          confirmPrompt={messages.common.confirmPrompt}
          busy={busy}
          onCancel={() => setConfirmDeleteId(null)}
          onConfirm={() => void commitDelete(confirmProject.id)}
        />
      )}
    </nav>
  );
}
