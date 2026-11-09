import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { api, isTauri } from "../../lib/ipc";

type Stats = { cpu: number; memory_used: number; memory_total: number; uptime: number; disk_used: number; disk_total: number; received: number; transmitted: number; processes: { name: string; memory: number }[]; gpu: { name: string; usage: number; used_mb: number; total_mb: number } | null };
type Clip = { text: string; time: number; pinned: boolean };
type ShelfItem = { path: string; pinned: boolean };
function load<T>(key: string, fallback: T): T { try { return JSON.parse(localStorage.getItem(key) || "null") ?? fallback; } catch { return fallback; } }
function bytes(n: number) { return n >= 1073741824 ? `${(n / 1073741824).toFixed(1)} GB` : n >= 1048576 ? `${(n / 1048576).toFixed(1)} MB` : n >= 1024 ? `${(n / 1024).toFixed(0)} KB` : `${n} B`; }

export function WeatherPanel() {
  const [city, setCity] = useState(() => localStorage.getItem("murmur-weather-city") || "");
  const [draft, setDraft] = useState(city);
  const [weather, setWeather] = useState<{temperature: number; code: number; name: string} | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    if (!city) return;
    const abort = new AbortController();
    const refresh = async () => {
      try {
        const locationResponse = await fetch(`https://geocoding-api.open-meteo.com/v1/search?count=1&name=${encodeURIComponent(city)}`, {signal: abort.signal});
        if (!locationResponse.ok) throw new Error("Could not find city");
        const location = (await locationResponse.json()).results?.[0];
        if (!location) throw new Error("City not found");
        const response = await fetch(`https://api.open-meteo.com/v1/forecast?latitude=${location.latitude}&longitude=${location.longitude}&current=temperature_2m,weather_code`, {signal: abort.signal});
        if (!response.ok) throw new Error("Weather unavailable");
        const data = await response.json();
        setWeather({temperature: data.current.temperature_2m, code: data.current.weather_code, name: location.name}); setError("");
      } catch (e) { if (!abort.signal.aborted) setError(String(e)); }
    };
    void refresh(); const timer = window.setInterval(() => void refresh(), 15 * 60 * 1000);
    return () => { abort.abort(); clearInterval(timer); };
  }, [city]);
  const description = !weather ? "" : weather.code === 0 ? "Clear" : weather.code <= 3 ? "Cloudy" : weather.code <= 48 ? "Foggy" : weather.code <= 67 ? "Rain" : weather.code <= 77 ? "Snow" : weather.code <= 82 ? "Showers" : "Stormy";
  return <div className="cc-weather">{weather && <><span className="cc-weather-symbol">☀</span><strong>{Math.round(weather.temperature)}°C</strong><small>{description} · {weather.name}</small></>}<form onSubmit={e => {e.preventDefault(); const value = draft.trim(); setCity(value); if (value) localStorage.setItem("murmur-weather-city", value); else localStorage.removeItem("murmur-weather-city");}}><input value={draft} onChange={e => setDraft(e.target.value)} placeholder="Enter city" aria-label="Weather city" /><button type="submit" aria-label="Update weather">→</button></form>{error && <small role="alert">{error}</small>}</div>;
}

export function StatsPanel() {
  const [stats, setStats] = useState<Stats | null>(null);
  const [error, setError] = useState("");
  const [speed, setSpeed] = useState<{download_mbps: number; upload_mbps: number} | null>(null);
  const [speedBusy, setSpeedBusy] = useState(false);
  useEffect(() => {
    if (!isTauri()) return;
    let live = true;
    const refresh = () => invoke<Stats>("dashboard_stats").then(s => { if (live) setStats(s); }).catch(e => { if (live) setError(String(e)); });
    void refresh(); const timer = window.setInterval(refresh, 5000);
    return () => { live = false; clearInterval(timer); };
  }, []);
  const ram = stats?.memory_total ? Math.round(stats.memory_used / stats.memory_total * 100) : 0;
  return <div className="cc-stats">
    <div className="cc-stat-main">
      {[{ title: "CPU", value: stats ? `${Math.round(stats.cpu)}%` : "—", percent: stats?.cpu ?? 0, detail: "Processor", color: "#d2ae59" }, { title: "GPU", value: stats?.gpu ? `${Math.round(stats.gpu.usage)}%` : "—", percent: stats?.gpu?.usage ?? 0, detail: stats?.gpu?.name ?? "Unavailable", color: "#cba4e9" }, { title: "RAM", value: stats ? `${ram}%` : "—", percent: ram, detail: stats ? `${bytes(stats.memory_used)} / ${bytes(stats.memory_total)}` : "Memory", color: "#a6d284" }].map(g => <section className="cc-stat-card" key={g.title}><div className="cc-gauge" style={{ background: `conic-gradient(${g.color} ${g.percent}%, #30323d 0)` }}><span>{g.value}</span></div><strong>{g.title}</strong><small>{g.detail}</small></section>)}
      <div className="cc-stat-mini"><small>DISK</small><strong>{stats ? `${bytes(stats.disk_used)} / ${bytes(stats.disk_total)}` : "—"}</strong></div>
      <div className="cc-stat-mini"><small>NETWORK</small><strong>{stats ? `↓ ${bytes(stats.received)}/s  ↑ ${bytes(stats.transmitted)}/s` : "—"}</strong></div>
      <div className="cc-stat-mini"><small>UP FOR</small><strong>{stats ? `${Math.floor(stats.uptime / 3600)} h ${Math.floor(stats.uptime % 3600 / 60)} m` : "—"}</strong></div>
    </div>
    <div className="cc-stat-side"><section><small>Top apps · RAM</small>{stats?.processes.map((p, i) => <div className="cc-process" key={`${p.name}-${i}`}><span>{p.name}</span><small>{bytes(p.memory)}</small></div>)}{!stats && <p>{error || "System statistics are available in the desktop app."}</p>}</section><section><small>Internet speed · Cloudflare</small><p>{speed ? `↓ ${speed.download_mbps.toFixed(1)} Mbps　 ↑ ${speed.upload_mbps.toFixed(1)} Mbps` : "Run a test to measure your connection."}</p><button className="cc-speed-button" disabled={speedBusy || !isTauri()} onClick={() => { setSpeedBusy(true); setError(""); void invoke<{download_mbps:number; upload_mbps:number}>("dashboard_speed_test").then(setSpeed).catch(e => setError(String(e))).finally(() => setSpeedBusy(false)); }}>{speedBusy ? "Testing…" : "▶  Run speed test"}</button>{error && stats ? <small role="alert">{error}</small> : null}</section></div>
  </div>;
}

