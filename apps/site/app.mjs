/**
 * StackSpirit marketing site behaviour.
 *
 * Progressive enhancement only: every page is complete without this module.
 * Handles quickstart tabs, copy buttons, EUR/USD currency toggle, live
 * managed-Home catalog fetch with local fallback, and the pixel-film craft.
 */

import { initCraft } from "./craft.mjs";
import {
  FALLBACK_CATALOG,
  PLANS_URL,
  backupPriceLabel,
  formatMoney,
  formatMonthly,
  parseCatalog,
  planSpecs,
  setupFeeLabel,
} from "./pricing-data.mjs";

document.documentElement.classList.add("has-js");

const PLANS_CTA = "https://app.stackspirit.io/cloud";
const REQUEST_TIMEOUT_MS = 5_000;
let currentCatalog = FALLBACK_CATALOG;
let currentCurrency = "eur";

function byId(id) {
  return document.getElementById(id);
}

function initYear() {
  const year = byId("year");
  if (year) year.textContent = String(new Date().getFullYear());
}

function initTabs() {
  const tablist = document.querySelector(".tablist");
  if (!tablist) return;

  const tabs = Array.from(tablist.querySelectorAll('[role="tab"]'));
  const panels = tabs.map((tab) => byId(tab.getAttribute("aria-controls") ?? ""));
  if (tabs.length === 0 || panels.some((panel) => panel === null)) return;

  function select(index, moveFocus) {
    tabs.forEach((tab, current) => {
      const selected = current === index;
      tab.setAttribute("aria-selected", selected ? "true" : "false");
      tab.tabIndex = selected ? 0 : -1;
      panels[current].hidden = !selected;
    });
    if (moveFocus) tabs[index].focus();
  }

  tabs.forEach((tab, index) => {
    tab.addEventListener("click", () => select(index, false));
    tab.addEventListener("keydown", (event) => {
      const last = tabs.length - 1;
      let next;
      switch (event.key) {
        case "ArrowRight":
          next = index === last ? 0 : index + 1;
          break;
        case "ArrowLeft":
          next = index === 0 ? last : index - 1;
          break;
        case "Home":
          next = 0;
          break;
        case "End":
          next = last;
          break;
        default:
          return;
      }
      event.preventDefault();
      select(next, true);
    });
  });

  const initial = Math.max(
    0,
    tabs.findIndex((tab) => tab.getAttribute("aria-selected") === "true"),
  );
  select(initial, false);
}

function legacyCopy(text) {
  if (typeof document.execCommand !== "function") {
    return Promise.reject(new Error("copy unavailable"));
  }
  const field = document.createElement("textarea");
  field.value = text;
  field.setAttribute("readonly", "");
  field.className = "visually-hidden";
  document.body.append(field);
  field.select();
  const copied = document.execCommand("copy");
  field.remove();
  return copied ? Promise.resolve() : Promise.reject(new Error("copy failed"));
}

function copyWithFallback(text) {
  if (navigator.clipboard && typeof navigator.clipboard.writeText === "function") {
    return navigator.clipboard.writeText(text).catch(() => legacyCopy(text));
  }
  return legacyCopy(text);
}

function initCopyButtons() {
  const status = byId("copy-status");
  for (const button of document.querySelectorAll("[data-copy]")) {
    button.addEventListener("click", () => {
      const text = button.getAttribute("data-copy") ?? "";
      if (!text) return;
      copyWithFallback(text)
        .then(() => {
          button.dataset.copied = "true";
          button.textContent = "Copied";
          if (status) status.textContent = `Copied: ${text}`;
          window.setTimeout(() => {
            delete button.dataset.copied;
            button.textContent = "Copy";
          }, 1_800);
        })
        .catch(() => {
          if (status) {
            status.textContent = "Copy failed. Select the command and copy it manually.";
          }
        });
    });
  }
}

function planCard(plan, currency) {
  const item = document.createElement("li");
  item.className = "plan-card";
  item.dataset.plan = plan.id;

  const head = document.createElement("div");
  head.className = "plan-card__head";

  const name = document.createElement("h3");
  name.textContent = plan.name;

  const amount = currency === "usd" ? plan.monthly_usd : plan.monthly_eur;
  const price = document.createElement("p");
  price.className = "plan-card__price";
  price.append(document.createTextNode(formatMoney(amount, currency)));
  const suffix = document.createElement("span");
  suffix.textContent = "/mo";
  price.append(suffix);

  head.append(name, price);

  const spec = document.createElement("p");
  spec.className = "plan-card__spec";
  spec.textContent = planSpecs(plan);

  const setup = document.createElement("p");
  const setupLabel = setupFeeLabel(plan, currency);
  setup.className = setupLabel
    ? "plan-card__setup"
    : "plan-card__setup plan-card__setup--none";
  setup.textContent = setupLabel ?? "No setup fee";

  const cta = document.createElement("a");
  cta.className = "plan-card__cta";
  cta.href = `${PLANS_CTA}?plan=${encodeURIComponent(plan.id)}&currency=${currency}`;
  cta.setAttribute("aria-label", `View ${plan.name} in the Web App`);
  cta.textContent = `Deploy ${plan.name}`;

  item.append(head, spec);
  if (plan.provider_spec) {
    const provider = document.createElement("p");
    provider.className = "plan-card__provider";
    provider.textContent = plan.provider_spec;
    item.append(provider);
  }
  item.append(setup, cta);
  return item;
}

