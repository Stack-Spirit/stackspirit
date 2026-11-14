import { useState } from "react";

import { toSpiritError, type SpiritError } from "../../bridge/errors";
import type { HostProfile } from "../../bridge/wire";
import { Button, ErrorState, Modal } from "../../components";
import { useFleetStore } from "../../state/fleetStore";
import {
  HostDraftFields,
  validateDraft,
  type DraftErrors,
} from "./HostDraftFields";

export function EditHostDialog({
  host,
  onClose,
  onSaved,
}: {
  host: HostProfile;
  onClose: () => void;
  onSaved?: () => void;
}) {
  const updateHost = useFleetStore((state) => state.updateHost);
  const [draft, setDraft] = useState({
    label: host.label,
    ssh: { ...host.ssh },
    identity_file: host.identity_file,
    vowp_home: host.vowp_home,
    default_project: host.default_project,
  });
  const [errors, setErrors] = useState<DraftErrors>({});
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<SpiritError | null>(null);

  async function save() {
    const nextErrors = validateDraft(draft);
    setErrors(nextErrors);
    if (Object.keys(nextErrors).length > 0) return;
    setSaving(true);
    setSaveError(null);
    try {
      await updateHost(host.id, draft);
      onSaved?.();
      onClose();
    } catch (error) {
      setSaveError(toSpiritError(error));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Modal
      open
      title={`Edit ${host.label}`}
      onClose={onClose}
      footer={
        <>
          <Button variant="secondary" onClick={onClose} disabled={saving}>
            Cancel
          </Button>
          <Button variant="primary" onClick={() => void save()} loading={saving}>
            Save changes
          </Button>
        </>
      }
    >
      <HostDraftFields draft={draft} errors={errors} onChange={setDraft} disabled={saving} />
      {saveError ? <ErrorState error={saveError} /> : null}
    </Modal>
  );
}
