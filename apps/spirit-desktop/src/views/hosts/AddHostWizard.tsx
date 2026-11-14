import { useState } from "react";

import { toSpiritError, type SpiritError } from "../../bridge/errors";
import type { HostKeyPin, HostKeyScan, HostProbe, NewHostProfile } from "../../bridge/wire";
import { Badge, Button, ErrorState, Modal } from "../../components";
import { useFleetStore } from "../../state/fleetStore";
import { FingerprintStep } from "./FingerprintStep";
import {
  emptyDraft,
  HostDraftFields,
  validateDraft,
  type DraftErrors,
} from "./HostDraftFields";
import { ProbeChecklist, probeAllowsSave } from "./ProbeChecklist";

export interface AddHostWizardProps {
  onClose: () => void;
  onSaved?: (label: string) => void;
}

type Step = 1 | 2 | 3;

export function AddHostWizard({ onClose, onSaved }: AddHostWizardProps) {
  const bridge = useFleetStore((state) => state.bridge);
  const addHost = useFleetStore((state) => state.addHost);
  const [step, setStep] = useState<Step>(1);
  const [draft, setDraft] = useState(() => ({
    ...emptyDraft(),
    ssh: { user: "owner", host: "", port: 22 },
  }));
  const [errors, setErrors] = useState<DraftErrors>({});
  const [scanning, setScanning] = useState(false);
  const [scanError, setScanError] = useState<SpiritError | null>(null);
  const [scan, setScan] = useState<HostKeyScan | null>(null);
  const [selectedSha, setSelectedSha] = useState<string | null>(null);
  const [verified, setVerified] = useState(false);
  const [pin, setPin] = useState<HostKeyPin | null>(null);
  const [probing, setProbing] = useState(false);
  const [probe, setProbe] = useState<HostProbe | null>(null);
  const [probeError, setProbeError] = useState<SpiritError | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<SpiritError | null>(null);

  async function startScan() {
    if (!bridge) return;
    setScanning(true);
    setScanError(null);
    setVerified(false);
    try {
      const result = await bridge.hosts.scanFingerprint(draft);
      setScan(result);
      const preferred = result.keys.find((key) => key.preferred) ?? result.keys[0];
      setSelectedSha(preferred?.sha256 ?? null);
    } catch (error) {
      setScanError(toSpiritError(error));
    } finally {
      setScanning(false);
    }
  }

  function nextFromAddress() {
    const nextErrors = validateDraft(draft);
    setErrors(nextErrors);
    if (Object.keys(nextErrors).length > 0) return;
    setStep(2);
    void startScan();
  }

  function trustFingerprint() {
    const key = scan?.keys.find((candidate) => candidate.sha256 === selectedSha);
    if (!key) return;
    const nextPin: HostKeyPin = {
      algorithm: key.algorithm,
      sha256: key.sha256,
      pinned_at: new Date().toISOString(),
    };
    setPin(nextPin);
    setStep(3);
    void startProbe(nextPin);
  }

  async function startProbe(nextPin: HostKeyPin | null) {
    if (!bridge || !nextPin) return;
    setProbing(true);
    setProbeError(null);
    try {
      setProbe(await bridge.hosts.probe(draft, nextPin));
    } catch (error) {
      setProbeError(toSpiritError(error));
    } finally {
      setProbing(false);
    }
  }

  async function save() {
    if (!pin) return;
    setSaving(true);
    setSaveError(null);
    try {
      const input: NewHostProfile = { ...draft, host_key: pin };
      const host = await addHost(input);
      onSaved?.(host.label);
      onClose();
    } catch (error) {
      setSaveError(toSpiritError(error));
    } finally {
      setSaving(false);
    }
  }

  const supervisorStage = probe?.stages.find((stage) => stage.stage === "supervisor");
  const saveAllowed = probeAllowsSave(probe);

  return (
    <Modal
      open
      title="Add your VPS"
      onClose={onClose}
      footer={
        step === 1 ? (
          <>
            <Button variant="secondary" onClick={onClose}>
              Cancel
            </Button>
            <Button variant="primary" onClick={nextFromAddress}>
              Next: verify host key
            </Button>
          </>
        ) : step === 3 ? (
          <>
            <Button
              variant="secondary"
              onClick={() => setStep(2)}
              disabled={saving || probing}
            >
              Back
            </Button>
            <Button
              variant="primary"
              onClick={() => void save()}
              disabled={!saveAllowed}
              loading={saving}
              title={
                saveAllowed
                  ? "Save this host profile"
                  : "Checks 1–4 must pass before this host can be saved."
              }
            >
              Save host
            </Button>
          </>
        ) : null
      }
    >
      <ol className="wizard-steps" aria-label="Add host steps">
        {[
          { number: 1, label: "Address" },
          { number: 2, label: "Verify host key" },
          { number: 3, label: "Test and save" },
        ].map((item) => (
          <li
            key={item.number}
            className={`wizard-steps__item${step === item.number ? " wizard-steps__item--current" : ""}`}
            aria-current={step === item.number ? "step" : undefined}
          >
            <span className="wizard-steps__number">{item.number}</span>
            {item.label}
          </li>
        ))}
      </ol>

      {step === 1 ? (
        <form
          className="wizard-form"
          onSubmit={(event) => {
            event.preventDefault();
            nextFromAddress();
          }}
        >
          <p className="muted">
            StackSpirit connects to an existing VOWP home over SSH. You need SSH key
            access and VOWP installed; nothing else.
          </p>
          <HostDraftFields draft={draft} errors={errors} onChange={setDraft} />
          <ul className="inline-checks" aria-label="Prerequisites">
            <li>
              <span aria-hidden="true">○</span> SSH key or agent
            </li>
            <li>
              <span aria-hidden="true">○</span> VOWP on the host
            </li>
            <li>
              <span aria-hidden="true">○</span> Host key verified
            </li>
          </ul>
        </form>
      ) : null}

      {step === 2 ? (
        <FingerprintStep
          scan={scan}
          scanning={scanning}
          scanError={scanError}
          selectedSha={selectedSha}
          verified={verified}
          onSelect={setSelectedSha}
          onVerified={setVerified}
          onRescan={() => void startScan()}
          onBack={() => setStep(1)}
          onTrust={trustFingerprint}
          busy={scanning}
        />
      ) : null}

      {step === 3 ? (
        <div className="probe-step">
          <p className="muted">
            Testing <strong className="mono">{draft.ssh.user}@{draft.ssh.host}:{draft.ssh.port}</strong>{" "}
            against the confirmed fingerprint.
          </p>
          {probeError ? (
            <ErrorState
              error={probeError}
              onRetry={() => void startProbe(pin)}
              retryLabel="Retry probe"
            />
          ) : (
            <ProbeChecklist probe={probe} busy={probing} />
          )}
          {supervisorStage && !supervisorStage.ok && saveAllowed ? (
            <div className="notice notice--warn" role="status">
              <Badge tone="warn">informational</Badge> The supervisor is stopped. You can save
              and connect, but the host card will read “Connected · supervisor stopped”.
            </div>
          ) : null}
          {saveError ? <ErrorState error={saveError} /> : null}
        </div>
      ) : null}
    </Modal>
  );
}
