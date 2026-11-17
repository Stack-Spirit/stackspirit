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

// The redesigned shell is opt-in while it is being designed: VITE_SPIRIT_UI=next or
// ?ui=next. The classic shell stays the default so its tests and flows are unchanged.
const uiParam = new URLSearchParams(window.location.search).get("ui");
const useNext = uiParam ? uiParam === "next" : import.meta.env.VITE_SPIRIT_UI === "next";
const Shell = useNext ? NextShell : AppShell;

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
