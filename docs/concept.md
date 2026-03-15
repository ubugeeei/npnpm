# Concept

`npnpm` is guided by one sentence:

> Fast like Bun. Elegant like pnpm. Built with respect for pacquet.

This document describes the direction of the project, not its current level of completeness.

`npnpm` is still deeply WIP.
Expect unfinished features, behavior changes, and large implementation gaps while the core is taking shape.

## Fast like Bun

Speed is a product feature, not a benchmark trick.
For this project, it is also a primary design constraint.

We want installs to feel native:

- reuse the global store aggressively
- avoid downloading or unpacking the same package twice
- keep warm-cache and frozen-lockfile installs especially fast
- move performance-critical logic into Rust instead of shelling out
- treat every avoidable syscall, allocation, hash map lookup, and network roundtrip as suspect

## Elegant like pnpm

Performance should not come at the cost of clarity.
And it should not come at the cost of pnpm compatibility either.

We want the package manager to stay understandable:

- deterministic lockfiles
- isolated installs backed by a content-addressable store
- workspace behavior that scales without becoming magical
- compatibility with pnpm's mental model, not just its command names

## Full pnpm compatibility

`npnpm` should aim for full pnpm compatibility, not partial familiarity.

That means chasing parity in:

- lockfile behavior
- workspace semantics
- linker and store behavior
- CLI flags and command expectations
- error cases and edge-case resolution behavior

If we are faster but observably less pnpm-compatible, the work is not finished.

## Respect for pacquet

This project starts from a pacquet fork, and that matters.

Respect means:

- building incrementally instead of rewriting everything at once
- keeping the codebase readable while it evolves
- preserving useful ideas from pacquet even when the implementation changes
- treating the fork as a foundation, not disposable scaffolding

## Near-term priorities

- make `pnpm-lock.yaml` generation and updates reliable
- close the feature gap around workspaces, peers, and `.bin`
- improve warm-cache and offline behavior
- keep benchmarking against pnpm and Bun as the implementation matures
- keep a living performance playbook based on Bun's package manager work
