import { MicIcon } from "../Icons";

/** The round mic badge that sits beside the dictation bubble. */
export function MicBubble() {
  return (
    <span className="notch-mic-bubble" aria-hidden="true">
      <MicIcon size={17} />
    </span>
  );
}
