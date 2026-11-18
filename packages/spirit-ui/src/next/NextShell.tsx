import { useEffect, useState, type CSSProperties } from "react";

import type { SpiritBridge } from "../bridge";
import { useConnection, useFleet, useFleetStore, useHosts, useSelectedHostId } from "../state/fleetStore";
import { RawDocumentDrawer } from "../views/capacity/RawDocumentDrawer";
import { AddHostWizard } from "../views/hosts/AddHostWizard";
import { InboxDrawer } from "../views/inbox/InboxDrawer";
import { SessionsDrawer } from "../views/sessions/SessionsDrawer";
import { AddProjectDialog } from "./AddProjectDialog";
import { ErrorNotice } from "./ErrorNotice";
import { ChatView } from "./ChatView";
import { useConversations } from "./conversations";
import { ContextPanel } from "./ContextPanel";
import { ServerPage } from "./ServerPage";
import { SettingsView } from "./SettingsView";
import { useShellUi, type NextView } from "./shellUi";
import { Sidebar } from "./Sidebar";
import { useNextTheme } from "./themePreference";
import { useResizable } from "./useResizable";
import { ConnectPrompt, Welcome } from "./Welcome";
import { WorkersPage } from "./WorkersPage";
import "./theme.css";

/**
 * The redesigned desktop shell (Meadow theme, chat-first, DeepSeek Harness layout).
 *
 * - No saved server: a full-page welcome with "Add server" and "Set up with AI".
 * - A saved server that is not connected: sidebar + one Connect action.
 * - Connected: sidebar (server health, global orchestrator, projects with their
 *   orchestrator and workers), the main view, and a details panel that is closed
 *   until it is toggled or something is opened in it.
 *
 * Main views: an orchestrator session, the server page, a project's workers page,
 * or settings. Hosts, connection, fleet and capacity come from the existing store
 * and bridge; orchestrator chat is a design preview (see conversations.ts).
 */
