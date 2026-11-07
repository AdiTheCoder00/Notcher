interface Props {
  message: string;
}

/** Error state: short, red, no stack traces, never more than two lines. */
export function ErrorPill({ message }: Props) {
  return (
    <div className="flex w-[332px] max-w-[calc(100vw-24px)] items-center gap-2.5 px-4 py-3">
      <span
        className="h-1.5 w-1.5 shrink-0 rounded-full"
        style={{ background: "var(--notch-danger)" }}
        aria-hidden="true"
      />
      <p
        className="line-clamp-2 text-[12px] leading-snug"
        style={{ color: "var(--notch-danger)" }}
      >
        {message}
      </p>
    </div>
  );
}
