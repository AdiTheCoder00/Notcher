export function Keycap({ children }: { children: string }) {
  return (
    <kbd className="inline-flex h-6 min-w-6 items-center justify-center rounded-md border border-line bg-panel-raised px-1.5 font-sans text-[11px] font-medium tracking-wide text-zinc-300 shadow-[inset_0_-1px_0_rgba(255,255,255,0.06)]">
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
