/**
 * Public managed-Home catalog helpers.
 *
 * The marketing site asks `GET https://api.stackspirit.io/v1/plans` for the
 * published prices and falls back to these values when the catalog is
 * unreachable or malformed. The numbers match docs/STRATEGY.md §6: Contabo's
 * regular monthly rate plus the 20% StackSpirit markup. The optional off-site
 * encrypted R2 backups are always presented off by default.
 *
 * This module is intentionally dependency-free and importable by the
 * verification script under Node as well as by the browser.
 */

export const PLANS_URL = "https://api.stackspirit.io/v1/plans";

/** Prices in euro. Home 8 is the only tier with a one-time setup fee. */
export const FALLBACK_CATALOG = Object.freeze({
  plans: Object.freeze([
    Object.freeze({
      id: "home-8",
      name: "Home 8",
      vcpu: 4,
      ram_gb: 8,
      disk_gb: 100,
      disk_kind: "SSD",
      monthly_eur: 6.6,
      setup_fee_eur: 7.8,
      currency: "EUR",
      provider_spec: "Cloud VPS 4",
    }),
    Object.freeze({
      id: "home-12",
      name: "Home 12",
      vcpu: 6,
      ram_gb: 12,
      disk_gb: 200,
      disk_kind: "SSD",
      monthly_eur: 9,
      setup_fee_eur: 0,
      currency: "EUR",
      provider_spec: "Cloud VPS 6",
    }),
    Object.freeze({
      id: "home-24",
      name: "Home 24",
      vcpu: 8,
      ram_gb: 24,
      disk_gb: 300,
      disk_kind: "SSD",
      monthly_eur: 17,
      setup_fee_eur: 0,
      currency: "EUR",
      provider_spec: "Cloud VPS 8",
    }),
    Object.freeze({
      id: "home-48",
      name: "Home 48",
      vcpu: 12,
      ram_gb: 48,
      disk_gb: 400,
      disk_kind: "SSD",
      monthly_eur: 30,
      setup_fee_eur: 0,
      currency: "EUR",
      provider_spec: "Cloud VPS 12",
    }),
    Object.freeze({
      id: "home-64",
      name: "Home 64",
      vcpu: 16,
      ram_gb: 64,
      disk_gb: 500,
      disk_kind: "SSD",
      monthly_eur: 44,
      setup_fee_eur: 0,
      currency: "EUR",
      provider_spec: "Cloud VPS 16",
    }),
  ]),
  backups: Object.freeze({
    monthly_eur: 3.5,
    recommended: true,
    provider: "cloudflare-r2",
  }),
});

const MAX_PLANS = 20;

function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isFiniteNumber(value, min, max) {
  return typeof value === "number" && Number.isFinite(value) && value >= min && value <= max;
}

function isBoundedString(value, maxLength) {
  return typeof value === "string" && value.trim().length > 0 && value.length <= maxLength;
}

function optionalString(value, maxLength) {
  if (value === null || value === undefined) return null;
  if (typeof value === "string" && value.trim() === "") return null;
  if (!isBoundedString(value, maxLength)) throw new Error("invalid optional text");
  return value.trim();
}

/**
 * Validate and copy an API catalog. Anything unexpected rejects the whole
 * response so the site keeps its standard prices rather than rendering a
 * partially trusted catalog. Unknown fields (including the server's
 * `default_enabled`) are deliberately ignored.
 */
export function parseCatalog(value) {
  if (!isRecord(value) || !Array.isArray(value.plans)) {
    throw new Error("invalid plan catalog");
  }
  if (value.plans.length === 0 || value.plans.length > MAX_PLANS) {
    throw new Error("invalid plan count");
  }

  const seen = new Set();
  const plans = value.plans.map((plan) => {
    if (!isRecord(plan)) throw new Error("invalid plan");
    if (!isBoundedString(plan.id, 64)) throw new Error("invalid plan id");
    if (!isBoundedString(plan.name, 80)) throw new Error("invalid plan name");
    const id = plan.id.trim();
    if (seen.has(id)) throw new Error("duplicate plan id");
    seen.add(id);
    if (!Number.isInteger(plan.vcpu) || plan.vcpu < 1 || plan.vcpu > 1024) {
      throw new Error("invalid vcpu");
    }
    if (!Number.isInteger(plan.ram_gb) || plan.ram_gb < 1 || plan.ram_gb > 4096) {
      throw new Error("invalid ram");
    }
    if (!Number.isInteger(plan.disk_gb) || plan.disk_gb < 1 || plan.disk_gb > 1_000_000) {
      throw new Error("invalid disk");
    }
    if (!isFiniteNumber(plan.monthly_eur, 0, 100_000)) throw new Error("invalid monthly price");
    const setup = plan.setup_fee_eur;
    if (
      setup !== null &&
      setup !== undefined &&
      !isFiniteNumber(setup, 0, 100_000)
    ) {
      throw new Error("invalid setup fee");
    }
    if (plan.currency !== "EUR") throw new Error("unsupported currency");

    return {
      id,
      name: plan.name.trim(),
      vcpu: plan.vcpu,
      ram_gb: plan.ram_gb,
      disk_gb: plan.disk_gb,
      disk_kind: optionalString(plan.disk_kind, 32),
      monthly_eur: plan.monthly_eur,
      setup_fee_eur: setup === undefined || setup === null ? null : setup,
      currency: "EUR",
      provider_spec: optionalString(plan.provider_spec, 80),
    };
  });

  if (!isRecord(value.backups) || typeof value.backups.recommended !== "boolean") {
    throw new Error("invalid backup plan");
  }
  const backupMonthly = value.backups.monthly_eur;
  if (
    backupMonthly !== null &&
    backupMonthly !== undefined &&
    !isFiniteNumber(backupMonthly, 0, 100_000)
  ) {
    throw new Error("invalid backup price");
  }

  return {
    plans,
    backups: {
      monthly_eur: backupMonthly === undefined || backupMonthly === null ? null : backupMonthly,
      recommended: value.backups.recommended,
      provider: optionalString(value.backups.provider, 64),
    },
  };
}

/** `€6.60` for fractional prices, `€44` for whole-euro prices. */
export function formatEuro(amount) {
  if (typeof amount !== "number" || !Number.isFinite(amount)) return "Price unavailable";
  const hasCents = Math.round(amount * 100) % 100 !== 0;
  return `€${amount.toFixed(hasCents ? 2 : 0)}`;
}

export function formatMonthly(amount) {
  if (amount === null || amount === undefined || !Number.isFinite(amount)) {
    return "Price unavailable";
  }
  return `${formatEuro(amount)}/mo`;
}

export function planSpecs(plan) {
  const disk = plan.disk_kind
    ? `${plan.disk_gb} GB ${plan.disk_kind}`
    : `${plan.disk_gb} GB disk`;
  return `${plan.vcpu} vCPU · ${plan.ram_gb} GB RAM · ${disk}`;
}

/** Null when the tier has no one-time setup fee. */
export function setupFeeLabel(plan) {
  if (plan.setup_fee_eur === null || plan.setup_fee_eur === 0) return null;
  return `+ ${formatEuro(plan.setup_fee_eur)} one-time setup`;
}

export function backupPriceLabel(backups) {
  if (backups.monthly_eur === null || backups.monthly_eur === undefined) {
    return "Priced separately at checkout.";
  }
  return `${formatMonthly(backups.monthly_eur)} if enabled.`;
}
