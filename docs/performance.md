# Performance

Performance is a feature, a product value, and a project constraint.

`npnpm` should obsess over install latency, warm-cache speed, filesystem efficiency, and scalability under large monorepos.

This is not secondary work after compatibility.
It is core work.

But the target is still pnpm parity.
An optimization that breaks pnpm-compatible behavior is a regression, not a win.

## Performance-first rules

- optimize the hot path, not the pretty path
- benchmark before and after every meaningful install-path change
- prefer removing work over making work slightly cheaper
- treat disk I/O, network I/O, metadata lookups, decompression, and path manipulation as first-class costs
- keep warm installs absurdly fast
- keep frozen-lockfile installs even faster
- never accept accidental O(N^2) behavior in resolution, linking, or scheduler code
- build correctness-preserving fast paths, not benchmark-only hacks
- do not trade away pnpm compatibility for benchmark numbers

## Bun research threads to steal from

These are concrete ideas worth continuously mining from Bun's package manager work:

- filesystem-aware cache and global store reuse
- isolated linker backends with clone, hardlink, and copy fallbacks
- cache-first offline and prefer-offline install modes
- peer resolution fast paths for isolated installs
- scheduler and dependency-unblock logic that avoids quadratic behavior
- robust handling of real-world filesystem edge cases such as `EXDEV`, `DT_UNKNOWN`, NFS, and FUSE
- synchronization fixes that keep parallel install logic fast without becoming flaky

## What this means for implementation

- cache package metadata aggressively
- cache tarball indexes and reuse the store without redownloading
- minimize allocations and string/path churn on the install path
- design data structures for large dependency graphs, not toy projects
- make profiling and benchmarking part of normal development
- prefer native filesystem operations and efficient fallbacks per platform
- keep the lockfile path optimized, because deterministic installs should also be fast installs

## Working principle

When there is a tradeoff, we should keep asking:

1. Can we do less work?
2. Can we reuse work we already paid for?
3. Can we make the remaining work more local, more parallel, or more native?

If the answer is still "not fast enough", keep going.

## External references

- Bun docs: <https://bun.com/docs/pm/global-cache>
- Bun docs: <https://bun.com/docs/pm/isolated-installs>
- Bun PR #21544: <https://github.com/oven-sh/bun/pull/21544>
- Bun PR #21122: <https://github.com/oven-sh/bun/pull/21122>
- Bun PR #21824: <https://github.com/oven-sh/bun/pull/21824>
- Bun PR #25983: <https://github.com/oven-sh/bun/pull/25983>
- Bun PR #26227: <https://github.com/oven-sh/bun/pull/26227>
- Bun PR #21365: <https://github.com/oven-sh/bun/pull/21365>
- Bun PR #23587: <https://github.com/oven-sh/bun/pull/23587>
- Bun PR #27096: <https://github.com/oven-sh/bun/pull/27096>
