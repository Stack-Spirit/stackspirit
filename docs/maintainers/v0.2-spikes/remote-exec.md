# S2 — remote-exec streaming spike

Status: **done**, 2026-11-10. Job `v2-s2-remote-exec-spike`, owns
`docs/maintainers/v0.2-spikes/remote-exec.md` and `scripts/spikes/remote-exec/**`.

This is the evidence for the §13.3 card in `VOWP-V2-PLAN.md`:

> Prototype streaming JSON lines over SSH into an append-only file with seq assignment; kill SSH
> mid-stream and prove spool + replay with no gap or duplicate; measure latency; decide whether to
> use ControlMaster multiplexing; test a lease-takeover notice reaching the old client within 2 s.

The prototype is Python and deliberately standalone. It does not add a `vowp` command or touch the
v0.1 workspace; `v2-f1-remote-exec` owns the Rust implementation.

## Verdict

| Card question | Answer | Evidence |
|---|---|---|
| Stream JSON lines into an append-only file with `seq`? | Yes. The home is the only seq writer; the client sends stable event ids. | `test_ingest_assigns_contiguous_seq_and_dedupes_by_id`; fault injection is 200/200 with `seq` 1..200. |
| Kill SSH mid-stream, spool and replay with no gap or duplicate? | Yes, including an ack lost after the home appended and a cut in the middle of a line. | 200 events, 3 forced cuts, 12 re-sends, 6 duplicate acks, home log still exactly 200 unique ids in order. |
| Spool survives a killed client, not just a killed SSH? | Yes. A second process resumes the on-disk spool and finishes the turn. | `evidence/client-resume.json`, `test_client_crash_resumes_from_spool`. |
| Latency? | Loopback ack RTT is single-digit to low-teens ms. On a synthetic 80 ms RTT, per-event ack-wait is ~85 ms and ~9 events/s; a 64-event window gives ~250 events/s after setup. | `evidence/live-check.json`; table below. |
| ControlMaster? | **Use it as a reconnect/probe optimisation, not as a correctness dependency.** Handshake p50 falls from 810–953 ms to 243–256 ms at 80 ms RTT (loopback: 217–294 ms to 11–22 ms); reconnect falls from 292–409 ms to 82–92 ms on loopback. It does not reduce per-event RTT. | `bench` and `reconnect-*` in `evidence/live-check.json`. |
| Takeover notice within 2 s? | Yes. Real SSH: 100–265 ms across runs; hermetic poll: below 2 s with a 100 ms poll. | `lease-revoke` in `evidence/live-check.json`; `test_lease_revoked_notice_reaches_the_client_within_2s`. |

Decision for F1/F2: the protocol and spool/replay model are sound; build them in Rust. Use one
long-lived `ssh -T` channel per turn, with acks and lease notices on the channel's stdout. Use
ControlMaster for reconnects and probes, but always keep the cold path working. Push lease notices
from the lease sweep; do not make the old client wait for its next heartbeat.

## What the prototype does

```text
laptop executor (client.py)                    home (ingest.py)
  synthetic event ── JSON line ───────────►  validate lease
        ▲                                    assign seq, append one line
        │   ack{id, seq, duplicate}          ack after append
        │   lease.revoked / done             fsync at end
        └──────────── JSON lines ───────────┘
  spool.jsonl + acks.jsonl
```

The client writes the event to a local spool **before** sending it. It records acks by event id.
On a dropped channel it reconnects and sends every event without an ack. The home checks the id
against its log: a known id with the same content is acknowledged with its original `seq`; a known
id with different content is rejected and never appended.

Files (all under `scripts/spikes/remote-exec/`):

| File | Purpose |
|---|---|
| `wire.py` | frames, append-only log scanner/truncator, event generator, latency stats, verifier |
| `ingest.py` | home writer: single-writer lock, seq, idempotency, torn-tail repair, lease watch |
| `client.py` | spool/ack/replay, heartbeat, takeover notice, fake and real SSH transports |
| `fake_ssh.py` | hermetic SSH-shaped proxy with `ack_lost` and `mid_line` cuts |
| `live_check.py` | private loopback sshd, ControlMaster benchmark, delay proxy, live evidence |
| `remote_exec_spike.py` | CLI: `run`, `resume`, `ingest`, `fake-ssh`, `takeover`, `verify`, `live` |
| `tests/test_remote_exec.py` | 10 hermetic tests |
| `evidence/` | sanitized fault-injection, resume, hermetic-test and live evidence |

## Protocol

One compact JSON object per line. The home -> client stream carries only acks and control frames;
it never carries harness output.

