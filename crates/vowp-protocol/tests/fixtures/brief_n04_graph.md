---
job: n04-graph
owns: ["crates/vowp-graph/**"]
must_not_touch: ["everything else"]
needs_done: [n02a-protocol-foundation]
gates: [report, "cargo test -p vowp-graph", owns_respected, review]
done_when: "report has status: done and every gate passes"
---
# n04-graph

Implement the DAG and the state machine.
