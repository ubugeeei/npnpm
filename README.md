# npnpm

Fast like Bun. Elegant like pnpm. Built with respect for pacquet.

> Very WIP.
> Expect missing features, rough edges, broken compatibility, and frequent rewrites.
> The vision is ambitious. The implementation is still early.

`npnpm` is an experimental Rust package manager aiming for full pnpm compatibility with a native core inspired by Bun's package manager architecture.

This repository currently grows from the `pacquet` fork, so parts of the codebase and the CLI still use the `pacquet` name while the project direction is being shaped.

Performance is the first non-negotiable.
If a design is elegant but slow, we still have more work to do.

Full pnpm compatibility is also a core goal.
Speed matters, but not by drifting away from pnpm semantics.

## Concept

- Fast like Bun: reduce redundant network and filesystem work, keep warm installs fast, and push more of the hot path into native code.
- Elegant like pnpm: keep the content-addressable store, deterministic lockfiles, and isolated install model easy to reason about.
- Fully compatible with pnpm: match pnpm behavior, lockfiles, workspace semantics, and CLI expectations as closely as possible.
- Respect for pacquet: evolve the fork carefully, keep the implementation readable, and learn from pacquet instead of erasing it.

More detail lives in [docs/concept.md](./docs/concept.md).
Performance notes live in [docs/performance.md](./docs/performance.md).

## Status

This project is very much a work in progress and not production ready.

If you try it today, assume things will break.

Implemented today:

- `.npmrc` support
- basic `add`, `install`, `run`, `test`, `start`, and `store` CLI commands
- content-addressable store support
- frozen lockfile installs
- initial `pnpm-lock.yaml` generation and reuse
- store-index based warm-cache tarball reuse

Still in progress:

- full `pnpm-lock.yaml` fidelity
- full pnpm CLI and behavior parity
- workspace support
- `node_modules/.bin` generation
- pnpm error and reporter parity
- deeper Bun-style install optimizations

## Debugging

```sh
TRACE=pacquet_tarball just cli add fastify
```

## Testing

```sh
# Install necessary dependencies
just install

# Start a mocked registry server (optional)
just registry-mock launch

# Run tests
just test
```

## Benchmarking

Start a local registry server first, such as [verdaccio](https://verdaccio.org/):

```sh
verdaccio
```

Then run the integrated benchmark:

```sh
# Compare the branch you're working on against main
just integrated-benchmark --scenario=frozen-lockfile my-branch main
```

```sh
# Compare current commit against the previous commit
just integrated-benchmark --scenario=frozen-lockfile HEAD HEAD~
```

```sh
# Compare current pacquet against pnpm
just integrated-benchmark --scenario=frozen-lockfile --with-pnpm HEAD
```

```sh
# Compare current pacquet, main, and pnpm together
just integrated-benchmark --scenario=frozen-lockfile --with-pnpm HEAD main
```

```sh
# See more options
just integrated-benchmark --help
```