| Frame | Direction | Fields |
|---|---|---|
| `hello` | client → home | `version`, `session`, `lease`, `client` |
| `hello.ack` | home → client | `session`, `last_seq`, `events` |
| `event` | client → home | `id`, `session`, `event` (the EventEnvelope body) |
| `ack` | home → client | `id`, `seq`, `duplicate` |
| `heartbeat` | client → home | `ts` |
| `heartbeat.ack` | home → client | `seq` |
| `lease.revoked` | home → client | `reason`, `by`, `at`, `last_seq` |
| `end` | client → home | `last_client_seq` |
| `done` | home → client | `events`, `last_seq`, `duplicates` |
| `error` | home → client | `code`, `message` |

Invariants proven by the tests:

1. `seq` starts at 1 and increases by exactly 1; the home assigns it, never the client.
2. A duplicate id with the same event content is acked with its original `seq` and not appended.
3. A duplicate id with different content returns `error{code: duplicate-conflict}` and is not appended.
4. A torn final line (bytes after the last newline) is truncated before the next append; corruption
   before a complete final line is an error, not a silent repair.
5. The home holds one writer lock per session. A new ingest after an SSH drop waits for the old
   process to release it, then scans and repairs the log.
6. A completed turn fsyncs the log; per-event fsync is optional and was measured but is not needed
   for the v0.1 `sync()` rule.

## Fault-injection evidence

Hermetic suite:

```sh
scripts/spikes/remote-exec/run.sh
```

Result (full output in `evidence/hermetic-tests.txt`): **10 tests, OK**.

The main scenario uses three cuts in one 200-event run:

```json
{"cuts":[
  {"connection":0,"after_lines":37,"mode":"ack_lost"},
  {"connection":1,"after_lines":41,"mode":"mid_line"},
  {"connection":2,"after_lines":23,"mode":"ack_lost"}
]}
```

`ack_lost` forwards the event to the home, waits until the home wrote the ack, then drops the ack
and kills the channel. `mid_line` forwards only the first half of a line before killing the channel.
The captured result (`evidence/fault-injection.json`):

| Metric | Result |
|---|---|
| Client status / exit | `ok` / 0 |
| Events in spool | 200 |
| Home records / last seq | 200 / 200 |
| Reconnects | 3 |
| Event frames sent | 212 |
| Re-sent frames | 12 |
| Duplicate acks | 6 |
| `verify` | `ok: true`, errors `[]` |
| Cuts observed | 3: two `ack_lost` (home ack seen for both) and one `mid_line` (partial bytes only) |

The verifier compares every home line with the spool: count, order, id, `seq` and event content.
A gap or duplicate is a failure, so this test cannot pass by only checking `true`.

Client-crash scenario (`evidence/client-resume.json`):

| Phase | Result |
|---|---|
| First process, 120 events, `max-reconnects=0` | exit 75, `reconnect-limit`, 57 acks, 58 sends, home has 58 records |
| New process, `resume`, no cut | exit 0, 120 acks, 63 sends, duplicate acks 1 |
| Home after resume | 120 records, `seq` 1..120, verifier `ok: true` |

The first process was deliberately unable to reconnect; the second had no memory of the first and
read only `spool.jsonl` + `acks.jsonl`. The duplicate ack is the event that was appended before the
first process stopped.

Other tests reject a reused id with different content, reject an inactive lease, truncate a torn
tail before appending, reject a gap in the home log, and make the verifier fail on a gap.

## Latency and throughput

Four live runs on the same Linux 6.8 / OpenSSH 9.6 / Python 3.12 host, with a private loopback
sshd. The "delay" rows go through a loopback TCP proxy that sleeps 40 ms one-way before every
forwarded chunk (~80 ms RTT). The host was shared with other jobs and load varied; the ranges below
show that spread. The committed `evidence/live-check.json` is one of the runs
(`2026-11-10T12:17:23Z`, lease notice 99.6 ms); the other runs measured 120–265 ms.

Handshake (`ssh <target> true`; 8 samples loopback, 4 samples delayed per run):

| Path | Cold p50 | Mux p50 |
|---|---:|---:|
| loopback | 216.9–294.0 ms | 10.5–22.1 ms |
| ~80 ms RTT | 810.3–952.7 ms | 243.1–256.3 ms |

Streams (ranges over the runs; the committed run is the high end where noted):

| Scenario | Window | Events | Total | Ack p50 | Ack p95 | Connect p50 |
|---|---:|---:|---:|---:|---:|---:|
| loopback, cold | 64 | 400 | 0.298–1.081 s | 3.9–18.1 ms | 4.5–61.9 ms | 273–562 ms |
| loopback, mux | 64 | 400 | 0.151–0.279 s | 3.3–20.0 ms | 5.0–64.0 ms | 83–131 ms |
| ~80 ms RTT, cold | 1 | 16 | 2.43–2.87 s | 82.0–86.7 ms | 94.5–175.7 ms | 926–1119 ms |
| ~80 ms RTT, cold | 64 | 120 | 1.36–2.24 s | 127.7–176.3 ms | 166.1–221.1 ms | 872–1760 ms |
| ~80 ms RTT, mux | 64 | 120 | 0.76–1.54 s | 132.5–165.5 ms | 161.8–174.3 ms | 290–1042 ms |

