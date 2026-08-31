import { useEffect, useRef, useState } from "react";
import { api, errorMessage } from "../api";
import type { ProjectSummary } from "../types";
import ConfirmDialog from "./ConfirmDialog";
import EmptyState from "./ui/EmptyState";
import { messages } from "../messages";

const FIRST_RUN_DISMISSED_KEY = "educai.firstRunDismissed";

interface ProjectsViewProps {
  projects: ProjectSummary[];
  onRefresh: () => Promise<void>;
  onOpen: (id: string) => void;
}

export default function ProjectsView({ projects, onRefresh, onOpen }: ProjectsViewProps) {
  const viewRef = useRef<HTMLDivElement>(null);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");
  const [deleting, setDeleting] = useState<ProjectSummary | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [firstRunDismissed, setFirstRunDismissed] = useState(
    () => localStorage.getItem(FIRST_RUN_DISMISSED_KEY) === "1",
  );

  const showFirstRunGuide = projects.length === 0 && !firstRunDismissed;

  function openCreateForm() {
    setCreating(true);
    setName(messages.project.defaultName);
  }

  function closeCreateForm() {
    setCreating(false);
    setName("");
  }

  function dismissFirstRun() {
    localStorage.setItem(FIRST_RUN_DISMISSED_KEY, "1");
    setFirstRunDismissed(true);
  }

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;

    function onKeyDown(event: KeyboardEvent) {
      if (deleting) return;

      if (event.key === "Escape") {
        if (creating) {
          event.preventDefault();
          closeCreateForm();
        } else if (editingId) {
          event.preventDefault();
          setEditingId(null);
        }
        return;
      }

      if (event.key === "n" && (event.ctrlKey || event.metaKey)) {
        if (!creating && !editingId) {
          event.preventDefault();
          openCreateForm();
        }
      }
    }

    view.addEventListener("keydown", onKeyDown);
    return () => view.removeEventListener("keydown", onKeyDown);
  }, [creating, editingId, deleting]);

  async function create() {
    setBusy(true);
    setError(null);
    try {
      const created = await api.projectCreate(name);
      closeCreateForm();
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
    <div className="view" ref={viewRef}>
      <header className="view-header">
        <h1>{messages.project.listHeading}</h1>
        <button
          type="button"
          className="primary"
          onClick={() => (creating ? closeCreateForm() : openCreateForm())}
        >
          {messages.project.newButton}
        </button>
      </header>

      {showFirstRunGuide && (
        <div className="first-run-guide">
          <div className="first-run-guide-header">
            <h2 className="first-run-guide-title">{messages.project.firstRun.title}</h2>
          </div>
          <ol className="first-run-guide-steps">
            {messages.project.firstRun.steps.map((step) => (
              <li key={step}>{step}</li>
            ))}
          </ol>
          <div className="first-run-guide-actions">
            <button type="button" className="secondary" onClick={dismissFirstRun}>
              {messages.project.firstRun.dismiss}
            </button>
          </div>
        </div>
      )}

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
            onFocus={(e) => e.target.select()}
            placeholder={messages.project.namePlaceholder}
            autoFocus
          />
          <button type="submit" className="primary" disabled={busy || name.trim() === ""}>
            {messages.common.create}
          </button>
          <button type="button" className="secondary" onClick={closeCreateForm}>
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
        <EmptyState
          title={messages.project.empty.title}
          actionLabel={messages.project.empty.action}
          onAction={openCreateForm}
        />
      ) : (
        projects.length > 0 && (
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
                      onFocus={(e) => e.target.select()}
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
        )
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
