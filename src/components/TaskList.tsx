import { useState, type FormEvent } from "react";
import type {
  ActionKind,
  ScheduleKind,
  ScheduledTask,
  TaskStatus,
  TaskUpdate,
} from "../lib/ipc";

const STATUS_STYLES: Record<TaskStatus, string> = {
  pending: "bg-accent/10 text-accent-soft ring-accent/25",
  paused: "bg-amber-500/10 text-amber-300 ring-amber-500/25",
  running: "bg-sky-500/10 text-sky-300 ring-sky-500/25",
  done: "bg-emerald-500/10 text-emerald-300 ring-emerald-500/25",
  cancelled: "bg-zinc-500/10 text-zinc-400 ring-zinc-500/25",
  missed: "bg-orange-500/10 text-orange-300 ring-orange-500/25",
  failed: "bg-red-500/10 text-red-300 ring-red-500/25",
};

const ACTION_LABELS: Record<ActionKind, string> = {
  open_app: "Open app",
  open_url: "Open URL",
  run_search: "Web search",
  remind: "Reminder",
  type_text: "Type text",
};

const ACTION_HINTS: Record<ActionKind, string> = {
  open_app: "Application name, for example Spotify",
  open_url: "https://example.com/page",
  run_search: "Search query",
  remind: "Text shown in the Windows notification",
  type_text: "Text typed into the focused window",
};

const DEFAULT_RRULE = "FREQ=DAILY;BYHOUR=9;BYMINUTE=0";

const dueFormatter = new Intl.DateTimeFormat(undefined, {
  weekday: "short",
  month: "short",
  day: "numeric",
  hour: "2-digit",
  minute: "2-digit",
});

function formatDue(ms: number): string {
  return dueFormatter.format(new Date(ms));
}

function pad(value: number): string {
  return String(value).padStart(2, "0");
}

