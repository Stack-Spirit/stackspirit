/** Small pure formatters shared by the cloud panels and their tests. */

import type {
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

export function formatMonthly(amount: number | null): string {
  return amount === null ? "Price unavailable" : `${formatEuro(amount)}/mo`;
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
