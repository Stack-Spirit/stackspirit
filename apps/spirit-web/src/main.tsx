import { createRoot } from "react-dom/client";

import type { SpiritErrorDto } from "@stackspirit/ui";
import { Button } from "@stackspirit/ui";
import "@stackspirit/ui/styles.css";

// Meadow tokens and surfaces. The package does not export `./next/theme.css` yet, so the
// Web app imports the same stylesheet the desktop shell uses by source path.
import "../../../packages/spirit-ui/src/next/theme.css";
// Baseline styling for the reused classic job lane/card/worker components.
import "../../../packages/spirit-ui/src/views/fleet/fleet.css";

import { createBridge } from "./bridge/create";
import { DesktopOnlyBoot } from "./app/DesktopOnlyPage";
import { SetupPanel } from "./app/SetupPanel";
import { pathFor, WebApp } from "./app/WebApp";
import { SpiritMark } from "./shared/next";
import "./web.css";

const container = document.getElementById("root");
if (!container) throw new Error("missing #root");

const root = createRoot(container);

function BootError({ error }: { error: SpiritErrorDto }) {
  return (
    <main className="web-boot" id="main-canvas">
      <div className="web-brand">
        <SpiritMark size={36} />
        <div>
          <strong>StackSpirit</strong>
          <span>Web cockpit · read-only</span>
        </div>
      </div>
      <section className="panel panel--danger" role="alert">
        <header className="panel__header">
          <h2 className="panel__title">The Web shell could not start</h2>
        </header>
        <div className="panel__body">
          <p>{error.message}</p>
          {error.hint ? <p className="muted">{error.hint}</p> : null}
          <Button variant="secondary" onClick={() => window.location.reload()}>
            Reload
          </Button>
        </div>
      </section>
    </main>
  );
}

const initialPath = pathFor(window.location.pathname);

if (initialPath !== "fleet") {
  // Direct desktop-only routes render without creating a bridge, so they make no request.
  root.render(<DesktopOnlyBoot surface={initialPath} />);
} else {
  const boot = createBridge();
  if (boot.bridge) {
    root.render(<WebApp bridge={boot.bridge} />);
  } else if (boot.setup) {
    root.render(<SetupPanel error={boot.error} />);
  } else if (boot.error) {
    root.render(<BootError error={boot.error} />);
  } else {
    root.render(<SetupPanel error={null} />);
  }
}