One loaded run measured 110 ms p50 on the 400-event loopback-cold stream; the same scenario was
3.9–18.1 ms in the other runs. Treat the loopback p50 as a transport floor, not a promise under
CPU contention. The delayed mux stream at the high end had lost its master and fallen back to a cold
connection (see the ControlMaster caveat); the live-master runs were 0.76–0.81 s total.

Throughput including SSH setup is ~6 events/s with a one-event window, 54–89 events/s with a
64-event window, and 78–158 events/s with mux. Excluding setup, the 64-event delayed runs are
~230–260 events/s. The per-event RTT is a floor set by the network (~85 ms at 80 ms RTT); a
larger window does not reduce it, it amortises it. **Do not wait for one ack per event across a
WAN.**

Reconnect after `SIGKILL` of the client's `ssh` process at event 100 (300 events, window 8):

| Scenario | Reconnect | Total | Re-sent | Duplicate acks | Verify |
|---|---:|---:|---:|---:|---|
| cold | 291.6–409.2 ms | 0.83–1.19 s | 8 | 0–8 | ok |
| mux | 81.6–92.2 ms | 0.36–0.57 s | 6–8 | 1–8 | ok |

The reconnect number is the time from spawning the replacement transport to `hello.ack`, so it
includes SSH setup and the remote ingest process start. Every run still verified 300/300.

## ControlMaster decision

Use a ControlMaster per home **as an optimisation**, with the cold path always available:

- Measured savings are one full handshake: 217–294 → 11–22 ms loopback, 810–953 → 243–256 ms at
  80 ms RTT, and 292–409 → 82–92 ms on reconnect. It does not improve per-event ack RTT.
- Start a dedicated master (`ssh -M -N -S <socket>`) while a session is active, or use
  `ControlMaster=auto` with a real persistent master. Do not make the first streaming `ssh` the only
  master: killing that stream process also kills the master, and the next connection is cold.
- Keep the socket under `~/.vowp/tmp/` (mode 0700), never `/tmp`, and use a short `%C`-based name
  (the Unix socket path limit is ~100 bytes). `ControlPersist=60` is a reasonable idle window for a
  session that may reconnect.
- Treat a missing/dead master as a normal condition. In the live runs, OpenSSH's `-S` client fell
  back to a cold connection when the master was gone; the turn still completed and verified. F1
  should record the fallback and re-arm the master, not fail the turn.
- Do not use a pty for the stream. `ssh -T -o BatchMode=yes` is enough; a pty would merge stderr,
  alter line endings and expose terminal control sequences.

## Lease takeover notice

The home watches the lease while it is idle and writes a `lease.revoked` frame on the same stdout
stream. The client has a reader thread, so it prints the notice even while the harness is quiet.

Measured:

- Hermetic test: notice below 2 s with the home's 100 ms poll.
- Live SSH: **99.6–265 ms** from the takeover write to the client receiving the frame (the client
  prints "lease revoked: taken over by phone ...; this surface is read-only" and exits 75 without
  sending more events). The committed run is 99.6 ms; the other runs were 120–265 ms.
- The home appended no event after the revocation.

The prototype uses a JSON lease file as a stand-in for the global DB row. F2 should not poll SQLite
per event: `surface::sweep_leases()` (the C0 hook) should signal the ingest writer through an
in-process condvar/channel, with a <=250 ms poll only as a fallback. The takeover must push, not wait
for the old client's 5 s heartbeat.

## Recommendations for F1/F2

**`vowp __exec` (client)**

- Spool per turn under `~/.vowp/spool/<session>/<turn>.jsonl` (mode 0600/0700), with an `acks`
  sidecar; fsync before sending, compact only after the session closes. Cap the spool (event count
  and bytes) and fail the turn loudly rather than dropping events.
- Event ids are client-generated ULIDs. Never send a client `seq`; the home assigns it.
- Use an in-flight window (32–64 worked here) and acknowledge asynchronously. On reconnect, resend
  from the first unacked event; idempotency makes over-sending safe.
- Heartbeat every 1 s on the same channel; do not open a second channel just for the lease. A
  `lease.revoked` notice stops input immediately; the surface then follows read-only.
- `ssh -T -F /dev/null` plus explicit `BatchMode=yes`, `IdentitiesOnly=yes` and the private key;
  never inherit the user's interactive ssh config for a turn. ControlMaster options as above.
