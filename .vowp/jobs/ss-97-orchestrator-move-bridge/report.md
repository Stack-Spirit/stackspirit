---
job: ss-97-orchestrator-move-bridge
status: blocked
branch: job/ss-97-orchestrator-move-bridge
commits: [6eaa42c]
tests: "fmt ok; clippy ok; cargo test -p spirit-core ok (incl. 5 new orchestrator_move tests); npm lint ok; npm test ok; npm run typecheck FAILS (see questions_for_owner)"
---

# ss-97 orchestrator move bridge

Implemented: `OrchestratorMoveTarget`, `VowpClient::orchestrator_move` (trait default Gate error, SSH impl with exact `--json orchestrator move <name> --node|--home` argv, WRITE_TIMEOUT, exit 127/unrecognized -> `not_supported:`, mock with `orchestrator_move_calls`), `LocalOrchestratorAction::OrchestratorMove` (requires_ready, no path map), `SpiritCore::orchestrator_move`, Tauri `local_orchestrator_move` registered in lib.rs, and `wire.ts` / `SpiritBridge.ts` / `MockBridge.ts` bridge support. The only change outside the brief's list of edited files is making `validate_orchestrator_name` `pub(crate)` in the owned service.rs so the SSH client reuses it.

Tests: exact node and home argv, ready gate allows, waiting gate / disconnected agent / disabled feature (asserted on the guard, since `set_gates` derives the flag) refuse, invalid name/node rejected before the client call, exit 127 / unrecognized subcommand gives `not_supported`.

## questions_for_owner

1. The brief's `packages/spirit-ui/src/bridge/TauriBridge.ts` does not exist. The real file is `apps/spirit-desktop/src/bridge/TauriBridge.ts`, which is outside `owns`. Because `SpiritBridge.localOrchestrators.orchestratorMove` is now required, `npm run typecheck` fails at `TauriBridge.ts(305,3)` TS2416 until that file adds:
   `orchestratorMove: (name: string, target: OrchestratorMoveTarget): Promise<OrchestratorView> => call("local_orchestrator_move", { name, target }),`
   I did not edit it to respect `owns`. Please either add that path to `owns` (and re-run this job) or apply the one-line addition in a follow-up.
