import { Badge, Button, EmptyState, Panel } from "../../components";
import {
  useConnection,
  useFleet,
  useFleetStore,
  useHosts,
  useSelectedHostId,
} from "../../state/fleetStore";
import { HostCard } from "./HostCard";
import { useConnectionPrompt } from "./useConnectionPrompt";

export function HostRail({ onManage }: { onManage: () => void }) {
  const hosts = useHosts();
  const connection = useConnection();
  const fleet = useFleet();
  const selectedHostId = useSelectedHostId();
  const latencyHistory = useFleetStore((state) => state.latencyHistory);
  const selectHost = useFleetStore((state) => state.selectHost);
  const disconnect = useFleetStore((state) => state.disconnect);
  const { requestConnect, connectionPrompt } = useConnectionPrompt();

  return (
    <Panel
      title="Hosts"
      actions={<Badge tone="unknown">{hosts.length}</Badge>}
      className="host-rail"
    >
      {hosts.length === 0 ? (
        <EmptyState
          icon="⌁"
          title="No VPS yet"
          body="Add an existing VOWP home over SSH. Nothing is installed on the server."
          actionLabel="Set up a host"
          onAction={onManage}
        />
      ) : (
        <div className="host-rail__list">
          {hosts.map((host) => (
            <HostCard
              key={host.id}
              host={host}
              connection={connection}
              fleet={fleet}
              latencyHistory={latencyHistory}
              selected={host.id === selectedHostId}
              compact
              testIdPrefix="host-rail-card"
              onSelect={() => selectHost(host.id)}
              onConnect={() => requestConnect(host)}
              onDisconnect={() => void disconnect(host.id)}
            />
          ))}
        </div>
      )}
      <Button variant="secondary" size="sm" className="host-rail__manage" onClick={onManage}>
        Manage hosts
      </Button>
      {connectionPrompt}
    </Panel>
  );
}
