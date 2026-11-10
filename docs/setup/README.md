# Setup docs (draft)

> **Status: planned (draft, v0.2).** This directory is new for v0.2 and is **not linked from the
> root README or any docs index yet**. The contract is a draft: the `vowp setup` commands are
> planned for v0.2 and are not in a released binary. The final page and the root README prompt are
> published with the v0.2 release. Until then, nothing here is an installation instruction for a
> released binary.

| File | What it is |
|---|---|
| [`agent-setup.md`](agent-setup.md) | The contract every harness follows: ask once, show the plan once, run it after one yes, stop at hand-offs, verify, report. Marked draft. |
| `README.md` | This staging page and the copyable README prompt. Not the root README. |

## The README prompt

This is the exact block to put in the first section of the root `README.md`. Keep it copyable: no
extra prose inside the fenced block.

```text
Set up VOWP <version> for me. Read
https://raw.githubusercontent.com/syver777/vowp/v<version>/docs/setup/agent-setup.md
and follow it exactly. Ask me only what it tells you to ask, show me the full plan once,
and run it after I approve. Never ask me to paste a secret into this chat.
```

`<version>` is replaced twice with the release version, without the leading `v`: the prose says
`VOWP 0.2.0` and the URL says `/v0.2.0/`. For 0.2.0 the contract URL is:

```text
https://raw.githubusercontent.com/syver777/vowp/v0.2.0/docs/setup/agent-setup.md
```

The tag must exist before the prompt is published. Do not point the prompt at `main` or a branch;
`main` can change after the owner reads it, and the contract is version-pinned on purpose.

## What the prompt must keep

Every phrase in the prompt is load-bearing:

| Phrase | Why it stays |
|---|---|
| `Set up VOWP <version> for me.` | The version is the one the contract and the release describe. |
| The raw URL to the **exact tag** | The harness reads the contract for the same release it is installing. |
| `follow it exactly` | The setup commands, not the harness's improvisation, do the work. |
| `Ask me only what it tells you to ask` | The contract asks five questions in one message, then works from the answers. |
| `show me the full plan once` | The owner approves the whole plan, not a phase at a time. |
| `run it after I approve` | One approval covers the plan; no setup phase runs before it. The contract's one pre-plan exception is the local CLI bootstrap, which the owner is told about in the question message. |
| `Never ask me to paste a secret into this chat.` | Values go through the hidden prompt or the one-time page, never the transcript. |

## Before publishing

- Replace `<version>` in the prose and the URL, and confirm the tag and the raw URL resolve.
- Put the prompt in the first README section, before the getting-started steps, so it is the first
  thing a reader can copy.
- Link [`agent-setup.md`](agent-setup.md) for people who want to read the contract before running
  it.
- Change the draft banner in `agent-setup.md` to `Status: current` and remove the sentences that
  call `vowp setup`/`vowp secret` unreleased. Do this only after `vowp setup` is implemented and
  `setup verify` passes. Update or remove this staging page too; it must not ship with a draft
  banner.
- Run the documentation checks: every relative link must resolve, and planned commands must stay
  marked as planned.
- Rehearse the rendered prompt in a harness on a clean control machine before publishing it.

## Draft boundary

The contract describes `vowp setup plan|home|control|verify` and the secret hand-offs as the v0.2
specification defines them. It does not describe v0.1 behaviour: v0.1 has no `setup` command, no
control role and no vault. The only pre-release use of this directory is review; nothing links to
it until the release documentation lands.
