import { describe, expect, it } from "vitest";

import {
  laneForState,
  countsFromProjects,
  selectFleet,
  workerRingModel,
  jobActionEligibility,
  jobStateChip,
} from "../domain/fleet";
import {
  groupWaiting,
  ramWithEstimate,
  selectEstimate,
  targetAdvice,
  waitingAgeSeconds,
} from "../domain/capacity";
import {
  eventAgeLabel,
  formatAgeParts,
  formatExactBytes,
  formatGiB,
  truncateText,
} from "../domain/format";
import { executorBadges, executorLabel, parseExecutor } from "../domain/executor";
import { allWarnings, prettyParsed, redactParsed } from "../domain/raw";
import { FAKE_SECRET_TOKENS, GIB, rawWithWarnings, scenario } from "./scenarios/fixtures";

describe("fleet domain", () => {
  it("maps every documented state to the exact lane", () => {
    expect(laneForState("running")).toBe("running");
    expect(laneForState("stalled")).toBe("running");
    expect(laneForState("merging")).toBe("running");
    expect(laneForState("pending")).toBe("ready");
    expect(laneForState("ready")).toBe("ready");
    expect(laneForState("gating")).toBe("review_fix");
    expect(laneForState("reviewing")).toBe("review_fix");
    expect(laneForState("fixing")).toBe("review_fix");
    expect(laneForState("parked")).toBe("parked_blocked");
    expect(laneForState("failed")).toBe("parked_blocked");
    expect(laneForState("cancelled")).toBe("parked_blocked");
    expect(laneForState("done")).toBe("done");
    expect(laneForState("replanned")).toBe("done");
    expect(laneForState("quarantined")).toBe("parked_blocked");
  });

  it("derives lane counts from jobs_by_state, not from visible cards", () => {
    const fixture = scenario("fresh-single");
    const counts = countsFromProjects(fixture.fleet.projects);
    expect(counts).toMatchObject({
      running: 3,
      ready: 3,
      review_fix: 1,
      parked_blocked: 1,
      done: 37,
    });
    expect(fixture.fleet.lanes.ready).toHaveLength(3);
    expect(fixture.fleet.lanes.done).toHaveLength(0);
  });

  it("filters lanes and worker cards while leaving capacity node-wide", () => {
    const fixture = scenario("multi-project");
    const selection = selectFleet(fixture.fleet, "northnoir");
    expect(selection.projects.map((project) => project.name)).toEqual(["northnoir"]);
    expect(selection.running.map((card) => card.project)).toEqual(["northnoir"]);
    expect(selection.lanes.ready.every((card) => card.project === "northnoir")).toBe(true);
    expect(selection.counts.running).toBe(1);
    expect(selection.counts.ready).toBe(1);
    expect(fixture.fleet.capacity.running.runs).toBe(3);
  });

  it("renders three busy of eight and a plain-language summary", () => {
    const fixture = scenario("fresh-single");
    const ring = workerRingModel(fixture.fleet);
    expect(ring.busy).toBe(3);
    expect(ring.idle).toBe(5);
    expect(ring.overTarget).toBe(0);
    expect(ring.summary).toMatch(/8 worker slots, 3 busy, 5 idle/);
    expect(ring.summary).toMatch(/target advised by VOWP is 8/);
  });

  it("aggregates slots above twelve and never shrinks below twelve slots", () => {
    const fixture = scenario("fresh-single");
    fixture.fleet.capacity.target.target_workers = 16;
    fixture.fleet.capacity.advised_workers = 16;
    const ring = workerRingModel(fixture.fleet);
    expect(ring.slots).toHaveLength(12);
    expect(ring.aggregateOverflow).toBe(4);
    expect(ring.summary).toMatch(/16 worker slots/);
  });

  it("explains target zero instead of drawing busy slots", () => {
    const fixture = scenario("idle");
    fixture.fleet.capacity.target.target_workers = 0;
    fixture.fleet.capacity.advised_workers = 0;
    const ring = workerRingModel(fixture.fleet);
    expect(ring.targetZero).toBe(true);
    expect(ring.slots).toHaveLength(0);
    expect(ring.summary).toContain("No workers allowed — VOWP target is 0");
  });

  it("shows busy greater than target as an inconsistency", () => {
    const fixture = scenario("busy");
    fixture.fleet.capacity.target.target_workers = 6;
    fixture.fleet.capacity.advised_workers = 6;
    const ring = workerRingModel(fixture.fleet);
    expect(ring.busy).toBe(8);
    expect(ring.overTarget).toBe(2);
    expect(ring.summary).toMatch(/2 above target/);
  });

  it("keeps unknown states visibly unknown", () => {
    expect(jobStateChip("quarantined")).toEqual({ label: "Unknown", tone: "unknown" });
  });

  it("disables job actions for ineligible states and stale data", () => {
    const fixture = scenario("fresh-single");
    const done = fixture.fleet.lanes.review_fix[0];
    expect(done).toBeDefined();
    if (!done) return;
    const ineligible = jobActionEligibility({ ...done, state: "done" }, true);
    expect(ineligible.retry.enabled).toBe(false);
    expect(ineligible.park.enabled).toBe(false);
    expect(ineligible.cancel.enabled).toBe(false);
    expect(ineligible.cancel.reason).toContain("done");

    const active = jobActionEligibility({ ...done, state: "active" }, true);
    expect(active.park.enabled).toBe(true);
    expect(active.cancel.enabled).toBe(true);
    expect(active.retry.enabled).toBe(false);

    const stale = jobActionEligibility({ ...done, state: "active" }, false);
    expect(stale.cancel.enabled).toBe(false);
    expect(stale.cancel.reason).toContain("Refresh");
  });
});

