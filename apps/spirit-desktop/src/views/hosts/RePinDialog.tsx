import { useState } from "react";

import { toSpiritError, type SpiritError } from "../../bridge/errors";
import type { HostKeyPin, HostKeyScan, HostProfile } from "../../bridge/wire";
import { Modal } from "../../components";
import { useFleetStore } from "../../state/fleetStore";
import { FingerprintStep } from "./FingerprintStep";

export function RePinDialog({
  host,
  onClose,
  onSaved,
}: {
  host: HostProfile;
  onClose: () => void;
  onSaved?: () => void;
}) {
  const bridge = useFleetStore((state) => state.bridge);
  const trustFingerprint = useFleetStore((state) => state.trustFingerprint);
  const [scan, setScan] = useState<HostKeyScan | null>(null);
  const [scanning, setScanning] = useState(false);
  const [scanError, setScanError] = useState<SpiritError | null>(null);
  const [selectedSha, setSelectedSha] = useState<string | null>(null);
  const [verified, setVerified] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<SpiritError | null>(null);

  async function scanNow() {
    if (!bridge) return;
    setScanning(true);
    setScanError(null);
    setVerified(false);
    setSaveError(null);
    try {
      const result = await bridge.hosts.scanFingerprint(host);
      setScan(result);
      const preferred = result.keys.find((key) => key.preferred) ?? result.keys[0];
      setSelectedSha(preferred?.sha256 ?? null);
    } catch (error) {
      setScanError(toSpiritError(error));
    } finally {
      setScanning(false);
    }
  }


  async function trust() {
    const key = scan?.keys.find((candidate) => candidate.sha256 === selectedSha);
    if (!key) return;
    const pin: HostKeyPin = {
      algorithm: key.algorithm,
      sha256: key.sha256,
      pinned_at: new Date().toISOString(),
    };
    setSaving(true);
    try {
      await trustFingerprint(host.id, pin);
      onSaved?.();
      onClose();
    } catch (error) {
      setSaveError(toSpiritError(error));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Modal open title={`Re-pin host key for ${host.label}`} onClose={onClose}>
      <p className="muted">
        A re-pin replaces the stored fingerprint only after you verify the new key on
        the server. StackSpirit never accepts a changed key automatically.
      </p>
      {saveError ? <div className="notice notice--danger">{saveError.message}</div> : null}
      <FingerprintStep
        scan={scan}
        scanning={scanning}
        scanError={scanError}
        selectedSha={selectedSha}
        verified={verified}
        onSelect={setSelectedSha}
        onVerified={setVerified}
        onRescan={() => void scanNow()}
        onTrust={() => void trust()}
        trustLabel="Re-pin this key"
        busy={saving}
      />
      <p className="muted mono small">Current pin: {host.host_key.sha256}</p>
    </Modal>
  );
}
