import { useCallback, useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { isTauri } from "../lib/ipc";
import { DownloadIcon } from "./Icons";

type Phase =
  | "idle"
  | "checking"
  | "current"
  | "available"
  | "downloading"
  | "installing"
  | "error";

/** Turns the updater's transport errors into something a person can act on. */
function explain(error: unknown): string {
  const text = String(error);
  if (text.includes("404") || text.includes("Could not fetch a valid release JSON")) {
    return "No published release yet. Tag a version so the release workflow can publish it.";
  }
  if (text.toLowerCase().includes("signature")) {
    return "The release was signed with a different key than this build expects.";
  }
  if (text.includes("Could not connect") || text.toLowerCase().includes("network")) {
    return "Could not reach the update server. Check the connection and try again.";
  }
  return text;
}

/**
 * In-app updates. `check()` reads the signed `latest.json` published with each
 * GitHub release; installing on Windows hands the NSIS installer the download,
 * which replaces Murmur and reopens it. Loadout, tasks and the key live in
 * AppData and are untouched.
 */
export function UpdateCard() {
  const [version, setVersion] = useState("");
  const [phase, setPhase] = useState<Phase>("idle");
  const [next, setNext] = useState<Update | null>(null);
  const [progress, setProgress] = useState(0);
  const [message, setMessage] = useState<string | null>(null);

  const look = useCallback(async () => {
    if (!isTauri()) {
      setPhase("error");
      setMessage("In-app updates only exist in the installed desktop app.");
      return;
    }
    setPhase("checking");
    setMessage(null);
    try {
      const found = await check();
      setNext(found);
      setPhase(found ? "available" : "current");
    } catch (error) {
      setNext(null);
      setPhase("error");
      setMessage(explain(error));
    }
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    void getVersion().then(setVersion).catch(() => undefined);
    void look();
  }, [look]);

  const install = useCallback(async () => {
    if (!next) return;
    setPhase("downloading");
    setProgress(0);
    setMessage(null);
    let total = 0;
    let done = 0;
    try {
      await next.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          done += event.data.chunkLength;
          setProgress(total > 0 ? Math.min(100, Math.round((done / total) * 100)) : 0);
        } else if (event.event === "Finished") {
          setPhase("installing");
        }
      });
      // The Windows installer reopens Murmur itself; this covers other targets.
      await relaunch();
    } catch (error) {
      setPhase("error");
      setMessage(explain(error));
    }
  }, [next]);

  const busy = phase === "checking" || phase === "downloading" || phase === "installing";
  const title =
    phase === "checking"
      ? "Checking for updates"
      : phase === "current"
        ? "You're up to date"
        : phase === "available"
          ? `Version ${next?.version ?? ""} is available`
          : phase === "downloading"
            ? `Downloading ${next?.version ?? ""}`
            : phase === "installing"
              ? "Installing"
              : phase === "error"
                ? "Update check failed"
                : "Update from inside Murmur";
  const detail =
    phase === "checking"
      ? "Looking at the newest published release."
      : phase === "current"
        ? version
          ? `Murmur ${version} is the newest published build.`
          : "No newer build is published."
        : phase === "available"
          ? "Your Loadout, tasks and key are kept; Murmur reopens after installing."
          : phase === "downloading"
            ? `${progress}% downloaded.`
            : phase === "installing"
              ? "Murmur closes and reopens when the installer finishes."
              : phase === "error"
                ? (message ?? "The update could not be checked.")
                : "Downloads a signed build from the latest release and installs it in place.";
  const action =
    phase === "available"
      ? "Download and install"
      : phase === "checking"
        ? "Checking…"
        : phase === "installing"
          ? "Installing…"
          : phase === "downloading"
            ? `Downloading ${progress}%`
            : "Check for updates";

  return (
    <section id="updates" className="dashboard-surface dashboard-anchor">
      <div className="dashboard-section-heading">
        <div>
          <h2>Updates</h2>
          <p>Murmur installs new versions in place, without a reinstall.</p>
        </div>
        <span className="dashboard-count" aria-label="Installed version">
          {version ? `v${version}` : "…"}
        </span>
      </div>

      <div className="dashboard-update-row">
        <span className="dashboard-update-mark" aria-hidden="true">
          <DownloadIcon size={16} />
        </span>
        <div className="dashboard-update-copy">
          <strong>{title}</strong>
          <span>{detail}</span>
        </div>
        <button
          type="button"
          className={phase === "available" ? "dashboard-primary-button" : "dashboard-subtle-button"}
          onClick={() => void (phase === "available" ? install() : look())}
          disabled={busy}
        >
          {action}
        </button>
      </div>

      {phase === "downloading" ? (
        <div
          className="dashboard-update-progress"
          role="progressbar"
          aria-label="Update download"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={progress}
        >
          <span style={{ width: `${progress}%` }} />
        </div>
      ) : null}

      {phase === "available" && next?.body ? (
        <pre className="dashboard-update-notes">{next.body}</pre>
      ) : null}
    </section>
  );
}