describe("capacity domain", () => {
  it("selects an exact estimate before pooled, default, and largest measured", () => {
    const estimates = [
      {
        project: "vowp",
        job_kind: "build",
        harness: "opencode",
        model: "deepseek/deepseek-flash",
        build_mode: null,
        samples: 3,
        ram_p50_bytes: 1 * GIB,
        ram_p90_bytes: 1.3 * GIB,
        cpu_p50_cores: 1,
        cpu_p90_cores: 2,
        duration_p50_ms: 1,
        duration_p90_ms: 2,
        source: "measured",
      },
      {
        project: null,
        job_kind: "build",
        harness: "opencode",
        model: null,
        build_mode: null,
        samples: 12,
        ram_p50_bytes: 1 * GIB,
        ram_p90_bytes: 1.2 * GIB,
        cpu_p50_cores: 1,
        cpu_p90_cores: 2,
        duration_p50_ms: 1,
        duration_p90_ms: 2,
        source: "pooled",
      },
      {
        project: null,
        job_kind: "build",
        harness: "opencode",
        model: null,
        build_mode: null,
        samples: 0,
        ram_p50_bytes: 2 * GIB,
        ram_p90_bytes: 2 * GIB,
        cpu_p50_cores: 1,
        cpu_p90_cores: 2,
        duration_p50_ms: 1,
        duration_p90_ms: 2,
        source: "default",
      },
    ];
    expect(
      selectEstimate(estimates, {
        project: "vowp",
        jobKind: "build",
        harness: "opencode",
        model: "deepseek/deepseek-flash",
        buildMode: null,
      })?.source,
    ).toBe("measured");
    expect(
      selectEstimate(estimates, {
        project: "other",
        jobKind: "build",
        harness: "opencode",
        model: "other",
        buildMode: null,
      })?.source,
    ).toBe("pooled");
    // With no pooled or default match, the largest measured row is the documented
    // last resort; the UI labels it with its real source.
    expect(
      selectEstimate(estimates, {
        project: "other",
        jobKind: "build",
        harness: "claude-code",
        model: "model-a",
        buildMode: null,
      })?.source,
    ).toBe("measured");
  });

  it("adds the estimate source, samples and safe slot estimate", () => {
    const fixture = scenario("fresh-single");
    const ram = fixture.fleet.capacity.ram;
    const estimate = fixture.fleet.capacity.estimates[0];
    expect(estimate).toBeDefined();
    if (!estimate) return;
    const withEstimate = ramWithEstimate(ram, [estimate], {
      project: "vowp",
      jobKind: "build",
      harness: "opencode",
      model: null,
      buildMode: null,
    });
    expect(withEstimate.per_worker_source).toBe("pooled");
    expect(withEstimate.per_worker_samples).toBe(12);
    expect(withEstimate.estimated_extra_slots).toBe(
      Math.floor((ram.available_bytes - ram.reserve_bytes) / estimate.ram_p90_bytes),
    );
  });

  it("does not invent slots when no estimate exists", () => {
    const fixture = scenario("fresh-single");
    const ram = ramWithEstimate(fixture.fleet.capacity.ram, [], {});
    expect(ram.per_worker_p90_bytes).toBeNull();
    expect(ram.estimated_extra_slots).toBeNull();
  });

  it("groups waiting reasons by code and keeps unknown codes", () => {
    const fixture = scenario("provider-limit");
    const groups = groupWaiting([
      ...fixture.fleet.capacity.waiting,
      { project: "vowp", job: "x", code: "quota?", detail: "x", since: fixture.fleet.generated_at },
    ]);
    expect(groups[0]?.code).toBe("provider");
    expect(groups[0]?.label).toBe("Provider limit");
    expect(groups.at(-1)?.code).toBe("quota?");
    expect(waitingAgeSeconds("2026-11-14T11:59:00.000Z", "2026-11-14T12:00:00.000Z")).toBe(60);
  });

  it("describes advice precedence and override labels", () => {
    expect(targetAdvice(8, 8).relation).toBe("equal");
    expect(targetAdvice(4, 8).relation).toBe("below");
    expect(targetAdvice(10, 8).relation).toBe("above");
    expect(targetAdvice(10, 8, true).label).toContain("owner override");
    expect(targetAdvice(4, null).canApply).toBe(false);
  });
});

