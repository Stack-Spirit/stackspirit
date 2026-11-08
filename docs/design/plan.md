# VOWP — Virtual Open Work-to-Production

**Plan v1.1 · 2026-11-08 · orchestrator: Claude (Mac) · status: approved by the owner — Rust core, SQLite state, overnight build starting**

Owner decisions (2026-11-08): start tonight; **Rust** for the core (TypeScript only later for the web window); a **SQLite database on every node** for nodes, sessions, jobs, workers, loops and schedules, with the **VPS as the always-on backbone**; private repo `syver777/vowp` (created); MIT; nothing public without the owner's yes.

VOWP is an open-source framework for running AI coding agents on your own machines — a laptop, a VPS, or both — as a team. Each agent can be Claude Code, opencode or another harness. Work is planned as a **graph** of jobs. Deterministic **loops** keep it moving all night. Each job runs in a **tmux window** you can watch from your Mac or phone. Every conversation is a **universal session** that can move between harnesses, models and machines without losing its context. Finished work moves from **Work to Production** only through guarded, reversible promotions.

This document is the full design for v0.1 and the plan to build it overnight with a planner (Claude Code), a loop keeper (DeepSeek) and DeepSeek workers on the VPS.

---

## 0. Outcome by morning

What you should find when you wake up:

1. **A private GitHub repo `syver777/vowp`** with a working **v0.1.0-rc**: `vowp` CLI, supervisor loops, job graph, tmux windows, Claude Code and opencode adapters, universal sessions with capsule switching, framework templates, install script, CI green.
2. **Documentation good enough for a stranger**: README quickstart (local, VPS, mixed), concepts, graph and loops, sessions and switching, providers and failover, Work→Production, security, troubleshooting, and one runnable example project. The quickstart is tested by a "fresh user" job that follows it literally in a clean environment.
3. **A night report** (`NIGHT-REPORT.md`) with what was built, what failed and why, real costs from receipts, and how many times Claude hit its limit and what took over.
4. **A short decision inbox** for you: make the repo public, publish `vowp` on npm, deploy the website on Vercel. Nothing goes public without your yes.

The name is still free everywhere (checked 2026-11-08: GitHub `syver777/vowp`, npm, PyPI and crates.io all return not found).

---

## 1. Inputs this plan is built on

