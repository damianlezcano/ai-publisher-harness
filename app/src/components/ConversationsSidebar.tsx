import { useCallback, useState } from "react";
import { api, errorMessage } from "../api";
import type { ProjectSummary } from "../types";
import { humanDate, messages } from "../messages";

interface ConversationsSidebarProps {
  conversations: ProjectSummary[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onRefresh: () => Promise<void>;
}

export default function ConversationsSidebar({
  conversations,
  selectedId,
  onSelect,
  onRefresh,
}: ConversationsSidebarProps) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const startRename = useCallback((project: ProjectSummary) => {
    setEditingId(project.id);
    setEditingName(project.name);
    setError(null);
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

  return (
    <nav className="conversations-sidebar" aria-label={messages.conversations.listAriaLabel}>
      <div className="conversations-sidebar-header">
        <h2 className="conversations-sidebar-title">{messages.conversations.title}</h2>
        <button
          type="button"
          className="primary conversations-new-button"
          onClick={() => void createConversation()}
          disabled={busy}
        >
          {messages.conversations.newButton}
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
                  <button
                    type="button"
                    className="secondary conversation-rename-button"
                    aria-label={messages.conversations.renameAriaLabel}
                    onClick={() => startRename(conversation)}
                  >
                    {messages.project.rename}
                  </button>
                </>
              )}
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
