import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { ErrorState, NextShell, toSpiritError } from "@stackspirit/ui";
import "@stackspirit/ui/styles.css";

import { createBridge } from "./bridge";
import { createDesktopCloudRuntime } from "./cloud/runtime";

const container = document.getElementById("root");
if (!container) throw new Error("missing #root");

const root = createRoot(container);

/**
 * The optional Account panel is wired for the real desktop host only. Preview
 * mode keeps its fake data, and a bad optional cloud base must never prevent
 * account-free local/SSH mode from booting.
 */
function installOptionalCloud(): void {
  if (import.meta.env.VITE_SPIRIT_MOCK === "1") return;
  try {
    createDesktopCloudRuntime({
      apiBaseUrl: import.meta.env.VITE_SPIRIT_CLOUD_API_URL || undefined,
    });
  } catch {
    // The Settings card keeps its honest local-mode copy.
  }
}

void createBridge()
  .then((bridge) => {
    installOptionalCloud();
    root.render(
      <StrictMode>
        <NextShell bridge={bridge} />
      </StrictMode>,
    );
  })
  .catch((error: unknown) => {
    root.render(
      <div className="boot-error">
        <ErrorState error={toSpiritError(error)} />
      </div>,
    );
  });
