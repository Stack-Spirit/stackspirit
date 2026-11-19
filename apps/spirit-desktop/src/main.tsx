import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { ErrorState, NextShell, toSpiritError } from "@stackspirit/ui";
import "@stackspirit/ui/styles.css";

import { createBridge } from "./bridge";

const container = document.getElementById("root");
if (!container) throw new Error("missing #root");

const root = createRoot(container);

void createBridge()
  .then((bridge) => {
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