describe("format and executor domain", () => {
  it("formats ages, bytes and evidence text without inventing values", () => {
    expect(formatAgeParts(134)).toBe("2m 14s");
    expect(eventAgeLabel(3)).toBe("live");
    expect(eventAgeLabel(241)).toBe("quiet 4m 1s");
    expect(eventAgeLabel(4, true)).toBe("stalled");
    expect(formatGiB(42.1 * GIB)).toBe("42.1 GiB");
    expect(formatExactBytes(5 * GIB)).toContain("bytes");
    expect(truncateText("x".repeat(201), 200).endsWith("…")).toBe(true);
  });

  it("splits harness, provider, model and variant", () => {
    const executor = parseExecutor("opencode:deepseek/deepseek-flash#max");
    expect(executor).toMatchObject({
      harness: "opencode",
      provider: "deepseek",
      model: "deepseek/deepseek-flash",
      variant: "max",
      effort: null,
    });
    expect(executorLabel(executor)).toContain("DeepSeek V4.1 Flash");
    expect(executorBadges(executor)).toMatchObject({
      harness: "opencode",
      provider: "deepseek",
      model: "DeepSeek V4.1 Flash",
      modifier: "max",
    });
    const claude = parseExecutor("claude-code:model-a@medium");
    expect(claude).toMatchObject({ provider: "claude", effort: "medium", variant: null });
  });

  it("keeps malformed and unknown executors raw", () => {
    for (const raw of [
      "deepseek/deepseek-flash",
      "future-harness:model",
      "opencode:model@fast",
      "claude-code:model#max",
      "opencode:model#max#extra",
    ]) {
      const executor = parseExecutor(raw);
      expect(executor.harness).toBe("unknown");
      expect(executor.display).toBe(raw);
    }
  });

  it("keeps a null executor as a shell card value", () => {
    expect(executorBadges(null)).toMatchObject({ harness: "shell", model: null });
  });
});

describe("raw document domain", () => {
  it("keeps every parse warning", () => {
    const raw = rawWithWarnings();
    expect(allWarnings(raw).map((warning) => warning.code)).toEqual([
      "unknown_enum",
      "missing_optional",
      "truncated",
    ]);
  });

  it("redacts secret-shaped keys and values", () => {
    const raw = rawWithWarnings();
    const rendered = prettyParsed(raw.status);
    expect(rendered).not.toContain(FAKE_SECRET_TOKENS.sk);
    expect(rendered).not.toContain("Bearer");
    expect(prettyParsed(raw.capacity)).not.toContain(FAKE_SECRET_TOKENS.ghp);
    expect(redactParsed({ api_key: "anything", ok: "visible" })).toEqual({
      api_key: "[redacted]",
      ok: "visible",
    });
  });
});