function updateBackups(backups) {
  const price = byId("backups-price");
  if (price) price.textContent = backupPriceLabel(backups, currentCurrency);
  const recommended = byId("backup-recommended");
  if (recommended) recommended.hidden = !backups.recommended;
  const input = byId("backups");
  const status = byId("backups-status");
  if (input instanceof HTMLInputElement && input.checked && status) {
    const amt = currentCurrency === "usd" ? backups.monthly_usd : backups.monthly_eur;
    status.textContent = `Backups selected: ${formatMonthly(amt, currentCurrency)}. Confirm the add-on at checkout; you can turn it off again.`;
  }
}

function renderCatalog(catalog) {
  currentCatalog = catalog;
  const list = byId("plans-list");
  if (!list) return;
  list.replaceChildren(...catalog.plans.map((p) => planCard(p, currentCurrency)));
  updateBackups(catalog.backups);
}

function setPricingSource(kind, message) {
  const pricing = byId("pricing");
  const source = byId("plans-source");
  const status = byId("plans-status");
  if (pricing) pricing.dataset.source = kind;
  if (status) status.textContent = message;

  if (!source) return;
  source.replaceChildren(document.createTextNode(message));

  if (kind === "fallback") {
    const retry = document.createElement("button");
    retry.type = "button";
    retry.className = "link-button";
    retry.textContent = "Retry";
    retry.addEventListener("click", () => {
      void loadPlans();
    });
    source.append(document.createTextNode(" "), retry);
  }
  source.hidden = false;
}

async function fetchCatalog() {
  const controller = new AbortController();
  const timer = window.setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);
  try {
    const response = await fetch(PLANS_URL, {
      method: "GET",
      headers: { Accept: "application/json" },
      credentials: "omit",
      cache: "no-store",
      signal: controller.signal,
      mode: "cors",
    });
    if (!response.ok) throw new Error(`plans request failed with ${response.status}`);
    return parseCatalog(await response.json());
  } finally {
    window.clearTimeout(timer);
  }
}

async function loadPlans() {
  if (!byId("plans-list")) return;
  try {
    const catalog = await fetchCatalog();
    renderCatalog(catalog);
    setPricingSource("live", "Live EUR & USD prices from api.stackspirit.io.");
  } catch {
    renderCatalog(FALLBACK_CATALOG);
    setPricingSource(
      "fallback",
      "Live catalog unavailable. Showing standard published prices.",
    );
  }
}

function initCurrencyToggle() {
  const buttons = Array.from(document.querySelectorAll("[data-currency]"));
  if (buttons.length === 0) return;

  function apply(curr) {
    currentCurrency = curr === "usd" ? "usd" : "eur";
    for (const btn of buttons) {
      const active = btn.getAttribute("data-currency") === currentCurrency;
      btn.setAttribute("aria-pressed", active ? "true" : "false");
      btn.classList.toggle("is-active", active);
    }
    renderCatalog(currentCatalog);
  }

  for (const btn of buttons) {
    btn.addEventListener("click", () => {
      apply(btn.getAttribute("data-currency") ?? "eur");
    });
  }
}

function initBackups() {
  const input = byId("backups");
  const status = byId("backups-status");
  if (!(input instanceof HTMLInputElement) || !status) return;

  input.checked = false;
  status.textContent = "";

  input.addEventListener("change", () => {
    if (input.checked) {
      const amt = currentCurrency === "usd" ? currentCatalog.backups.monthly_usd : currentCatalog.backups.monthly_eur;
      const price = amt === null || amt === undefined ? "priced separately at checkout" : formatMonthly(amt, currentCurrency);
      status.textContent = `Backups selected: ${price}. Confirm the add-on at checkout; you can turn it off again.`;
    } else {
      status.textContent = "Backups off. Your Home still stores local snapshots on disk.";
    }
  });
}

initYear();
initTabs();
initCopyButtons();
initCurrencyToggle();
initBackups();
initCraft();
void loadPlans();