| Source | What it gave |
|---|---|
| Orchestrator session of 2026-11-06 (recovered from this session's history and `work/framework-vowp.md` on the VM; the original opencode session has since been deleted) | The name VOWP. What it is: VPS + agent sessions in tmux + one orchestrator reachable from Mac, Termius or phone; Work and Production projects; any model. A GitHub repo plus a simple Vercel site. "Extract the framework from the real process, don't invent it." The job table and brief/report front-matter idea. |
| `portable-session-harness-v1-plan.md` (your DeepSeek harness notes) | Universal sessions: a canonical event log, adapters per harness, capsules (structured handoff), the switch protocol, nodes (laptop/VPS), the window idea, and legal guardrails. It also lists your own goals: start from stock harnesses, sessions usable from any harness, use your Claude subscription "through a window" and switch to another provider when its limit ends, and let people run heavy multi-agent work on their own VPS. |
| The v0 we ran on 2026-11-06/07 | One brief and one report per job, strict file ownership, gates that check reports and the remote, guarded deploys, a watcher, and one orchestrator that verifies against live artifacts. |
| What went wrong in practice (these are requirements) | OpenRouter credits ran out and every worker died. A safety classifier refused deploys, and the correct response was to stage them. A deploy script was never written, yet a gate that only checked "file exists" would have passed. Live production code was not in git. Two pushed files had syntax errors that the worker's own checks missed. `.vercelignore` broke a prod build that the local build passed. `tmux respawn-pane` without a command silently restarted six old workers. Workers announced their next step and then idled for an hour. Answering opencode permission dialogs needed tmux keystroke hacks. |

I found no stored plan for "loops and graphs" in any saved session, so sections 7–8 design them from scratch on top of the recovered job table, gates and watcher ideas. If you remember specifics I've missed, tell me and I'll fold them in before the night starts.

---

## 2. Principles

1. **Stock harnesses, no forks.** VOWP drives Claude Code, opencode and others through their official headless/JSON interfaces. It never scrapes a TUI, never sends keystrokes to answer dialogs, and never reads, stores or forwards anyone's login tokens.
2. **The database is the truth for decisions; files are the truth for content.** Each node has one SQLite database (`~/.vowp/vowp.db`, WAL mode, one writer: the supervisor) holding nodes, projects, jobs, edges, gates and evidence, runs, workers, sessions (index), loops, schedules, ledger and inbox. Content stays in plain files: append-only session logs (JSONL) and blobs, briefs and reports (Markdown, because agents read and write files naturally), and `vowp.toml`. Kill everything, restart, and it continues where it was.
3. **Deterministic loops, LLM judgment.** A plain program ticks every 60 seconds and never "decides to stop". Language models are called only for judgment: planning, diagnosing a stall, reviewing a report, writing a handoff. The program cannot idle the way an agent can.
4. **Any model, any machine.** A model is a setting in a profile ladder, not an assumption. The same job can run on Claude Code on a laptop or on DeepSeek on a VPS.
5. **Evidence over claims.** A job is done when its gates pass with recorded evidence (commands run, exit codes, diffs), not when the agent says so.
6. **Work → Production is guarded and reversible.** Promotions snapshot what is live, ship only the intended change, record the rollback and require a human yes by default.
7. **Cost is reported, not capped.** Every call's real cost comes from the harness's own receipt. Totals are reported at checkpoints. There are no invented budgets, but there are hard bounds on retries and no-progress loops.
8. **Humans are never a bottleneck for unrelated work.** A question for the owner parks one job and lands in an inbox; everything else keeps running.

---

## 3. Concepts

| Term | Meaning |
|---|---|
| **Node** | A machine running VOWP: your Mac (`local`), a VPS (`vps`), any server. |
| **Harness** | An agent program: Claude Code, opencode, Codex, Gemini CLI… |
| **Adapter** | VOWP code that drives one harness headlessly and turns its output into canonical events. |
| **Session** | A universal, harness-neutral conversation with a stable id and an append-only event log. It can have several native sessions underneath (one per harness run). |
| **Capsule** | A structured handoff (intent, decisions, state, files, gotchas, next steps) written when a session switches harness/model/node. |
| **Switch** | Moving a session to another executor (harness + model + node), recorded as lineage. |
| **Window** | Any view onto live work: a tmux window running a job, the board, a chat window into a session. Switching the harness behind a session does not close its window. |
| **Job** | A node in the graph: one unit of work with a brief, owned files, gates and a report. |
| **Graph** | Jobs plus edges (needs, verifies, fixes, escalates, blocks). It is a DAG except for bounded loop edges. |
| **Gate** | A machine-checkable condition with recorded evidence: report fields present, command exits 0, branch pushed, reviewer passed… |
| **Loop** | A repeating process with a tick, an input, actions and bounds (section 8). |
| **Supervisor** | The deterministic program that runs the loops. |
| **Roles** | **Planner** (owns the graph, writes briefs, reviews), **Keeper** (judgment when things stall or fail; takes over planning when the planner is unavailable), **Worker** (does a job), **Verifier** (checks a job independently). |
| **Profile** | A role's model ladder, e.g. planner = Claude Sonnet 5.5 medium → DeepSeek V4.1 Flash 1017 max. |
| **Slot** | A concurrent worker place (also a tmux window). |
| **Inbox** | Questions and approvals waiting for a human. |
| **Ledger** | Append-only record of dispatches, gate results, merges, costs and switches. |
| **Environment** | Where code runs: `work` (staging) and `prod`, each with its own checkout, deploy commands and guards. |
| **Promotion** | Moving a verified change from Work to Production through guards, with a serving snapshot and rollback. |

---

## 4. Architecture

```
            You:  Mac terminal · Claude Code on the Mac · Termius on the phone · opencode app
                         │  (ssh / tunnel)                │
          ┌──────────────┴───────────────┐   ┌────────────┴──────────────┐
          │ NODE: local (Mac)            │   │ NODE: vps                  │
          │  vowp CLI ─────────────────────► │  vowp CLI (same binary)    │
          │  sessions store (~/.vowp)    │   │  sessions store (~/.vowp)  │
          │  adapters: claude-code, …    │   │  supervisor (systemd user) │
          └──────────────────────────────┘   │   ├ L1 tick 60s            │
                                             │   ├ graph + scheduler      │
                                             │   ├ gates + merge queue    │
                                             │   ├ failover/switcher      │
                                             │   ├ ledger, inbox, reports │
                                             │   └ tmux window manager    │
                                             │  tmux session vowp-<proj>  │
                                             │   board│planner│keeper│job…│
                                             │  adapters (headless JSON): │
                                             │   claude -p stream-json    │
                                             │   opencode run --format json│
                                             │  git worktrees per job     │
                                             └────────────────────────────┘
```

Rules:

- The supervisor and the CLI talk to harnesses only through adapters. Adapters only use documented headless modes.
- A project's graph and supervisor live on one node (its "home"). Other nodes see it over SSH (`vowp --node vps …`). There is no cross-node sync in v0.1: a session moves between nodes as an exported bundle (capsule plus recent tail).
- The tmux windows show live runs. They are views, never control channels: VOWP never sends keystrokes to a harness TUI.
- An HTTP+SSE daemon API and a web/PWA window are v0.2. They are designed for now (stable event and job schemas), but not built tonight.

---

## 5. Universal sessions

Adapted from your session-harness plan, with v0.1 simplified to what both harnesses officially support today.

### 5.1 Event log

`~/.vowp/sessions/<sessionId>/log.jsonl` is append-only. Every line is an envelope:

```ts
interface EventEnvelope<T = unknown> {
  id: string;            // ULID
  sessionId: string;     // canonical ULID
  seq: number;           // 1,2,3… gaps are rejected
  ts: string;            // ISO-8601 ms
  type: EventType;       // see below
  actor: "user" | "assistant" | "system" | "tool";
  source: { harness: "claude-code" | "opencode" | "cli" | "vowp"; model?: string; node?: string; nativeSessionId?: string };
  payload: T;
  raw?: unknown;         // original harness event (display only, never replayed)
  replayable?: boolean;
}
```

Event types: `session.created`, `session.executor` (attach/detach), `turn.started`/`turn.ended`, `user.message`, `assistant.message`, `assistant.reasoning` (display only), `tool.call`, `tool.result` (large output spilled to `blobs/<sha256>`), `file.change`, `plan`, `limit` (`quota|credits|rate|context|auth`, `resetsAt?`), `cost` (`usd`, tokens, `source: receipt|price-table|unknown`), `switch`, `capsule`, `error`, `note`.

Same storage invariants as the session-harness plan: the JSONL is authoritative for session content and fsynced at turn ends, the session rows and event index in `vowp.db` are rebuildable from it (`vowp reindex`), and a session directory copied to another node opens there.

### 5.2 Adapters in v0.1

| Adapter | How it drives the harness | Resume | Limits detected |
|---|---|---|---|
| **claude-code** | `claude -p --output-format stream-json --verbose [--resume <id>] --model … --effort … --permission-mode …` (official headless mode; optionally `--input-format stream-json` for multi-turn) | `--resume <native id>` | usage/session/weekly limit text and the reset time, 429/overloaded, auth errors; unknown repeated errors are treated as "unavailable for 15 min" |
| **opencode** | `opencode run --format json --session <id> --model provider/model#variant --auto` against the background service (sessions also show in the opencode app); attaching to `opencode serve` via its HTTP API is an option | `--session <id>` | provider 402/insufficient credit, 429, auth, context overflow |
| **cli** (generic) | Any command with JSON or plain output (e.g. `codex exec --json`, `gemini -p --output-format json`) plus a mapping file | per tool | exit codes and patterns from the mapping file |
| **acp** (v0.2) | Agent Client Protocol (`claude-code-acp` and others) for richer events and permission flow | yes | yes |

The adapter contract is the one from your plan (`probe()`, `capabilities()`, `start(input)` → `send/events/interrupt/stop`), so adding a harness is one file plus one capability entry.

### 5.3 Capsule and switch

The capsule schema is unchanged from your plan (intent, constraints, decisions, state done/in-progress/blocked/next, files, gotchas, open questions, telemetry). It is written by the cheapest capable model on the node (default DeepSeek V4.1 Flash), validated against the schema, and repaired once. The previous capsule is kept on failure.

The switch protocol is also unchanged: freeze → drain ≤ 5 s → capture capsule → stop → start the target seeded with capsule plus the last 2 turns as plain text → record `switch` → unfreeze. Switches are triggered by the user, by a `limit` event per the profile policy, by an adapter crash (if configured), or by context pressure.

Cross-node in v0.1: `vowp session export <id>` writes a bundle (capsule + tail + lineage), and `vowp session import` on the other node creates a child session.

### 5.4 Your daily case, concretely

- On the Mac you work in Claude Code as you do now. `vowp session watch` records that session into the canonical store via the claude-code adapter's transcript importer.
- When Claude's limit hits, `vowp session switch <id> --to opencode:deepseek/deepseek-flash#max --node vps` hands off to DeepSeek on the VPS with a capsule. You keep talking through `vowp chat <id>` in any terminal, including Termius.
- When Claude resets, switch back the same way. The lineage shows both halves as one conversation.

---

## 6. Windows

### 6.1 tmux window manager (v0.1)

- **One dedicated tmux session per project:** `vowp-<project>`. VOWP never touches other tmux sessions (your `main0` and the CLI film worker running there are invisible to it).
- **Layout:** window 0 `board` (live overview), 1 `supervisor` (loop log), 2 `planner`, 3 `keeper`, then one window per running job (`job-<id>`), and `inbox`.
- **Each job window shows a pretty-printed view of the job's event log.** The harness runs headless and its JSON goes to the log, so the window is readable and the log is complete.
- **Safety rules learned the hard way:**
  - Never `respawn-pane` without an explicit command. Recycling a window means kill it and create a new one.
  - Tag every VOWP window (`@vowp=1`), and only ever kill tagged windows.
  - `capture-pane` is for display only. Logs drive decisions, never screen text.
  - No keystrokes are ever sent to a harness to answer prompts. Permissions are set by harness mode and policy.
- **Access:** from the Mac, `ssh -t vps tmux attach -t vowp-<project>`, or `vowp attach --node vps`. Termius works the same way. Job sessions also appear in the opencode app.

### 6.2 Terminal windows built into the CLI (v0.1)

- `vowp board` is a refreshing dashboard: jobs by state, slots, current planner/keeper executor, limits and reset times, costs, inbox count, last checkpoint.
- `vowp chat <session>` is a window into any session on any harness. Type to send, stream replies, and use `/switch <harness:model>`, `/capsule` and `/export`.
- `vowp graph` prints the graph as ASCII or Mermaid.

### 6.3 Web window (v0.2)

The PWA from your plan (nodes, sessions, session view with an executor switcher, capsule view, board) runs on top of the daemon API. Event and job schemas are frozen in v0.1 so v0.2 only adds transport and UI.

---

## 7. The graph

### 7.1 Job node

Stored in the node database (`jobs` + `edges` tables) and edited only through `vowp job add|update` (validated), never by hand. `vowp graph export` writes it as YAML so a project can keep its graph in git.

```yaml
id: n04-graph-engine
title: "Graph engine: DAG, loop edges, ownership locks, scheduler"
kind: build            # plan | spec | build | test | verify | fix | merge | docs | research | promote | release | decision
role: worker           # planner | keeper | worker | verifier
profile: worker        # model ladder (section 11)
needs: [n02-protocol]  # must be done first
owns: ["packages/graph/**"]          # the only paths this job may change (lock)
reads: ["packages/protocol/**", "docs/design/**"]
brief: .vowp/jobs/n04-graph-engine/brief.md
report: .vowp/jobs/n04-graph-engine/report.md
gates:
  - report: { fields: [status, branch, commits, tests] }
  - cmd: { run: "pnpm -C packages/graph test", expect_exit: 0 }
  - cmd: { run: "pnpm -r typecheck", expect_exit: 0 }
  - owns_respected: true            # diff touches only `owns`
  - review: { by: verifier }        # spawns a verify job
loop: { until: gates, max_attempts: 3, on_exhausted: escalate }
priority: 50
state: pending
```

### 7.2 Edges

| Edge | Meaning |
|---|---|
| `needs` | A depends on B being done. |
| `verifies` | An automatic verify job for a build job. |
| `fixes` | A fix job created by a failed verify. This is a **loop edge**, bounded by `max_attempts`. |
| `escalates` | Exhausted attempts or a human question → a `decision` job in the inbox. |
| `blocks` | A decision job blocks the jobs waiting on it. |

### 7.3 States

```
pending ──needs done──► ready ──dispatched──► running ──report+cmd gates──► reviewing ──verify pass──► merging ──► done
                                             │   ▲                            │
                                    no progress  │ continue/nudge             └─verify fail──► fixing (loop edge, ≤3) ─► reviewing
                                             ▼   │
                                          stalled ┘──attempts exhausted──► failed ──keeper──► replanned | parked
any state ──needs a human / provider blocked──► parked ──resolved──► ready
```

### 7.4 Scheduler rules

1. Only `ready` jobs are dispatched, by priority, then age.
2. **Ownership locks:** no two running jobs may have overlapping `owns` globs. Conflicts wait. Two jobs never need the same file at once, and the planner splits work so that holds.
3. **Slots and provider concurrency:** max workers per node, and max concurrent sessions per provider (lowered automatically after 429s).
4. **One git worktree per job** on branch `job/<id>`; the worker's working directory is that worktree.
5. **Single writer to `main`:** only the supervisor's merge queue writes `main`. It rebases the job branch, runs the repo-wide test command, fast-forwards and pushes. A conflict becomes a `merge` job for the same owner.

---

## 8. The loops

| Loop | Tick | What it does | Bounds |
|---|---|---|---|
| **L1 Supervisor** | 60 s, deterministic | Reconciles every job: process alive? new events? report present? limits? Runs gates, moves states, feeds the merge queue, dispatches ready jobs, triggers L4/L5, writes the board. | Never stops. The watchdog restarts it if it dies. |
| **L2 Worker** | per job | Starts the job's session from its brief. When the run ends without a report, sends "you stopped before writing your report — continue" into the same session. Then gates. | 2 automatic continues, then the keeper diagnoses. |
| **L3 Verify** | after a report | A separate verifier session reads the brief, the diff and the gate evidence, and passes or lists concrete failures. Failure creates a `fix` job with the findings. | 3 fix rounds, then escalate. |
| **L4 Planner** | on events + 30 min heartbeat | Gets a digest (finished, failed, stalled, inbox) and acts with `vowp job add/update` and brief files: decomposes, reprioritises, reviews reports against evidence, writes checkpoint summaries. | One planner turn at a time. |
| **L5 Keeper** | on stalls/failures + 20 min heartbeat | Diagnoses stalls (reads logs), decides nudge / restart / replan / park, writes nudges. **Takes over the planner role when the planner's harness is unavailable.** | One keeper turn at a time. |
| **L6 Failover** | on `limit` events | Claude limit → capsule → planner continues on the next rung (DeepSeek). Probes Claude after the reset time (one tiny call, then every 15 min) and switches back with a capsule. | Never silently moves work to a *paid* provider the profile doesn't list. Credits exhausted → park + inbox. |
| **L7 Human** | continuous | Questions and approvals go to `inbox/` (and optionally a phone notification). Only the asking job waits. | — |
| **L8 Watchdog** | systemd user service (`Restart=always`, linger on) + 5-min cron check | Keeps L1 alive across crashes and reboots. | — |

| **L9 Schedules** | every tick | Runs due rows of the `schedules` table (cron expressions or intervals): create a job, start a loop, or run a command. Examples: "every 30 min planner heartbeat", "07:00 weekdays morning report", "01:00 dependency-update job", "hourly provider-credit check", "nightly database backup". | Missed runs while the node was down run once, not N times. |

Loops are rows too (`loops` table: kind, interval, bounds, last/next run, state), so `vowp loops` lists them and you can pause or resume any loop. System cron and systemd only keep the supervisor itself alive.

**No-progress detection:** a job is "stalled" after 20 min with no new events and no new commits. A loop that repeats the same action 3 times without new evidence is stopped and escalated.

### 8.1 Database schema (v0.1, one SQLite file per node)

| Table | Holds |
|---|---|
| `nodes` | This node and known remote nodes (name, ssh target, last seen, capabilities) |
| `projects` | Projects homed on this node (path, repo, default profiles, environments) |
| `jobs`, `edges` | The graph: job fields from 7.1, edges with kind |
| `runs` | Every attempt: job, role, harness, model, native session id, pid/window, start/end, exit, outcome |
| `gate_results` | Gate, pass/fail, evidence (command, exit, output digest/blob ref), time |
| `workers` | Slots and their current run |
| `sessions`, `session_events` | Universal sessions and an index of their logs (the JSONL stays authoritative) |
| `capsules`, `switches` | Handoffs and lineage |
| `loops`, `schedules` | Loop definitions/state; cron and interval schedules with last/next run |
| `ledger` | Dispatches, merges, promotions and costs (usd, tokens, source: receipt/price-table/unknown) |
| `inbox` | Questions and approvals for humans, with answers |
| `limits` | Per provider/harness: kind, until, last probe |
| `releases` | Promotions with serving snapshot and rollback command |

Postgres or replicated SQLite (libSQL) is a v0.3 option for teams and multi-node sync; v0.1 stays embedded and zero-ops.

---

## 9. Gates (as data)

| Gate | Passes when | Evidence recorded |
|---|---|---|
| `report` | The report exists and its front-matter has the required fields with valid values | Parsed front-matter |
| `cmd` | The command exits with the expected code (optional output pattern) | Command, exit code, last 200 lines, duration |
| `owns_respected` | `git diff main...job/<id>` touches only `owns` | File list |
| `pushed` | The branch/commit exists on the remote | `git ls-remote` output |
| `review` | The verifier passed | Verifier report |
| `script` | A custom checker exits 0 | Same as `cmd` |
| `deploy` | The deploy command ran **and** the post-deploy probe matched | Versions before/after, probe output, rollback command |

**The anti-pattern is banned:** a gate may never be "file exists" for anything that must *work* (deploy scripts, tests, builds). It must execute and check results.

**Report front-matter:**

```yaml
---
job: n04-graph-engine
status: done            # done | partial | blocked
branch: job/n04-graph-engine
commits: [abc1234, def5678]
tests: { command: "pnpm -C packages/graph test", passed: 42, failed: 0 }
files_changed: [packages/graph/src/scheduler.ts, …]
open_items: []
questions_for_owner: []
---
```

**Brief front-matter** (the job's contract, generated from the graph node plus the planner's text):

```yaml
---
job: n04-graph-engine
owns: ["packages/graph/**"]
must_not_touch: ["everything else"]
needs_done: [n02-protocol]
gates: [report, "pnpm -C packages/graph test", "pnpm -r typecheck", owns_respected, review]
done_when: "report has status: done and every gate passes"
---
```

---

## 10. Work → Production

Generalised from today's guarded deploys:

```toml
[env.work]
checkout = "~/myapp"
deploy = ["supabase functions deploy {fn} --project-ref WORK_REF"]

[env.prod]
checkout = "~/myapp-prod"           # fast-forward-only clone of main
deploy = ["vercel deploy --prod"]
guards = ["clean_tree", "head_equals_remote", "build", "no_active_jobs:./scripts/guard.sh"]
approval = "human"                 # human | policy
serving_bank = "./scripts/bank-serving.sh {target}"   # snapshot what is live before deploying
```

`vowp promote <change> --to prod` runs: guards → **serving bank** (snapshot live) → diff candidate vs serving (must equal the intended change) → approval → deploy → probe → record version, rollback command and release-log line. `vowp rollback <release>` redeploys the banked serving copy.

This captures the biggest lessons of the last week: **live is not always git**, **a local build is not the deploy build** (upload-set rules like `.vercelignore` are checked), and **a refused deploy is staged, never worked around**.

---

## 11. Providers, models and failover

`vowp.toml` profiles. These are tonight's settings:

```toml
[profiles.planner]
ladder   = ["claude-code:claude-sonnet-5-5@medium", "opencode:deepseek/deepseek-flash#max"]
on_quota = "next_then_probe_back"

[profiles.keeper]
ladder = ["opencode:deepseek/deepseek-flash#max"]

[profiles.worker]
ladder = ["opencode:deepseek/deepseek-flash#max"]

[profiles.verifier]
ladder = ["opencode:deepseek/deepseek-flash#max"]

[providers.deepseek]
max_concurrent = 6
on_credits_exhausted = "park"     # report the blocker, never switch to another paid provider silently
```

`deepseek/deepseek-flash` is **DeepSeek V4.1 Flash 1017** with DeepSeek as the provider (models.dev id `deepseek-flash`, 1M context, 393k output). The `#max` variant is accepted by opencode on the VPS: I tested it with one tiny call today and deleted the probe session.

| Signal | Example | Action |
|---|---|---|
| `quota` | Claude: "You've hit your limit · resets …" | Capsule → next rung; probe back after the reset |
| `credits` | 402 / insufficient balance | Park every job on that provider; inbox; stop |
| `rate` | 429 | Backoff with jitter; lower that provider's concurrency |
| `auth` | 401 / login expired | Park; inbox ("log in again") |
| `context` | Context overflow | Capsule → fresh session on the same harness |
| `crash` | Process exited without a report | L2 continue (×2) → keeper |

**Cost ledger:** opencode's `step_finish` events and Claude's stream-json `result` carry usage and cost. VOWP records `usd` with `source: receipt`, or a price-table estimate labelled `source: price-table`, or `unknown` (never assumed free). `vowp cost` shows totals by job, role, provider and model.

---

## 12. Framework files the agents follow

Shipped in `packages/framework/templates/` and installed by `vowp init`:

- **`AGENTS.md`** (harness-neutral). Covers working autonomously until the report exists, owned paths only, never committing secrets (names only), reporting costs, verifying against real artifacts, and staging any refused action rather than working around it.
- **`CLAUDE.md`** that just imports `AGENTS.md` (`@AGENTS.md`), so both harnesses read the same rules.
- **`brief.md` and `report.md` templates** with the front-matter above.
- **Skills** in `.agents/skills/`, loaded by both harnesses: `vowp-worker` (the job contract), `vowp-planner` (how to shape the graph, split ownership and write briefs), `vowp-verifier` (how to judge evidence).
- **`opencode.jsonc` agent definitions** for planner, keeper, worker and verifier, with permission presets so no dialog ever needs a keystroke.

---

## 13. Repo layout and stack

**Rust (stable), one static-ish binary `vowp`.** Crates: `rusqlite` (bundled SQLite), `serde`/`serde_json`, `clap`, `tokio` only where streams need it (prefer plain threads), `axum` for the v0.2 API, `cron` for schedules, `globset` for ownership, `ulid`, `time`. Distributed as prebuilt binaries for Linux (x86_64/arm64) and macOS (arm64/x86_64) on GitHub Releases, an `install.sh`, Homebrew and `cargo install vowp` later, and optionally an npm wrapper with prebuilt binaries (the esbuild/Codex pattern). **MIT** license. The v0.2 web window is TypeScript and is served by the daemon as static files.

```
vowp/
├─ crates/
│  ├─ vowp-protocol/  events, capsule, job, gate, profile, config types (serde) + JSON Schema export
│  ├─ vowp-store/     SQLite schema + migrations, JSONL logs, blobs, reindex
│  ├─ vowp-graph/     DAG + loop edges, state machine, ownership locks, scheduler (pure functions)
│  ├─ vowp-tmux/      safe window manager (private-socket tests)
│  ├─ vowp-adapters/  claude-code, opencode, cli (generic)
│  ├─ vowp-core/      supervisor loops, schedules, gates, merge queue, failover/switch, capsule writer,
│  │                  ledger, inbox, checkpoints, promote/rollback
│  └─ vowp-cli/       the `vowp` binary
├─ framework/        AGENTS.md, CLAUDE.md, brief/report templates, skills, opencode.jsonc (embedded in the binary)
├─ docs/             concepts, quickstarts, guides, reference, design
├─ examples/hello-vowp/   a tiny project with a 3-job graph you can run in 5 minutes
├─ site/             static website (built tonight, deployed only on your yes)
├─ scripts/          install.sh, systemd unit, cron watchdog
└─ .github/workflows/ci.yml   fmt, clippy, test on Linux + macOS; release binaries on tags
```

### CLI v0.1

```
vowp init                         # vowp.toml + framework files + .vowp/ in the current repo
vowp doctor                       # node, tmux, git, harnesses found, providers reachable
vowp up | down | status | board   # supervisor + tmux session
vowp plan "goal"                  # planner turns a goal into a graph (you review)
vowp job add|update|ls|show|retry|park
vowp graph [--mermaid]
vowp attach [job]                 # tmux attach / pretty log follow
vowp session ls|show|chat|switch|export|import|watch
vowp inbox ls|answer
vowp cost [--by job|role|model]
vowp node add|ls                  # ssh nodes; any command takes --node <name>
vowp promote|rollback|releases
vowp loops [pause|resume <loop>]
vowp schedule add "<cron>" <job-template|loop|command> | ls | pause | rm
vowp reindex | backup
```

---

## 14. Security and compliance

- The supervisor and any API bind to `127.0.0.1` only. Remote access is SSH (or a tunnel). A per-node token is used for the v0.2 API.
- VOWP stores **no credentials**. Harness logins stay in each harness (`~/.claude`, opencode's auth/env). Provider keys are environment variables the user already has. Logs and reports refer to secret **names**, never values. A secret-scan gate runs before every push.
- Stock binaries only: no client spoofing, no "Sign in with Claude" inside VOWP, no token forwarding. Using a Claude subscription means running the user's own, signed-in, official `claude` CLI on their own machine.
- `cwd` allow-list per node. Jobs run in their own worktrees.

---

## 15. Scope

**v0.1 (tonight):** everything in sections 5–13 except the HTTP API, the web window, ACP and multi-user.

- **Must-haves by morning:** protocol, store, graph, tmux, both adapters, supervisor loops L1–L8, failover, capsule switch (CLI), CLI core, framework templates, docs, install, example, CI.
- **Nice-to-haves:** promote/rollback, website, `vowp session watch` importer for interactive Claude sessions.

**v0.2:** daemon HTTP+SSE API, the web/PWA window, ACP adapter, a phone-friendly board, cross-node session sync, a cost dashboard.

**v0.3:** multi-user, quotas, VPS metrics, parallel fan-out policies, hosted relay.

---

## 16. The overnight build

### 16.1 Team

| Role | Harness / model | Where | Notes |
|---|---|---|---|
| **Orchestrator** | Claude Opus 5.5 (this Mac session) | Mac | Builds the bootstrap kit, launches, checks the first hour, hands over. Back in the morning (or whenever you talk to me). |
| **Planner** | Claude Code, **Sonnet 5.5, medium**, headless `claude -p --resume` | VPS | Owns the graph, writes briefs, reviews. Can stop at its subscription limit — designed for. |
| **Keeper** | opencode, **DeepSeek V4.1 Flash 1017, max** (`deepseek/deepseek-flash#max`) | VPS | Keeps the loops going with the deterministic supervisor. Diagnoses stalls. **Becomes the planner when Claude hits its limit** and hands back when it resets. |
| **Workers ×4** | opencode, DeepSeek V4.1 Flash 1017, max | VPS | One job each, in its own worktree and tmux window. |
| **Verifier** | opencode, DeepSeek V4.1 Flash 1017, max (separate sessions) | VPS | Independent checks of every build job. |

### 16.2 Bootstrap (me, about 45–60 minutes after your go)

VOWP can't run itself before it exists, so a small, plain **bootstrap supervisor** (`~/vowp-night/night.py`, Python stdlib, about 400 lines) implements L1–L8 with the same graph, brief, report and gate formats. Near the end of the night the `dogfood` job switches the night over to the real `vowp up`, so VOWP finishes building itself.

1. With your yes, create the **private** repo `syver777/vowp` from your Mac (`gh`). Add a **deploy key limited to that one repo** with write access, generated on the VPS. Workers can push there and nowhere else.
2. Clone it on the VPS (`~/vowp`) and set up `~/vowp-night/` (graph, jobs, ledger, inbox, runs, `NIGHT-REPORT.md`).
3. Write and test `night.py`:
   - Unit tests for the gates, the scheduler and the limit parser.
   - A dry run with fake adapters (echo scripts) through dispatch → report → gates → verify → merge.
   - A **failover drill**: a fake Claude limit message must move the planner to DeepSeek and back.
   - A crash drill: kill a worker and confirm L2 continues it.
4. Install the systemd user service `vowp-night` (`Restart=always`; linger is already on) plus a 5-min cron check.
5. Create tmux session `vowp-night` (board / supervisor / planner / keeper / job windows), separate from `main0`.
6. Seed the graph (16.3), copy this plan into the repo as `docs/design/plan.md`, and start. I watch the first jobs land, then hand over.

### 16.3 Night graph (seed — the planner refines it)

| Job | Kind / role | Needs | Owns | Done when |
|---|---|---|---|---|
| n00 design-spec | spec / **planner** | — | `docs/design/**` | Architecture, SQLite schema (`schema.sql`), crate APIs, CLI reference, event/capsule/job types — precise enough for DeepSeek workers |
| n01 skeleton | build | — | `Cargo.toml`, `rust-toolchain.toml`, `crates/*/Cargo.toml` + stub `lib.rs`/`main.rs`, `.github/**`, `LICENSE`, `CONTRIBUTING.md`, `SECURITY.md`, `.gitignore` | `cargo build/test/fmt --check/clippy -D warnings` green on the stub workspace; CI file valid |
| n11 framework-templates | docs | — | `framework/**` | Templates + skills + opencode.jsonc; both harnesses load them |
| n02 protocol | build | n00 n01 | `crates/vowp-protocol/**` | serde types + JSON Schema export + tests |
| n03 store | build | n02 | `crates/vowp-store/**` | Schema + migrations, JSONL append/seq/fsync, blobs, reindex, crash test |
| n04 graph | build | n02 | `crates/vowp-graph/**` | DAG, loop edges, states, ownership (globset), scheduler, property tests |
| n05 tmux | build | n01 | `crates/vowp-tmux/**` | Safe manager, tests on a private socket, no respawn trap |
| n06a adapter-opencode | build | n02 | `crates/vowp-adapters/src/opencode/**` | Mapping + errors, fixtures from 2 tiny real runs |
| n06b adapter-claude | build | n02 | `crates/vowp-adapters/src/claude_code/**` | stream-json mapping, resume, limit parser with fixtures |
| n06c adapter-cli | build | n02 | `crates/vowp-adapters/src/cli/**` | Generic mapping-file adapter |
| n07 capsule-switch | build | n03 n06a n06b | `crates/vowp-core/src/session/**` | Capsule writer + switch + export/import, round-trip test |
| n08 supervisor | build | n03 n04 n05 n06a n06b | `crates/vowp-core/src/{loops,gates,ledger,schedules}/**` | L1–L9 + merge queue + failover drill in tests |
| n09 cli | build | n08 (stubs earlier) | `crates/vowp-cli/**` | Every v0.1 command; `--node` over ssh |
| n10 promote | build | n02 n09 | `crates/vowp-core/src/promote/**` | Guards, serving bank, promote/rollback, release log |
| n12 docs | docs (from 21:00, continuous) | n00 | `docs/**`, `README.md`, `examples/**` | Every page in section 13; example runs |
| n13 install-ops | build | n09 | `scripts/**` | install.sh, systemd + cron templates, `vowp doctor` |
| n14 dogfood | build / planner-led | n08 n09 n13 | — | Night switched to `vowp up`; breakages turned into fix jobs |
| n15 fresh-user | verify | n12 n13 | — | A clean Docker `node:24` container follows the README; passes with fake adapters plus one tiny real DeepSeek job |
| n16 site | docs | n12 | `site/**` | Static site builds; not deployed |
| n17 release-prep | release / planner | all | `CHANGELOG.md` | v0.1.0-rc.1 tag (private repo), `npm pack` dry run, morning inbox ready |

Every build job automatically gets a verify job, and possibly fix jobs, per L3.

### 16.4 Rough timeline (CET)

- **19:30–20:30:** bootstrap, then n00 spec (planner), n01 skeleton, n11 templates in parallel.
- **20:30–00:30:** n02, then n03/n04/n05/n06a/n06b/n06c in parallel; docs start.
- **00:30–04:00:** n07, n08, n09, n10, n13; the first Claude limit is likely here, and the keeper takes over planning.
- **04:00–07:00:** n14 dogfood, n15 fresh-user, n16 site, fixes.
- **07:00–08:00:** n17 release prep, night report, inbox.

This is an estimate, not a promise. Must-haves are prioritised so a short night still ends with a usable v0.1.

### 16.5 Guardrails for the night

- Workers touch **only** `~/vowp` worktrees and the private repo. NorthNoir is read-only reference. VOWP never touches tmux `main0`, your CLI film worker, or any NorthNoir deploy.
- **No public actions:**
  - No public repo flip.
  - No npm publish.
  - No Vercel deploy.
  - No new accounts or keys.
- Pushes go only to `job/*` branches and to `main` via the merge queue, in the private repo.
- No `sudo`. No touching other users' files. No reading secret values. The secret-scan gate runs before every push.
- Retry bounds and no-progress detection are on. Paid providers are never switched silently.

### 16.6 Cost (estimate, reported from receipts)

- **DeepSeek V4.1 Flash 1017:** $0.15/M input, $0.60/M output (reasoning counts as output), $0.003/M for cached input (models.dev).
  - Each of the 6 DeepSeek sessions (keeper, verifier, 4 workers) at max reasoning is roughly **$0.20–0.60 per active hour**. Long contexts are mostly cache hits.
  - **Night estimate: about $10–40 in total.**
  - Real totals come from the per-call receipts in the ledger, reported every 2 hours in `NIGHT-REPORT.md`.
- **Claude planner:** runs on your subscription. There's no per-token charge, but it uses your 5-hour window. Expect 1–2 limit hits overnight, each covered by DeepSeek.
- **Already spent:** one variant probe today (a fraction of a cent).

### 16.7 Watching it

- **From the Mac or phone (Termius):** `ssh dev@<your-vps>` then `tmux attach -t vowp-night`. Window 0 is the board.
- **Single files:** `~/vowp-night/NIGHT-REPORT.md` and `~/vowp-night/inbox/`.
- **The opencode app:** the DeepSeek job sessions appear there too.
- **Talking to me:** I re-read the ledger and give you one status answer.

### 16.8 Morning handover

The night report covers what was built, gate evidence, failures and their fixes, real cost, and limit and failover events. It also scores the session fidelity rubric (from your harness plan) for every switch that happened.

The inbox then asks you to:

1. Make `syver777/vowp` public.
2. Publish `vowp@0.1.0` on npm.
3. Deploy the site on Vercel (and choose a domain).
4. Confirm the license.

---

## 17. Risks and mitigations

| Risk | Mitigation |
|---|---|
| DeepSeek Flash drifts on architecture-level work | Claude writes the spec and exact interfaces first (n00). Jobs are small with precise briefs. Every job is verified independently. The planner reviews against evidence. |
| Claude hits its limit mid-plan | Designed failover (L6) with a capsule and probe-back. Tested before handover. |
| DeepSeek credits run out | Jobs park, the inbox records it, no silent switch. The night report says where it stopped. |
| Workers collide on files | Ownership locks, worktrees, and a single-writer merge queue. |
| A worker stops early or idles | Headless runs end; L2 continues automatically (bounded); the keeper diagnoses. |
| Runaway loops | Attempt bounds, repeated-action detection, stall timers. |
| The bootstrap supervisor itself has a bug | Unit tests and two drills before handover; systemd restarts; the planner and keeper can see its log; I check the first hour. |
| It disturbs your other work | Separate tmux session, separate repo, separate worktrees. Provider concurrency drops if DeepSeek rate-limits (your film worker shares the provider). |
| Quality isn't "ready for others" by morning | n15 fresh-user test plus docs checks gate the release candidate. If it isn't ready, the night report says exactly what's missing. |

---

## 18. Decisions for you

| # | Decision | My recommendation |
|---|---|---|
| 1 | Start tonight with this plan? | Yes |
| 2 | Create the **private** repo `syver777/vowp` now, with a deploy key scoped to that repo for the VPS | Yes |
| 3 | License | MIT |
| 4 | Stack | TypeScript / Node ≥ 22 |
| 5 | Public actions | None overnight; you approve them in the morning |
| 6 | Size | 4 workers + keeper + verifier on DeepSeek; planner on Claude Sonnet 5.5 medium |
| 7 | Phone notifications for inbox items | Off for tonight (check the board via Termius); can be added with ntfy later |
