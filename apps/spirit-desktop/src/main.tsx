import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import {
  AppShell,
  ErrorState,
  NextShell,
  toSpiritError,
} from "@stackspirit/ui";
import "@stackspirit/ui/styles.css";

import { createBridge } from "./bridge";

const container = document.getElementById("root");
if (!container) throw new Error("missing #root");

const root = createRoot(container);

// The Meadow shell is the default. The classic cockpit stays available while the
// redesign settles: VITE_SPIRIT_UI=classic or ?ui=classic (its e2e suite uses this).
const uiParam = new URLSearchParams(window.location.search).get("ui");
const useClassic = uiParam ? uiParam === "classic" : import.meta.env.VITE_SPIRIT_UI === "classic";
const Shell = useClassic ? AppShell : NextShell;

void createBridge()
  .then((bridge) => {
    root.render(
      <StrictMode>
        <Shell bridge={bridge} />
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
