import { useCallback, useEffect, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import { api } from "../lib/ipc";

/** Below this the gesture was a click, not a loop. */
const MIN_LOOP_CSS = 8;

interface Point {
  x: number;
  y: number;
}

function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value));
}

/**
 * Fullscreen transparent overlay on the monitor under the cursor. The user draws
 * a freehand loop; only the bounding box matters, and it is sent normalised
 * (0..1) so Rust can map it to physical pixels without CSS/DPI drift.
 */
export default function CaptureOverlay() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const points = useRef<Point[]>([]);
  const drawing = useRef(false);
  const sent = useRef(false);
  const [hintVisible, setHintVisible] = useState(true);

  const context = useCallback(() => canvasRef.current?.getContext("2d") ?? null, []);

  const resize = useCallback(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;

    // Back the canvas with device pixels so the stroke is crisp on 150%/200%
    // displays, while all pointer maths stays in CSS pixels.
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(window.innerWidth * dpr);
    canvas.height = Math.round(window.innerHeight * dpr);

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.lineWidth = 2.5;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.strokeStyle = "#a7b2ff";
    ctx.shadowColor = "rgba(124, 140, 255, 0.85)";
    ctx.shadowBlur = 12;
  }, []);

  useEffect(() => {
    resize();
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, [resize]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      sent.current = true;
      void api.cancelCircle().catch(() => undefined);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const clear = useCallback(() => {
    const ctx = context();
    ctx?.clearRect(0, 0, window.innerWidth, window.innerHeight);
    points.current = [];
  }, [context]);

  const submit = useCallback(async () => {
    if (sent.current) return;

    const list = points.current;
    if (list.length < 2) {
      clear();
      return;
    }

    let minX = Number.POSITIVE_INFINITY;
    let minY = Number.POSITIVE_INFINITY;
    let maxX = Number.NEGATIVE_INFINITY;
    let maxY = Number.NEGATIVE_INFINITY;

    for (const point of list) {
      minX = Math.min(minX, point.x);
      minY = Math.min(minY, point.y);
      maxX = Math.max(maxX, point.x);
      maxY = Math.max(maxY, point.y);
    }

    const width = maxX - minX;
    const height = maxY - minY;

    // A stray click should not send a 4 px crop to Gemini.
    if (width < MIN_LOOP_CSS || height < MIN_LOOP_CSS) {
      clear();
      return;
    }

    sent.current = true;
    const viewportWidth = window.innerWidth;
    const viewportHeight = window.innerHeight;

    await api
      .submitCircle({
        x: clamp01(minX / viewportWidth),
        y: clamp01(minY / viewportHeight),
        width: clamp01(width / viewportWidth),
        height: clamp01(height / viewportHeight),
      })
      .catch(() => undefined);
  }, [clear]);

  const start = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    if (sent.current) return;

    points.current = [{ x: event.clientX, y: event.clientY }];
    drawing.current = true;
    setHintVisible(false);
    event.currentTarget.setPointerCapture(event.pointerId);

    const ctx = context();
    if (!ctx) return;
    ctx.beginPath();
    ctx.moveTo(event.clientX, event.clientY);
  };

  const move = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    if (!drawing.current) return;

    const ctx = context();
    if (!ctx) return;

    points.current.push({ x: event.clientX, y: event.clientY });
    ctx.lineTo(event.clientX, event.clientY);
    ctx.stroke();
  };

  const end = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    if (!drawing.current) return;
    drawing.current = false;

    try {
      event.currentTarget.releasePointerCapture(event.pointerId);
    } catch {
      // Already released by the browser; nothing to undo.
    }

    const ctx = context();
    ctx?.closePath();
    ctx?.stroke();

    void submit();
  };

  return (
    <div className="relative h-full w-full cursor-crosshair select-none overflow-hidden">
      <div
        className="pointer-events-none absolute inset-0"
        style={{ background: "rgba(4, 6, 12, 0.10)" }}
      />

      <canvas
        ref={canvasRef}
        className="absolute inset-0 h-full w-full"
        onPointerDown={start}
        onPointerMove={move}
        onPointerUp={end}
        onPointerCancel={end}
      />

      {hintVisible ? (
        <div className="pointer-events-none absolute inset-x-0 bottom-10 flex justify-center">
          <span className="rounded-full border border-white/15 bg-black/60 px-3.5 py-1.5 text-[11px] font-medium text-white/80 backdrop-blur">
            Draw a loop around what you mean · Esc to cancel
          </span>
        </div>
      ) : null}
    </div>
  );
}
