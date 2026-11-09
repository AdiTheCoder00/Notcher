import { useState } from "react";
import { api, isTauri } from "../lib/ipc";

interface Props {
  hasKey: boolean;
  onChanged: () => void;
}

export function ApiKeyCard({ hasKey, onChanged }: Props) {
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function save() {
    if (!isTauri()) {
      setError("Open the app with `npm run tauri dev` to save a key.");
      return;
    }
    if (draft.trim().length === 0) {
      setError("Paste your Gemini API key first.");
      return;
    }
    setBusy(true);
    try {
      await api.saveApiKey(draft.trim());
      setDraft("");
      setError(null);
      onChanged();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!isTauri()) return;
    setBusy(true);
    try {
      await api.clearApiKey();
      setError(null);
      onChanged();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="dashboard-surface rounded-2xl border border-line bg-panel/80 p-5">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h2 id="gemini-key-heading" className="text-base font-semibold text-zinc-100">
            Gemini API key
          </h2>
          <p className="mt-1 text-sm leading-relaxed text-muted">
            Stored in the Windows Credential Manager, never in a file.
          </p>
        </div>
        <span className={`dashboard-api-key-status ${hasKey ? "is-ready" : "is-pending"}`}>
          {hasKey ? "Key saved" : "Not set"}
        </span>
      </div>

      <div className="mt-5 flex flex-wrap items-end gap-2">
        <div className="flex min-w-[180px] flex-1 flex-col gap-1">
          <label htmlFor="gemini-api-key" className="text-xs font-medium text-muted">
            API key
          </label>
          <input
            id="gemini-api-key"
            type="password"
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void save();
            }}
            placeholder={hasKey ? "Replace with a new key" : "AIza…"}
            spellCheck={false}
            autoComplete="off"
            aria-invalid={error ? true : undefined}
            aria-describedby={error ? "gemini-api-key-error" : undefined}
            className="dashboard-api-key-input h-11 w-full rounded-lg border border-line px-3 font-mono text-sm text-zinc-200 outline-none transition placeholder:text-muted focus:border-accent/60 focus:ring-2 focus:ring-accent/20"
          />
        </div>
        <button
          type="button"
          onClick={() => void save()}
          disabled={busy}
          className="h-11 shrink-0 rounded-lg bg-accent px-4 text-sm font-semibold text-on-accent transition hover:underline underline-offset-4 disabled:opacity-50"
        >
          {hasKey ? "Replace" : "Save key"}
        </button>
        {hasKey ? (
          <button
            type="button"
            onClick={() => void remove()}
            disabled={busy}
            className="h-11 shrink-0 rounded-lg border border-line px-3 text-sm font-medium text-muted transition hover:border-red-500/40 hover:text-red-300 disabled:opacity-50"
          >
            Remove
          </button>
        ) : null}
      </div>

      {error ? (
        <p id="gemini-api-key-error" className="mt-3 text-sm text-red-300" role="alert">
          {error}
        </p>
      ) : null}
    </section>
  );
}
