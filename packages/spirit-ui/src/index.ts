/**
 * Public source entry for `@stackspirit/ui`.
 *
 * Vite and Vitest consume this file directly; there is no separate package build. Host
 * applications create a `SpiritBridge` and pass it to `AppShell`. Nothing here imports a
 * host runtime or a Node-only module.
 */
export * from "./bridge";
export * from "./components";
export * from "./domain/capacity";
export * from "./domain/connection";
export * from "./domain/engine";
export * from "./domain/executor";
export * from "./domain/format";
export * from "./domain/raw";
export * from "./state/fleetStore";

export { AppShell } from "./app/AppShell";
export { NextShell } from "./next/NextShell";
export { StatusStrip } from "./app/StatusStrip";
export { TopBar, type AppView, type TopBarProps } from "./app/TopBar";

export { CapacityRail } from "./views/capacity/CapacityRail";
export { EngineView } from "./views/engine/EngineView";
export { FleetCanvas } from "./views/fleet/FleetCanvas";
export { HostManager } from "./views/hosts/HostManager";
export { InboxDrawer } from "./views/inbox/InboxDrawer";
