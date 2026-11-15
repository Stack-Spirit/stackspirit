import { Button } from "../../components";

export interface ConnectionControlProps {
  state: string;
  hostLabel: string;
  onConnect: () => void;
  onDisconnect: () => void;
  disabled?: boolean;
}

export function ConnectionControl({
  state,
  hostLabel,
  onConnect,
  onDisconnect,
  disabled = false,
}: ConnectionControlProps) {
  if (state === "connected" || state === "stale") {
    return (
      <Button variant="secondary" size="sm" onClick={onDisconnect} disabled={disabled}>
        Disconnect
      </Button>
    );
  }
  if (state === "connecting" || state === "disconnecting") {
    return (
      <Button variant="secondary" size="sm" onClick={onDisconnect} disabled={disabled}>
        Cancel
      </Button>
    );
  }
  return (
    <Button
      variant="primary"
      size="sm"
      onClick={onConnect}
      disabled={disabled}
      aria-label={`Connect to ${hostLabel}`}
    >
      Connect
    </Button>
  );
}
