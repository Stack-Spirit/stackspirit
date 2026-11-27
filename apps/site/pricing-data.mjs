/**
 * Public managed-Home catalog helpers (EUR + USD).
 *
 * Fetches `GET https://api.stackspirit.io/v1/plans` and falls back to these
 * published values when unreachable. Supports both the live Cloudflare Worker
 * cent-based schema (EUR + USD) and the legacy euro-float schema.
 */

export const PLANS_URL = "https://api.stackspirit.io/v1/plans";

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
      monthly_usd: 7.2,
      setup_fee_usd: 8.4,
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
      monthly_usd: 9.8,
      setup_fee_usd: 0,
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
      monthly_eur: 16.8,
      setup_fee_eur: 0,
      monthly_usd: 18.2,
      setup_fee_usd: 0,
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
      monthly_usd: 32.5,
      setup_fee_usd: 0,
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
      monthly_eur: 44.4,
      setup_fee_eur: 0,
      monthly_usd: 48,
      setup_fee_usd: 0,
      currency: "EUR",
      provider_spec: "Cloud VPS 16",
    }),
  ]),
  backups: Object.freeze({
    monthly_eur: 3,
    monthly_usd: 3.5,
    recommended: true,
    provider: "cloudflare-r2",
  }),
});

const FALLBACK_BY_ID = new Map(FALLBACK_CATALOG.plans.map((p) => [p.id, p]));
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
    const rawName = plan.name ?? plan.label;
    if (!isBoundedString(rawName, 80)) throw new Error("invalid plan name");
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

    const curr = String(plan.currency ?? "EUR").toUpperCase();
    if (curr !== "EUR") throw new Error("unsupported currency");

    const monthlyEur = isFiniteNumber(plan.monthly_eur, 0, 100_000)
      ? plan.monthly_eur
      : isFiniteNumber(plan.monthly_cents, 0, 10_000_000)
        ? plan.monthly_cents / 100
        : null;
    if (monthlyEur === null) throw new Error("invalid monthly price");

    const setupEur =
      plan.setup_fee_eur !== undefined && plan.setup_fee_eur !== null
        ? isFiniteNumber(plan.setup_fee_eur, 0, 100_000)
          ? plan.setup_fee_eur
          : (() => {
              throw new Error("invalid setup fee");
            })()
        : plan.setup_fee_cents !== undefined && plan.setup_fee_cents !== null
          ? isFiniteNumber(plan.setup_fee_cents, 0, 10_000_000)
            ? plan.setup_fee_cents / 100
            : (() => {
                throw new Error("invalid setup fee");
              })()
          : 0;

    const fb = FALLBACK_BY_ID.get(id);
    const usdObj = isRecord(plan.prices) && isRecord(plan.prices.usd) ? plan.prices.usd : null;
    const monthlyUsd = isFiniteNumber(plan.monthly_usd, 0, 100_000)
      ? plan.monthly_usd
      : isFiniteNumber(plan.monthly_usd_cents, 0, 10_000_000)
        ? plan.monthly_usd_cents / 100
        : usdObj && isFiniteNumber(usdObj.monthly_cents, 0, 10_000_000)
          ? usdObj.monthly_cents / 100
          : fb?.monthly_usd ?? Math.round(monthlyEur * 1.09 * 100) / 100;

    const setupUsd = isFiniteNumber(plan.setup_fee_usd, 0, 100_000)
      ? plan.setup_fee_usd
      : isFiniteNumber(plan.setup_fee_usd_cents, 0, 10_000_000)
        ? plan.setup_fee_usd_cents / 100
        : usdObj && isFiniteNumber(usdObj.setup_fee_cents, 0, 10_000_000)
          ? usdObj.setup_fee_cents / 100
          : fb?.setup_fee_usd ?? (setupEur > 0 ? Math.round(setupEur * 1.09 * 100) / 100 : 0);

    return {
      id,
      name: rawName.trim(),
      vcpu: plan.vcpu,
      ram_gb: plan.ram_gb,
      disk_gb: plan.disk_gb,
      disk_kind: optionalString(plan.disk_kind, 32),
      monthly_eur: monthlyEur,
      setup_fee_eur: setupEur,
      monthly_usd: monthlyUsd,
      setup_fee_usd: setupUsd,
      currency: "EUR",
      provider_spec: optionalString(plan.provider_spec ?? plan.contabo_product, 80),
    };
  });

  // Live API exposes addons[], legacy tests expose backups{}.
  const rawBackup = isRecord(value.backups)
    ? value.backups
    : Array.isArray(value.addons)
      ? value.addons.find((a) => isRecord(a) && a.id === "backup-r2")
      : null;

  if (!isRecord(rawBackup) || typeof rawBackup.recommended !== "boolean") {
    throw new Error("invalid backup plan");
  }
  const backupEur = isFiniteNumber(rawBackup.monthly_eur, 0, 100_000)
    ? rawBackup.monthly_eur
    : isFiniteNumber(rawBackup.monthly_cents, 0, 10_000_000)
      ? rawBackup.monthly_cents / 100
      : null;
  const backupUsdObj = isRecord(rawBackup.prices) && isRecord(rawBackup.prices.usd) ? rawBackup.prices.usd : null;
  const backupUsd = isFiniteNumber(rawBackup.monthly_usd, 0, 100_000)
    ? rawBackup.monthly_usd
    : isFiniteNumber(rawBackup.monthly_usd_cents, 0, 10_000_000)
      ? rawBackup.monthly_usd_cents / 100
      : backupUsdObj && isFiniteNumber(backupUsdObj.monthly_cents, 0, 10_000_000)
        ? backupUsdObj.monthly_cents / 100
        : FALLBACK_CATALOG.backups.monthly_usd;

  return {
    plans,
    backups: {
      monthly_eur: backupEur,
      monthly_usd: backupUsd,
      recommended: rawBackup.recommended,
      provider: optionalString(rawBackup.provider, 64) ?? "cloudflare-r2",
    },
  };
}

export function formatMoney(amount, currency = "eur") {
  if (typeof amount !== "number" || !Number.isFinite(amount)) return "Price unavailable";
  const hasCents = Math.round(amount * 100) % 100 !== 0;
  const symbol = currency === "usd" ? "$" : "€";
  return `${symbol}${amount.toFixed(hasCents ? 2 : 0)}`;
}

export function formatEuro(amount) {
  return formatMoney(amount, "eur");
}

export function formatMonthly(amount, currency = "eur") {
  if (amount === null || amount === undefined || !Number.isFinite(amount)) {
    return "Price unavailable";
  }
  return `${formatMoney(amount, currency)}/mo`;
}

export function planSpecs(plan) {
  const disk = plan.disk_kind
    ? `${plan.disk_gb} GB ${plan.disk_kind}`
    : `${plan.disk_gb} GB disk`;
  return `${plan.vcpu} vCPU · ${plan.ram_gb} GB RAM · ${disk}`;
}

export function setupFeeLabel(plan, currency = "eur") {
  const fee = currency === "usd" ? plan.setup_fee_usd : plan.setup_fee_eur;
  if (fee === null || fee === undefined || fee === 0) return null;
  return `+ ${formatMoney(fee, currency)} one-time setup`;
}

export function backupPriceLabel(backups, currency = "eur") {
  const amount = currency === "usd" ? backups.monthly_usd : backups.monthly_eur;
  if (amount === null || amount === undefined) {
    return "Priced separately at checkout.";
  }
  return `${formatMonthly(amount, currency)} if enabled.`;
}
