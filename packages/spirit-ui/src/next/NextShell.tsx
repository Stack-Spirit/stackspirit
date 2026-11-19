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
import { useCluster, useClusterStore } from "./clusterStore";
import { initializeConversations, useConversations } from "./conversations";
import { ContextPanel } from "./ContextPanel";
import { FormClusterDialog } from "./FormClusterDialog";
import { PromoteBackupDialog } from "./PromoteBackupDialog";
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
 * - A saved server that is not connected: sidebar + Connect and a local setup chat.
 * - Connected: sidebar (server health, global orchestrator, projects with their
 *   orchestrator and workers), the main view, and a details panel that is closed
 *   until it is toggled or something is opened in it.
 *
 * Main views: an orchestrator session, the server page, a project's workers page,
 * or settings. Hosts, connection, fleet and capacity come from the existing store
 * and bridge; orchestrator chat is driven by `bridge.chat` (see conversations.ts).
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
  const cluster = useCluster();
  const loadCluster = useClusterStore((state) => state.loadCluster);
  const [theme, setTheme] = useNextTheme();
  const side = useResizable({ storageKey: "stackspirit.sidebarWidth", initial: 272, min: 220, max: 420, grows: "right" });
  const drawer = useResizable({ storageKey: "stackspirit.panelWidth", initial: 400, min: 320, max: 640, grows: "left" });
  const [addingProject, setAddingProject] = useState(false);
  const [clusterFormOpen, setClusterFormOpen] = useState(false);
  const [promoteOpen, setPromoteOpen] = useState(false);
  const [promoteTarget, setPromoteTarget] = useState<string | null>(null);
  // Classic drawers, reused unchanged: inbox answers, .asif sessions, raw document.
  const [inboxOpen, setInboxOpen] = useState(false);
  const [rawOpen, setRawOpen] = useState(false);
  const sessionsOpen = useFleetStore((state) => state.sessionsOpen);
  const sessionsIntent = useFleetStore((state) => state.sessionsIntent);
  const closeSessions = useFleetStore((state) => state.closeSessions);

  const connected = connection.state === "connected" || connection.state === "stale";
  // A saved cluster belongs to the saved servers, not to the active connection. Read it
  // once the bridge is initialized and again whenever the active server changes; the
  // store skips preview, and the background read never delays the shell.
  useEffect(() => {
    if (hostsLoaded) void loadCluster();
  }, [bridge, hostsLoaded, connection.host_id, connection.state, loadCluster]);
  // "global" shows the Connect prompt until a saved server is connected; the setup
  // assistant is opened explicitly from the welcome page, the Connect prompt or the
  // sidebar, and chats locally without connecting.
  const [view, setView] = useState<NextView>({ kind: "session", session: "global" });
  const [setupChosen, setSetupChosen] = useState(false);
  const [adding, setAdding] = useState(false);
  // "Set up a model" in the composer opens Settings with the Engine card visible;
  // opening Settings normally keeps it folded.
  const [settingsEngine, setSettingsEngine] = useState(false);
  const navigate = (next: NextView) => {
    if (next.kind === "settings") setSettingsEngine(false);
    setView(next);
  };

  useEffect(() => {
    let disposed = false;
    let unsubscribe: (() => void) | undefined;
    // Subscribe the conversation cache to the same bridge once. The returned cleanup
    // removes the chat event listener and the fleet listener together.
    const closeConversations = initializeConversations(bridge);
    void initialize(bridge).then((cleanup) => {
      if (disposed) cleanup();
      else unsubscribe = cleanup;
    });
    return () => {
      disposed = true;
      unsubscribe?.();
      closeConversations();
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
  // The setup chat can show a saved server without connecting. It is read-only and
  // never exposes a key path; ChatView derives only the key file's name.
  const setupServer =
    view.kind === "session" && view.session === "setup" && hosts.length > 0 ? activeHost : null;
  const scopeProject =
    view.kind === "workers"
      ? view.project
      : view.kind === "session" && view.session.startsWith("project:")
        ? view.session.slice("project:".length)
        : null;

  let main;
  if (view.kind === "settings") {
    main = (
      <SettingsView
        theme={theme}
        onTheme={setTheme}
        onOpenRaw={() => setRawOpen(true)}
        initialAdvanced={settingsEngine}
      />
    );
  } else if (view.kind === "server" && serverLabel) {
    main = (
      <ServerPage
        serverLabel={serverLabel}
        onOpenProject={openWorkers}
        onFormCluster={() => setClusterFormOpen(true)}
        onPromote={(target) => {
          setPromoteTarget(target);
          setPromoteOpen(true);
        }}
      />
    );
  } else if (view.kind === "workers" && serverLabel) {
    main = <WorkersPage key={`workers:${view.project}`} project={view.project} onOpenSession={() => openSession(view.project)} />;
  } else if (view.kind === "session" && (connected || view.session === "setup")) {
    main = (
      <ChatView
        key={`chat:${view.session}`}
        session={view.session}
        serverLabel={serverLabel}
        savedServer={setupServer}
        onOpenWorkers={openWorkers}
        onOpenEngineSettings={() => {
          setSettingsEngine(true);
          setView({ kind: "settings" });
        }}
      />
    );
  } else if (activeHost) {
    main = (
      <ConnectPrompt
        serverLabel={activeHost.label}
        address={`${activeHost.ssh.user}@${activeHost.ssh.host}${activeHost.ssh.port !== 22 ? `:${activeHost.ssh.port}` : ""}`}
        state={connection.host_id === activeHost.id ? connection.state : "disconnected"}
        error={connection.host_id === activeHost.id ? (connection.error?.message ?? null) : null}
        onConnect={() => void connect(activeHost.id)}
        onChatWithSetupAssistant={() => setView({ kind: "session", session: "setup" })}
      />
    );
  } else {
    main = <ChatView key="chat:setup" session="setup" serverLabel={null} savedServer={null} />;
  }

  return (
    <div
      className={`nx${side.dragging || drawer.dragging ? " nx--resizing" : ""}`}
      data-testid="next-shell"
      style={{ "--nx-side-w": `${side.width}px`, "--nx-panel-w": `${drawer.width}px` } as CSSProperties}
    >
      <Sidebar
        view={view}
        onNavigate={navigate}
        onAddServer={() => setAdding(true)}
        onAddProject={() => setAddingProject(true)}
        onFormCluster={() => setClusterFormOpen(true)}
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
      {clusterFormOpen ? (
        <FormClusterDialog
          hosts={hosts}
          initial={cluster?.config ?? null}
          onClose={() => setClusterFormOpen(false)}
        />
      ) : null}
      {promoteOpen ? (
        <PromoteBackupDialog
          hosts={hosts}
          initialTarget={promoteTarget}
          onClose={() => setPromoteOpen(false)}
          onConnect={(hostId) => {
            void connect(hostId);
            setView({ kind: "server" });
          }}
        />
      ) : null}
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
