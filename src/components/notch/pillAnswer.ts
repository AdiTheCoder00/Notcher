/**
 * A short, single-line answer packs into one pill instead of the full card.
 * Must stay in sync with `is_pill_answer` in src-tauri/src/notch.rs, which
 * picks the window size for that state.
 */
export const PILL_ANSWER_MAX_CHARS = 90;

export function isPillAnswer(text: string | null | undefined): boolean {
  const trimmed = text?.trim() ?? "";
  return trimmed.length > 0 && trimmed.length <= PILL_ANSWER_MAX_CHARS && !trimmed.includes("\n");
}