/** Milliseconds → the value a datetime-local input expects. */
function toLocalInput(ms: number): string {
  const date = new Date(ms);
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(
    date.getHours()
  )}:${pad(date.getMinutes())}`;
}

function fromLocalInput(value: string): number | null {
  const millis = new Date(value).getTime();
  return Number.isFinite(millis) ? millis : null;
}

interface Draft {
  prompt: string;
  action: ActionKind;
  payload: string;
  schedule: ScheduleKind;
  due: string;
  recurrence: string;
}

function draftFrom(task: ScheduledTask): Draft {
  return {
    prompt: task.prompt,
    action: task.action,
    payload: task.payload,
    schedule: task.schedule,
    due: toLocalInput(task.due_at_ms),
    recurrence: task.recurrence ?? DEFAULT_RRULE,
  };
}

interface Props {
  tasks: ScheduledTask[];
  onPause: (id: number) => Promise<void> | void;
  onResume: (id: number) => Promise<void> | void;
  onDelete: (id: number) => Promise<void> | void;
  onSave: (task: TaskUpdate) => Promise<void> | void;
}

export function TaskList({ tasks, onPause, onResume, onDelete, onSave }: Props) {
  const [editingId, setEditingId] = useState<number | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  if (tasks.length === 0) {
    return (
      <div className="rounded-xl border border-dashed border-line/80 px-4 py-8 text-center">
        <p className="text-base text-zinc-200">Nothing scheduled yet</p>
        <p className="mt-2 text-sm leading-relaxed text-muted">
          Hold Ctrl + Alt and say what you need, for example “remind me to send the invoice in
          twenty minutes”.
        </p>
      </div>
    );
  }

  function startEdit(task: ScheduledTask) {
    setEditingId(task.id);
    setDraft(draftFrom(task));
    setError(null);
  }

  function cancelEdit() {
    setEditingId(null);
    setDraft(null);
    setError(null);
  }

  async function submitEdit(event: FormEvent, task: ScheduledTask) {
    event.preventDefault();
    if (!draft) return;

    if (!draft.prompt.trim()) {
      setError("A task needs a prompt.");
      return;
    }

    const dueAt = fromLocalInput(draft.due);
    if (dueAt === null) {
      setError("Choose a valid date and time.");
      return;
    }
    if (draft.schedule === "recurring" && !draft.recurrence.trim()) {
      setError("A recurring task needs a recurrence rule.");
      return;
    }

    setSaving(true);
    setError(null);
    try {
      await onSave({
        id: task.id,
        prompt: draft.prompt.trim(),
        due_at_ms: dueAt,
        action: draft.action,
        payload: draft.payload.trim(),
        recurrence: draft.schedule === "recurring" ? draft.recurrence.trim() : null,
      });
      cancelEdit();
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="space-y-2">
      {error ? (
        <p className="rounded-lg border border-red-500/25 bg-red-500/5 px-3 py-2 text-sm text-red-300">
          {error}
        </p>
      ) : null}

      <ul className="divide-y divide-line/70 overflow-hidden rounded-xl border border-line">
        {tasks.map((task) => (
          <li key={task.id} className="bg-panel-raised/40">
            {editingId === task.id && draft ? (
              <form className="space-y-3 px-4 py-4" onSubmit={(event) => void submitEdit(event, task)}>
                <div>
                  <label className="text-[11px] font-medium uppercase tracking-wide text-muted">
                    Task
                  </label>
                  <input
                    value={draft.prompt}
                    onChange={(event) => setDraft({ ...draft, prompt: event.target.value })}
                    className="mt-1 w-full rounded-lg border border-line bg-panel px-3 py-2 text-sm text-zinc-200 outline-none focus:border-accent/50"
                  />
                </div>

                <div className="grid gap-3 sm:grid-cols-2">
                  <div>
                    <label className="text-[11px] font-medium uppercase tracking-wide text-muted">
                      Schedule
                    </label>
                    <select
                      value={draft.schedule}
                      onChange={(event) =>
                        setDraft({
                          ...draft,
                          schedule: event.target.value as ScheduleKind,
                        })
                      }
                      className="mt-1 w-full rounded-lg border border-line bg-panel px-3 py-2 text-sm text-zinc-200 outline-none focus:border-accent/50"
                    >
                      <option value="one_time">One time</option>
                      <option value="recurring">Recurring</option>
                    </select>
                  </div>
                  <div>
                    <label className="text-[11px] font-medium uppercase tracking-wide text-muted">
                      {draft.schedule === "recurring" ? "Next run" : "Due"}
                    </label>
                    <input
                      type="datetime-local"
                      value={draft.due}
                      onChange={(event) => setDraft({ ...draft, due: event.target.value })}
                      className="mt-1 w-full rounded-lg border border-line bg-panel px-3 py-2 text-sm text-zinc-200 outline-none focus:border-accent/50"
                    />
                  </div>
                </div>

                {draft.schedule === "recurring" ? (
                  <div>
                    <label className="text-[11px] font-medium uppercase tracking-wide text-muted">
                      Recurrence (RFC 5545 RRULE)
                    </label>
                    <input
                      value={draft.recurrence}
                      onChange={(event) =>
                        setDraft({ ...draft, recurrence: event.target.value })
                      }
                      placeholder={DEFAULT_RRULE}
                      className="mt-1 w-full rounded-lg border border-line bg-panel px-3 py-2 font-mono text-xs text-zinc-200 outline-none focus:border-accent/50"
                    />
                  </div>
                ) : null}

                <div className="grid gap-3 sm:grid-cols-2">
                  <div>
                    <label className="text-[11px] font-medium uppercase tracking-wide text-muted">
                      Action
                    </label>
                    <select
                      value={draft.action}
                      onChange={(event) =>
                        setDraft({ ...draft, action: event.target.value as ActionKind })
                      }
                      className="mt-1 w-full rounded-lg border border-line bg-panel px-3 py-2 text-sm text-zinc-200 outline-none focus:border-accent/50"
                    >
                      {(Object.keys(ACTION_LABELS) as ActionKind[]).map((action) => (
                        <option key={action} value={action}>
                          {ACTION_LABELS[action]}
                        </option>
                      ))}
                    </select>
                  </div>
                  <div>
                    <label className="text-[11px] font-medium uppercase tracking-wide text-muted">
                      Payload
                    </label>
                    <input
                      value={draft.payload}
                      onChange={(event) => setDraft({ ...draft, payload: event.target.value })}
                      placeholder={ACTION_HINTS[draft.action]}
                      className="mt-1 w-full rounded-lg border border-line bg-panel px-3 py-2 text-sm text-zinc-200 outline-none focus:border-accent/50"
                    />
                  </div>
                </div>

                <div className="flex items-center justify-end gap-2">
                  <button
                    type="button"
                    onClick={cancelEdit}
                    className="rounded-md px-2.5 py-1.5 text-xs text-muted transition hover:bg-white/5 hover:text-zinc-200"
                  >
                    Cancel
                  </button>
                  <button
                    type="submit"
                    disabled={saving}
                    className="rounded-md border border-accent/35 bg-accent/10 px-2.5 py-1.5 text-xs font-medium text-accent-soft transition hover:bg-accent/15 disabled:opacity-50"
                  >
                    {saving ? "Saving…" : "Save changes"}
                  </button>
                </div>
              </form>
            ) : (
              <div className="flex flex-wrap items-center gap-3 px-4 py-3">
                <span
                  className={`rounded-full px-2.5 py-1 text-[11px] font-medium capitalize tracking-wide ring-1 ring-inset ${
                    STATUS_STYLES[task.status]
                  }`}
                >
                  {task.status}
                </span>

                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm font-medium text-zinc-100">{task.prompt}</p>
                  <p className="mt-1 truncate text-xs text-muted">
                    {ACTION_LABELS[task.action]}
                    {task.payload ? ` · ${task.payload}` : ""}
                  </p>
                  {task.last_error ? (
                    <p className="mt-1 truncate text-xs text-red-300" title={task.last_error}>
                      {task.last_error}
                    </p>
                  ) : null}
                </div>

                <div className="shrink-0 text-right">
                  <p className="text-sm tabular-nums text-zinc-200">{formatDue(task.due_at_ms)}</p>
                  {task.schedule === "recurring" ? (
                    <p
                      className="max-w-[180px] truncate text-xs text-muted"
                      title={task.recurrence ?? undefined}
                    >
                      {task.recurrence}
                    </p>
                  ) : null}
                </div>

                {task.status !== "running" ? (
                  <div className="flex shrink-0 items-center gap-1">
                    {task.status === "pending" ? (
                      <button
                        type="button"
                        onClick={() => void onPause(task.id)}
                        className="rounded-md px-2 py-1.5 text-xs text-muted transition hover:bg-white/5 hover:text-zinc-200"
                      >
                        Pause
                      </button>
                    ) : null}
                    {task.status === "paused" ? (
                      <button
                        type="button"
                        onClick={() => void onResume(task.id)}
                        className="rounded-md px-2 py-1.5 text-xs text-accent-soft transition hover:bg-accent/10"
                      >
                        Resume
                      </button>
                    ) : null}
                    <button
                      type="button"
                      onClick={() => startEdit(task)}
                      className="rounded-md px-2 py-1.5 text-xs text-muted transition hover:bg-white/5 hover:text-zinc-200"
                    >
                      Edit
                    </button>
                    <button
                      type="button"
                      onClick={() => void onDelete(task.id)}
                      className="rounded-md px-2 py-1.5 text-xs text-muted transition hover:bg-red-500/10 hover:text-red-300"
                    >
                      Delete
                    </button>
                  </div>
                ) : null}
              </div>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
