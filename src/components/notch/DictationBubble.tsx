import { LiveWaveform } from "./LiveWaveform";
import { MicBubble } from "./MicBubble";

interface Props {
  keys: string[];
  text: string;
}

/**
 * The dictation bubble: the chord that started it, the status line, and the mic
 * badge. The notch window is sized for exactly this row (src-tauri/src/notch.rs).
 */
export function DictationBubble({ keys, text }: Props) {
  return (
    <div className="notch-dictation-row flex h-full items-start gap-3 p-3">
      <div className="notch-dictation-card">
        <div className="notch-dictation-keys">
          <span className="notch-dictation-chord">
            {keys.map((key, index) => (
              <span key={`${key}-${index}`} className="notch-dictation-key">
                {index > 0 ? (
                  <span className="notch-dictation-plus" aria-hidden="true">
                    +
                  </span>
                ) : null}
                <kbd className="notch-keycap">{key}</kbd>
              </span>
            ))}
          </span>
          <LiveWaveform active />
        </div>
        <p className="notch-dictation-text">{text}</p>
      </div>
      <MicBubble />
    </div>
  );
}
