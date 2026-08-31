import { useState } from "react";
import { api, errorMessage } from "../api";
import type { ProjectSummary } from "../types";
import ConfirmDialog from "./ConfirmDialog";
import { messages } from "../messages";

interface ProjectsViewProps {
  projects: ProjectSummary[];
  onRefresh: () => Promise<void>;
  onOpen: (id: string) => void;
}

export default function ProjectsView({ projects, onRefresh, onOpen }: ProjectsViewProps) {
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");
  const [deleting, setDeleting] = useState<ProjectSummary | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function create() {
    setBusy(true);
    setError(null);
    try {
      const created = await api.projectCreate(name);
      setCreating(false);
      setName("");
      await onRefresh();
      onOpen(created.id);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function rename(id: string) {
    setBusy(true);
    setError(null);
    try {
      await api.projectRename(id, editingName);
      setEditingId(null);
      await onRefresh();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!deleting) return;
    setBusy(true);
    setError(null);
    try {
      await api.projectDelete(deleting.id);
      setDeleting(null);
      await onRefresh();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="view">
      <header className="view-header">
        <h1>{messages.project.listHeading}</h1>
        <button type="button" className="primary" onClick={() => setCreating((v) => !v)}>
          {messages.project.newButton}
        </button>
      </header>

      {creating && (
        <form
          className="inline-form"
          onSubmit={(e) => {
            e.preventDefault();
            void create();
          }}
        >
          <label className="sr-only" htmlFor="new-project-name">
            {messages.project.nameLabel}
          </label>
          <input
            id="new-project-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder={messages.project.namePlaceholder}
            autoFocus
          />
          <button type="submit" className="primary" disabled={busy || name.trim() === ""}>
            {messages.common.create}
          </button>
          <button type="button" className="secondary" onClick={() => setCreating(false)}>
            {messages.common.cancel}
          </button>
        </form>
      )}

      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}

      {projects.length === 0 && !creating ? (
        <p className="muted">{messages.project.empty.title}</p>
      ) : (
        <ul className="project-list" aria-label={messages.project.listAriaLabel}>
          {projects.map((project) => (
            <li key={project.id} className="project-row">
              {editingId === project.id ? (
                <form
                  className="inline-form"
                  onSubmit={(e) => {
                    e.preventDefault();
                    void rename(project.id);
                  }}
                >
                  <label className="sr-only" htmlFor={`rename-${project.id}`}>
                    {messages.project.renameLabel}
                  </label>
                  <input
                    id={`rename-${project.id}`}
                    value={editingName}
                    onChange={(e) => setEditingName(e.target.value)}
                    autoFocus
                  />
                  <button
                    type="submit"
                    className="primary"
                    disabled={busy || editingName.trim() === ""}
                  >
                    {messages.common.save}
                  </button>
                  <button type="button" className="secondary" onClick={() => setEditingId(null)}>
                    {messages.common.cancel}
                  </button>
                </form>
              ) : (
                <>
                  <span className="project-name">{project.name}</span>
                  <span className="row-actions">
                    <button type="button" className="primary" onClick={() => onOpen(project.id)}>
                      {messages.project.open}
                    </button>
                    <button
                      type="button"
                      className="secondary"
                      onClick={() => {
                        setEditingId(project.id);
                        setEditingName(project.name);
                      }}
                    >
                      {messages.project.rename}
                    </button>
                    <button type="button" className="danger" onClick={() => setDeleting(project)}>
                      {messages.common.delete}
                    </button>
                  </span>
                </>
              )}
            </li>
          ))}
        </ul>
      )}

      {deleting && (
        <ConfirmDialog
          title={messages.project.delete.title}
          message={messages.project.delete.confirmMessage(deleting.name)}
          confirmText={deleting.name}
          busy={busy}
          onCancel={() => setDeleting(null)}
          onConfirm={() => void remove()}
        />
      )}
    </div>
  );
}