export function ClipboardPanel() {
  type Entry = Clip & { kind?: "text" | "image" };
  const [enabled, setEnabled] = useState(false);
  const [items, setItems] = useState<Entry[]>(() => load("murmur-clipboard-history", []));
  const [selected, setSelected] = useState(0);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("All");
  const [error, setError] = useState("");
  useEffect(() => { try { localStorage.setItem("murmur-clipboard-history", JSON.stringify(items)); } catch { setError("History is full. Remove items to save new copies."); } }, [items]);
  useEffect(() => {
    if (!enabled || !isTauri()) return;
    let live = true, busy = false;
    const poll = async () => {
      if (busy) return;
      busy = true;
      try {
        const item = await invoke<{kind: "text" | "image"; data: string}>("dashboard_clipboard");
        if (!live || !item.data || (item.kind === "text" && item.data.length > 100000)) return;
        setItems(old => {
          if (old.some(x => x.text === item.data && (x.kind ?? "text") === item.kind)) return old;
          const next: Entry[] = [{ text: item.data, kind: item.kind, time: Date.now(), pinned: false }, ...old];
          // Keep local WebView storage below its normal quota.
          while (next.length > 100 || JSON.stringify(next).length > 4_000_000) {
            let index = next.length - 1;
            while (index > 0 && next[index]?.pinned) index--;
            if (index === 0) return old;
            next.splice(index, 1);
          }
          return next;
        });
      } catch (err) { const message = String(err); if (message.includes("too large") || message.includes("size limit")) setError("This image is too large to save in clipboard history."); }
      finally { busy = false; }
    };
    void poll(); const timer = window.setInterval(() => void poll(), 2000);
    return () => { live = false; clearInterval(timer); };
  }, [enabled]);
  const filtered = useMemo(() => items.filter(x => {
    const image = x.kind === "image";
    const link = !image && /^https?:\/\//i.test(x.text);
    return (image ? "image".includes(query.toLowerCase()) : x.text.toLowerCase().includes(query.toLowerCase())) &&
      (filter === "All" || (filter === "Images" ? image : filter === "Links" ? link : !image && !link));
  }), [items, query, filter]);
  const current = filtered[selected] ?? filtered[0];
  const update = (fn: (x: Entry) => Entry | null) => { if (current) setItems(old => old.flatMap(x => x === current ? [fn(x)].filter((v): v is Entry => v !== null) : [x])); };
  const copy = () => { if (!current) return; const action = current.kind === "image" ? invoke("dashboard_copy_image", { data: current.text }) : api.copyText(current.text); void action.catch(e => setError(String(e))); };
  return <div className="cc-extra">
    <div className="cc-extra-search"><input aria-label="Search clipboard history" value={query} onChange={e => {setQuery(e.target.value); setSelected(0);}} placeholder="Search clipboard history..." />{["All", "Text", "Images", "Links"].map(x => <button className={filter === x ? "selected" : ""} onClick={() => {setFilter(x); setSelected(0);}} key={x}>{x}</button>)}</div>
    <div className="cc-clipboard-layout"><div className="cc-clip-list"><div className="cc-extra-caption">HISTORY</div>{filtered.map((item, i) => <button className={current === item ? "selected" : ""} key={`${item.time}-${i}`} onClick={() => setSelected(i)}><span>{item.pinned ? "★ " : ""}{item.kind === "image" ? "Image" : item.text}</span><small>{new Date(item.time).toLocaleTimeString([], {hour: "numeric", minute: "2-digit"})}</small></button>)}{!filtered.length && <p>No matching items.</p>}</div>
      <div className="cc-clip-detail">{current ? <><div className="cc-clip-text">{current.kind === "image" ? <img className="cc-clip-image" src={`data:image/png;base64,${current.text}`} alt="Copied image" /> : current.text}</div><small>Type　　{current.kind === "image" ? "Image" : /^https?:\/\//i.test(current.text) ? "Link" : "Text"}<br />Copied　{new Date(current.time).toLocaleString()}</small></> : <p>Copied text and images appear here when history is enabled.</p>}</div>
    </div><div className="cc-extra-footer"><button onClick={() => setEnabled(v => !v)} disabled={!isTauri()}>{enabled ? "Pause history" : "Start history"}</button><span>{enabled ? "Recording on this device until paused or Murmur closes." : "History is paused. Copies stay on this device."}</span>{current && <><button onClick={copy}>Copy</button><button onClick={() => update(x => ({...x, pinned: !x.pinned}))}>{current.pinned ? "Unpin" : "Pin"}</button><button onClick={() => update(() => null)}>Remove</button></>}{error && <span role="alert">{error}</span>}</div>
  </div>;
}
export function ShelfPanel({ onPicker }: { onPicker: (busy: boolean) => void }) {
  const [items, setItems] = useState<ShelfItem[]>(() => load("murmur-shelf", []));
  const [query, setQuery] = useState(""); const [filter, setFilter] = useState("All"); const [error, setError] = useState("");
  const dragStart = useRef<{x: number; y: number; path: string} | null>(null);
  useEffect(() => { try { localStorage.setItem("murmur-shelf", JSON.stringify(items)); } catch { setError("Shelf could not be saved."); } }, [items]);
  const choose = async (folder: boolean) => { onPicker(true); try { const paths = await invoke<string[]>("dashboard_pick_files", { folder }); setItems(old => [...old, ...paths.filter(p => !old.some(x => x.path === p)).map(path => ({ path, pinned: false }))]); } catch (e) { setError(String(e)); } finally { onPicker(false); } };
  const filtered = items.filter(x => { const p = x.path.toLowerCase(); const type = /\.(png|jpg|jpeg|gif|webp)$/.test(p) ? "Images" : /\.(mp4|mov|mkv|webm)$/.test(p) ? "Video" : /\.(mp3|wav|m4a|flac)$/.test(p) ? "Audio" : /\.(pdf|docx?|txt|md)$/.test(p) ? "Docs" : "Other"; return p.includes(query.toLowerCase()) && (filter === "All" || filter === type); });
  return <div className="cc-extra"><div className="cc-extra-search"><input aria-label="Search the shelf" value={query} onChange={e => setQuery(e.target.value)} placeholder="Search the shelf" />{["All", "Video", "Images", "Audio", "Docs"].map(x => <button className={filter === x ? "selected" : ""} onClick={() => setFilter(x)} key={x}>{x}</button>)}</div><div className="cc-shelf-layout"><aside><div className="cc-extra-caption">ON THE SHELF</div><strong>▦　Shelf</strong><button onClick={() => void choose(false)} disabled={!isTauri()}>＋ Add files</button><button onClick={() => void choose(true)} disabled={!isTauri()}>＋ Add folder</button></aside><div className="cc-shelf-items">{filtered.map(item => <div className="cc-shelf-item" key={item.path}><button className="cc-shelf-open" onPointerDown={e => { if (e.button === 0) dragStart.current = {x: e.clientX, y: e.clientY, path: item.path}; }} onPointerUp={() => { dragStart.current = null; }} onPointerMove={e => { const start = dragStart.current; if (!start || !(e.buttons & 1) || Math.hypot(e.clientX - start.x, e.clientY - start.y) < 6) return; dragStart.current = null; onPicker(true); void invoke("dashboard_drag_path", { path: start.path }).catch(err => setError(String(err))).finally(() => onPicker(false)); }} onClick={() => void invoke("dashboard_open_path", { path: item.path }).catch(e => setError(String(e)))} title={item.path}><span>📁</span><strong>{item.path.split(/[\\/]/).pop()}</strong></button><div><button onClick={() => setItems(old => old.map(x => x === item ? {...x, pinned: !x.pinned} : x))}>{item.pinned ? "Unpin" : "Pin"}</button><button onClick={() => setItems(old => old.filter(x => x !== item))}>Remove</button></div></div>)}{!filtered.length && <p>Add files or folders to keep them close at hand.</p>}</div></div><div className="cc-extra-footer"><span>Drag items to another app. Removing an item does not delete it.</span>{error && <span role="alert">{error}</span>}</div></div>;
}
