import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { AppShell } from "./app/AppShell";
import { createBridge } from "./bridge";
import { toSpiritError } from "./bridge/errors";
import { ErrorState } from "./components";
import "./styles.css";

const container = document.getElementById("root");
if (!container) throw new Error("missing #root");

const root = createRoot(container);

void createBridge()
  .then((bridge) => {
    root.render(
      <StrictMode>
        <AppShell bridge={bridge} />
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