- Larger events need no special client path: the home applies the existing 8 KiB blob spill after
  ingest. The spike only tested 8 KiB payloads, not a full 64 KiB+ tool result.

**`vowp session ingest` (home)**

- Take the session writer lock; wait for a previous ingest to exit. Scan the log, truncate a torn
  final line, verify contiguous `seq`; refuse to append to corruption.
- Append one line per accepted event, ack after the write, and fsync at `turn.ended`/`end` (the
  v0.1 sync points), not per event.
- Deduplicate by `(event id, content hash)`. A conflict is a client bug and must not append.
- Check the lease before every event and while idle. Push `lease.revoked` immediately; stop
  accepting bytes from the old surface. Reject a `hello` whose token is not the active lease.
- Pass the lease token in the `hello` frame, not `--lease <token>` on the command line: the command
  line is visible to `ps` on the home. Keep a `--lease` flag only if a caller explicitly accepts that
  exposure.
- The remote probe TODO in `switch.rs` can use the same transport: `ssh -T ... vowp doctor --json`
  through the same master, with the existing 10 s connect timeout.

**Named tests to port to F1**

`remote_stream_assigns_contiguous_seq`, `remote_stream_ack_loss_replay_has_no_duplicate`,
`remote_stream_mid_line_drop_has_no_gap`, `remote_stream_duplicate_id_content_conflict`,
`remote_stream_truncates_torn_tail`, `remote_stream_lease_revoked_notice_within_2s`,
`remote_stream_rejects_invalid_lease`, `remote_stream_spool_survives_client_crash`,
`remote_stream_large_events`, `remote_probe_remote_node_uses_ssh`, and an opt-in
`remote_stream_control_master_falls_back_when_dead`.

## Reproduce

```sh
scripts/spikes/remote-exec/run.sh
scripts/spikes/remote-exec/run.sh --live \
  --out scripts/spikes/remote-exec/evidence/live-check.json
```

The live command starts `/usr/sbin/sshd` as the current user on a free `127.0.0.1` port with
temporary host/client keys; the client children get a temporary `HOME`, `VOWP_HOME` and `TMPDIR`. It
never touches the real VOWP home, the user's tmux server, an external network or a real harness. On a
machine without sshd it prints `skipped` (exit 0); the hermetic suite still runs.

A direct cut/replay run:

```sh
TMP=$(mktemp -d)
mkdir -p "$TMP/work" "$TMP/home"
python3 - "$TMP" <<'PY'
import json, sys
from pathlib import Path
root = Path(sys.argv[1])
(root / "home" / "lease.json").write_text(json.dumps({
    "token": "tok-spike", "state": "active", "holder": "laptop",
    "by": None, "reason": None, "at": "2026-01-01T00:00:00Z"}))
(root / "plan.json").write_text(json.dumps({"cuts": [
    {"connection": 0, "after_lines": 7, "mode": "ack_lost"},
    {"connection": 1, "after_lines": 5, "mode": "mid_line"}]}))
PY
scripts/spikes/remote-exec/remote_exec_spike.py run \
  --workdir "$TMP/work" --home-dir "$TMP/home" --lease-file "$TMP/home/lease.json" \
  --fake-ssh-plan "$TMP/plan.json" --fake-ssh-trace "$TMP/trace.jsonl" \
  --events 20 --window 1 --heartbeat-interval 5 --max-reconnects 4
scripts/spikes/remote-exec/remote_exec_spike.py verify \
  --home-log "$TMP/home/log.jsonl" --spool "$TMP/work/spool.jsonl"
```

Expected: `status=ok`, `reconnects=2`, `resent=2`, `duplicate_acks=1`, verifier `ok: true` with
`home_records=20` and `last_seq=20`.

## Limits and open questions

- This is a Python prototype with a synthetic event source. It does not measure a real harness,
  a real VOWP DB, blob spill, secret redaction or the acceptance rig (T0 was not merged when this
  ran). F1 owns those.
- Live measurements are loopback plus an artificial 40 ms one-way proxy, not a real WAN. The
  ControlMaster conclusion is based on handshake and reconnect measurements; a real WAN test belongs
  to the acceptance rig.
- The lease file poll models a DB lookup. The 100–265 ms live notice is a design proof, not the final
  F2 notification path; an in-process signal should be faster and cheaper.
- The separate ControlMaster was sometimes closed by the peer under the crude delay proxy. The cold
  fallback still completed every turn. F1 should treat master death as expected and covered.
- The `--lease <token>` argv question is a recommendation, not an owner decision: the plan names
  that flag, but a token in `ps` is avoidable with the `hello` frame.
- Hermetic tests are portable. The live script is Linux-oriented (`/usr/sbin/sshd`); macOS can run
  the hermetic suite, and the workspace's cargo gates do not depend on this directory.
