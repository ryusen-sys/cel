# Releasing the CEL fork

Releases use Ryusen's Conventional Commit, git-cliff and reviewed release-PR
pattern. **This pipeline publishes GitHub source archives only.** The inherited
`cel-core` and `cel-core-proto` crates.io names belong to upstream; registry
publication requires a separate naming/ownership decision and is not configured.

## Release flow

1. Squash-merge ordinary PRs using their Conventional Commit titles. CI runs
   checks on all PR bases, including stacked branches, and on `main`.
2. A `main` push updates the append-only `release/next` branch. Git-cliff infers
   the next version; one shared version updates the workspace manifest, local
   dependencies, all three workspace lock entries and the fuzz lock's local
   `cel-core` entry. Only fork commits after
   `77277ec6a336646d946ee549a5012116378b82e4` enter `changelog.md`.
3. A rolling draft release PR receives a `Release build dry run` status. Its
   exact head runs formatting, lint, tests, the pinned upstream conformance
   harness and source packaging without release credentials. Only success makes
   the PR ready for review.
4. Merging that PR validates its title/version and the trusted App-created
   status, requires the tested and merged trees to match, then creates its tag.
   That validated workflow directly invokes publication. Tag pushes alone never
   authorize a release.
5. The publisher revalidates the exact tag/commit before receiving credentials,
   then attaches `ryusen-cel-X.Y.Z.tar.gz`, `SHA256SUMS` and git-cliff release notes
   to the GitHub release. The archive includes the pinned cel-spec submodule so
   the inherited conformance harness can run from an extracted release.

Retrying accepts an existing tag only if it points at the expected commit. For
an untagged merged release or interrupted publication, `Recover Untagged Release`
accepts a stable tag and full commit SHA on `main`, repeats tests and packaging,
and validates the version/tag before publication. Do not manually trigger a
release or recovery merely to test CI.

## Titles and changelogs

Allowed types are `feat`, `fix`, `doc`, `docs`, `test`, `ci`, `refactor`, `perf`,
`chore`, `revert`, `style` and `security`. Keep this list aligned with
`.github/workflows/commits.yml` and `cliff.toml`. Features bump minor versions;
fixes and other included changes bump patch versions. Breaking changes use `!`
or a `BREAKING CHANGE:` footer. Planning-only `docs(plan)`/`docs(rfd)`, dependency
chores and generated release commits are excluded. The initial fork release is
`v0.5.2` if no release tag exists; subsequent releases use git-cliff's inference.

```sh
nix develop --command make next-version
nix develop --command make changelog
```

`changelog.prev.md` is the unchanged upstream changelog. Do not regenerate it.
Both CI and local commands use the flake-locked git-cliff executable, not a
downloading action. Package and version checks are exercised by `make check`.

## Required repository settings

Before merging or enabling release workflows, an administrator must configure:

- GitHub Actions enabled and the pinned external actions allowed. Every job
  runs on GitHub-hosted `ubuntu-24.04` runners; no self-hosted runner is needed.
- Squash-only merging using PR titles; protect `main` and require `Validate title`,
  `Check`, `Upstream conformance harness`, `Nix build` and `Differential fuzz smoke`
  checks.
- A `RELEASE` environment restricted to protected `main`, never tags. Set
  `RELEASE_APP_ID` and `RELEASE_APP_SECRET` there using the existing release App.
- Install that App for this repository with contents, pull-request and commit
  status permissions. Permit append-only `release/next` pushes, but no forced
  updates. Keep release PRs out of automatic branch updates and merge queues.
- Protect `v*` tags from updates/deletion and allow creation only by the release
  App. Do not require `Release build dry run` on ordinary PRs; release tagging
  verifies that status and its creator independently.

Ordinary PR checks have read-only tokens and no release secrets. Checkouts never
persist credentials. App tokens are repository-scoped and provided only to
mutation/API steps; networked Git uses `scripts/git-with-token.sh`. Exactly one
reusable workflow publishes, and only validated release merge/recovery paths can
call it with publishing enabled. Removing the old workflow from `main` alone
does not secure historical tag workflows: environment and tag restrictions matter.
