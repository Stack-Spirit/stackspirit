# VOWP — Virtual Open Work-to-Production

**Private work in progress.** VOWP is an open-source framework (Rust, MIT) for running AI coding agents —
Claude Code, opencode and other stock harnesses — as a team on your own machines: a laptop, a VPS, or both.
Work is planned as a graph of jobs, deterministic loops keep it moving, every job runs in a tmux window you
can watch, sessions can move between harnesses, models and machines without losing context, and finished
work reaches production only through guarded, reversible promotions. The design is in
[`docs/design/plan.md`](docs/design/plan.md); usable documentation will follow before the first release.
