import { lazy, Suspense } from "react";

const MainWindow = lazy(() => import("./windows/MainWindow"));
const NotchWindow = lazy(() => import("./windows/NotchWindow"));
const CaptureOverlay = lazy(() => import("./windows/CaptureOverlay"));

/** Rust opens each window from the shared index using its `window` query. */
export default function App() {
  const view = new URLSearchParams(window.location.search).get("window");
  return (
    <Suspense fallback={null}>
      {view === "notch" ? (
        <NotchWindow />
      ) : view === "capture" ? (
        <CaptureOverlay />
      ) : (
        <MainWindow />
      )}
    </Suspense>
  );
}
