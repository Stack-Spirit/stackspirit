/** Small pure formatters shared by the cloud panels and their tests. */

import type {
  BackupPlan,
  CloudMachine,
  MachinePresence,
  ManagedPlan,
  ProvisioningPhase,
  ProvisioningStatus,
} from "../cloud/types";

export function formatEuro(amount: number | null): string {
  if (amount === null || !Number.isFinite(amount)) return "price unavailable";
  return `€${amount.toFixed(2)}`;
}

export function formatUsd(amount: number | null): string {
  if (amount === null || !Number.isFinite(amount)) return "price unavailable";
  return `$${amount.toFixed(2)}`;
}

export function formatMonthly(amount: number | null): string {
  return amount === null ? "Price unavailable" : `${formatEuro(amount)}/mo`;
}

export function formatMonthlyUsd(amount: number | null): string {
  return amount === null ? "Price unavailable" : `${formatUsd(amount)}/mo`;
}

function numeric(value: unknown): number | null {
  if (typeof value === "number" && Number.isFinite(value)) return Math.max(0, value);
  if (typeof value === "string" && value.trim() !== "") {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) return Math.max(0, parsed);
  }
  return null;
}

function nestedNumeric(value: unknown, unit: "usd" | "eur"): number | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  return numeric(record[unit] ?? record[unit.toUpperCase()]);
}

/**
 * The Web client reports `/v1/plans` currency fields it receives. The shared
 * `ManagedPlan` type predates the USD field, so readers stay structural and the
 * panel falls back to EUR rather than guessing a conversion.
 */
export function planMonthlyUsd(plan: ManagedPlan): number | null {
  const extra = plan as ManagedPlan & {
    monthly_usd?: unknown;
    prices?: unknown;
    monthly?: unknown;
  };
  return (
    numeric(extra.monthly_usd) ??
    nestedNumeric(extra.prices, "usd") ??
    nestedNumeric(extra.monthly, "usd")
  );
}

export function planSetupFeeUsd(plan: ManagedPlan): number | null {
  const extra = plan as ManagedPlan & {
    setup_fee_usd?: unknown;
    setup_fee?: unknown;
  };
  return numeric(extra.setup_fee_usd) ?? nestedNumeric(extra.setup_fee, "usd");
}

export function backupMonthlyUsd(backup: BackupPlan): number | null {
  const extra = backup as BackupPlan & { monthly_usd?: unknown; prices?: unknown };
  return numeric(extra.monthly_usd) ?? nestedNumeric(extra.prices, "usd");
}

export function planSpecs(plan: ManagedPlan): string {
  const disk = plan.disk_kind ? `${plan.disk_gb} GB ${plan.disk_kind}` : `${plan.disk_gb} GB disk`;
  return `${plan.vcpu} vCPU · ${plan.ram_gb} GB RAM · ${disk}`;
}

/** Null when the tier has no one-time setup fee, so Home 8 can show it explicitly. */
export function setupFeeLabel(plan: ManagedPlan): string | null {
  if (plan.setup_fee_eur === null || plan.setup_fee_eur === 0) return null;
  return `One-time setup fee ${formatEuro(plan.setup_fee_eur)}`;
}

export function presenceLabel(status: MachinePresence): string {
  switch (status) {
    case "online":
      return "Online";
    case "offline":
      return "Offline";
    case "degraded":
      return "Degraded";
    case "revoked":
      return "Revoked";
  }
}

export function presenceTone(status: MachinePresence): string {
  switch (status) {
    case "online":
      return "ok";
    case "degraded":
      return "warn";
    case "revoked":
      return "danger";
    case "offline":
      return "muted";
  }
}

export function machineLabel(machine: CloudMachine): string {
  return machine.project ? `${machine.name} · ${machine.project}` : machine.name;
}

export function provisioningLabel(phase: ProvisioningPhase): string {
  switch (phase) {
    case "queued":
      return "Queued";
    case "provisioning":
      return "Provisioning";
    case "online":
      return "Online";
    case "failed":
      return "Failed";
  }
}

export function provisioningTone(phase: ProvisioningPhase): string {
  if (phase === "online") return "ok";
  if (phase === "failed") return "danger";
  if (phase === "queued") return "muted";
  return "warn";
}

export function provisioningProgress(provisioning: ProvisioningStatus): number | null {
  if (provisioning.progress_pct === null) return null;
  return Math.max(0, Math.min(100, Math.round(provisioning.progress_pct)));
}

export function isProvisioningLive(provisioning: ProvisioningStatus | null): boolean {
  return provisioning !== null && (provisioning.phase === "queued" || provisioning.phase === "provisioning");
}
