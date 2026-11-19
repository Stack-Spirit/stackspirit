import { errorTitle, type SpiritError } from "../bridge/errors";

/**
 * Inline typed failure for a cluster form or promotion. It keeps the dialog's fields
 * and shows the stable code, the safe message and the next step; raw stderr, paths,
 * targets and tokens are never part of a bridge error.
 */
export function ClusterErrorAlert({ error }: { error: SpiritError }) {
  return (
    <div className="nx-cluster__error" role="alert" data-testid="cluster-error">
      <strong>{errorTitle(error)}</strong>
      <code>{error.code}</code>
      <p>{error.message}</p>
      {error.hint ? <p>{error.hint}</p> : null}
    </div>
  );
}
