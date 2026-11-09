export function Keycap({ children }: { children: string }) {
  return (
    <kbd className="inline-flex h-6 min-w-6 items-center justify-center rounded-[9px] border border-line bg-panel-raised px-1.5 font-mono text-[10px] font-medium uppercase tracking-[0.08em] text-zinc-300">
      {children}
    </kbd>
  );
}

export function KeyCombo({ keys }: { keys: readonly string[] }) {
  return (
    <span className="flex items-center gap-1">
      {keys.map((key, index) => (
        <span key={key} className="flex items-center gap-1">
          {index > 0 ? <span className="text-[10px] text-muted">+</span> : null}
          <Keycap>{key}</Keycap>
        </span>
      ))}
    </span>
  );
}