export function NextShell({ bridge }: { bridge: SpiritBridge }) {
  const initialize = useFleetStore((state) => state.initialize);
  const hostsLoaded = useFleetStore((state) => state.hostsLoaded);
  const connect = useFleetStore((state) => state.connect);
  const hosts = useHosts();
  const selectedHostId = useSelectedHostId();
  const connection = useConnection();
  const closePanel = useShellUi((state) => state.closePanel);
  const panelOpen = useShellUi((state) => state.panel !== null);
  const requestProject = useConversations((state) => state.requestProject);
  const fleet = useFleet();
  const [theme, setTheme] = useNextTheme();
  const side = useResizable({ storageKey: "stackspirit.sidebarWidth", initial: 272, min: 220, max: 420, grows: "right" });
  const drawer = useResizable({ storageKey: "stackspirit.panelWidth", initial: 400, min: 320, max: 640, grows: "left" });
  const [addingProject, setAddingProject] = useState(false);
  // Classic drawers, reused unchanged: inbox answers, .asif sessions, raw document.
  const [inboxOpen, setInboxOpen] = useState(false);
  const [rawOpen, setRawOpen] = useState(false);
  const sessionsOpen = useFleetStore((state) => state.sessionsOpen);
  const sessionsIntent = useFleetStore((state) => state.sessionsIntent);
  const closeSessions = useFleetStore((state) => state.closeSessions);

  const connected = connection.state === "connected" || connection.state === "stale";
  // "global" shows the Connect prompt until a saved server is connected; the setup
  // assistant is opened explicitly from the welcome page or the sidebar.
  const [view, setView] = useState<NextView>({ kind: "session", session: "global" });
  const [setupChosen, setSetupChosen] = useState(false);
  const [adding, setAdding] = useState(false);

  useEffect(() => {
    let disposed = false;
    let unsubscribe: (() => void) | undefined;
    void initialize(bridge).then((cleanup) => {
      if (disposed) cleanup();
      else unsubscribe = cleanup;
    });
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [bridge, initialize]);

  // Follow the connection: leave the setup assistant for the global orchestrator
  // once a server connects, and close the details panel when it disconnects.
  const [lastConnected, setLastConnected] = useState(connected);
  if (connected !== lastConnected) {
    setLastConnected(connected);
    if (connected && view.kind === "session" && view.session === "setup") {
      setView({ kind: "session", session: "global" });
    }
    if (!connected && (view.kind === "server" || view.kind === "workers")) {
      setView({ kind: "session", session: "global" });
    }
  }
  useEffect(() => {
    if (!connected) closePanel();
  }, [connected, closePanel]);

  const activeHost =
    hosts.find((host) => host.id === connection.host_id) ??
    hosts.find((host) => host.id === selectedHostId) ??
    hosts[0] ??
    null;

  if (!hostsLoaded) {
    return <div className="nx-welcome" aria-busy="true" />;
  }

  const addServerDialog = adding ? <AddHostWizard title="Add a server" onClose={() => setAdding(false)} /> : null;

  if (hosts.length === 0 && !setupChosen && view.kind !== "settings") {
    return (
      <>
        <Welcome
          onAddServer={() => setAdding(true)}
          onSetupWithAi={() => {
            setSetupChosen(true);
            setView({ kind: "session", session: "setup" });
          }}
        />
        {addServerDialog}
      </>
    );
  }

  const openSession = (project: string | null) =>
    setView({ kind: "session", session: project ? `project:${project}` : "global" });
  const openWorkers = (project: string) => setView({ kind: "workers", project });
  const openServer = () => setView({ kind: "server" });

  const serverLabel = connected && activeHost ? activeHost.label : null;
  const scopeProject =
    view.kind === "workers"
      ? view.project
      : view.kind === "session" && view.session.startsWith("project:")
        ? view.session.slice("project:".length)
        : null;

  let main;
  if (view.kind === "settings") {
    main = <SettingsView theme={theme} onTheme={setTheme} onOpenRaw={() => setRawOpen(true)} />;
  } else if (view.kind === "server" && serverLabel) {
    main = <ServerPage serverLabel={serverLabel} onOpenProject={openWorkers} />;
  } else if (view.kind === "workers" && serverLabel) {
    main = <WorkersPage key={`workers:${view.project}`} project={view.project} onOpenSession={() => openSession(view.project)} />;
  } else if (view.kind === "session" && (connected || view.session === "setup")) {
    main = (
      <ChatView key={`chat:${view.session}`} session={view.session} serverLabel={serverLabel} onOpenWorkers={openWorkers} />
    );
  } else if (activeHost) {
    main = (
      <ConnectPrompt
        serverLabel={activeHost.label}
        address={`${activeHost.ssh.user}@${activeHost.ssh.host}${activeHost.ssh.port !== 22 ? `:${activeHost.ssh.port}` : ""}`}
        state={connection.host_id === activeHost.id ? connection.state : "disconnected"}
        error={connection.host_id === activeHost.id ? (connection.error?.message ?? null) : null}
        onConnect={() => void connect(activeHost.id)}
        onSetupWithAi={() => setView({ kind: "session", session: "setup" })}
      />
    );
  } else {
    main = <ChatView key="chat:setup" session="setup" serverLabel={null} />;
  }

  return (
    <div
      className={`nx${side.dragging || drawer.dragging ? " nx--resizing" : ""}`}
      data-testid="next-shell"
      style={{ "--nx-side-w": `${side.width}px`, "--nx-panel-w": `${drawer.width}px` } as CSSProperties}
    >
      <Sidebar
        view={view}
        onNavigate={setView}
        onAddServer={() => setAdding(true)}
        onAddProject={() => setAddingProject(true)}
        theme={theme}
        onTheme={setTheme}
        resizeHandle={side.handleProps}
        resizing={side.dragging}
        onOpenRaw={() => setRawOpen(true)}
      />
      <div className={`nx-stage${panelOpen ? " nx-stage--panel" : ""}`}>
        <div className="nx-column">
          <ErrorNotice />
          {main}
        </div>
        {connected ? (
          <ContextPanel
            project={scopeProject}
            onOpenServer={openServer}
            onOpenWorkers={openWorkers}
            onOpenSession={(project) => openSession(project)}
            onOpenInbox={() => setInboxOpen(true)}
            resizeHandle={drawer.handleProps}
          />
        ) : null}
      </div>
      {addServerDialog}
      <InboxDrawer key={inboxOpen ? "inbox-open" : "inbox-closed"} open={inboxOpen} onClose={() => setInboxOpen(false)} />
      <RawDocumentDrawer open={rawOpen} onClose={() => setRawOpen(false)} />
      <SessionsDrawer
        key={`${sessionsOpen ? "open" : "closed"}-${sessionsIntent}`}
        open={sessionsOpen}
        onClose={closeSessions}
      />
      {addingProject && serverLabel ? (
        <AddProjectDialog
          serverLabel={serverLabel}
          existing={(fleet?.projects ?? []).map((project) => project.name)}
          onClose={() => setAddingProject(false)}
          onCreate={(request) => {
            setAddingProject(false);
            requestProject(request);
            setView({ kind: "session", session: "global" });
          }}
        />
      ) : null}
    </div>
  );
}
