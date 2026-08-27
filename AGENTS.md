# ferritin — agent notes

## Git workflow

- **PRs are always squash-merged** — main stays clean and linear.
- Because of that: **never stack PRs**. A stacked branch carries the
  original commits of its base; once the base is squash-merged, the
  stack conflicts against main with duplicate diffs. One branch per
  PR, always cut from the latest `main`. Wait for the merge before
  starting dependent work.
- Direct pushes to `main` are rejected (branch protection).
- If a PR conflicts after its base was squash-merged: merge
  `origin/main` into the PR branch, resolve (usually the branch side
  is correct), run the suite, push.

## Layout

- `src/` — the binary: env config loading + wiring
- `crates/ferritin-core/src/domain/` — domain types and ports, one
  module per aggregate (`auth`, `rules`, `mappings`, `models`);
  nothing here knows SQL or sockets exist
- `crates/ferritin-core/src/db/` — database layer, one repository
  module per table/aggregate implementing its port for `PgStore`;
  a new table gets its own module. Row models and row ↔ domain
  conversions live here, never in `domain/`
- `crates/ferritin-cloud/` — adapters to external systems, one module
  per system (`aws::s3`, `aws::sqs`)
- `migrations/` — sqlx migrations at the workspace root, embedded at
  compile time, run at startup under an advisory lock. **Every
  migration is a reversible pair** (`<version>_<desc>.up.sql` +
  `<version>_<desc>.down.sql`) so prod can be rolled back with
  `sqlx migrate revert`. Every table carries `created_at` /
  `updated_at` (trigger-maintained) / `deleted_at` (soft delete);
  these stay out of the row models

## Config boundary

Env holds deployment config (node identity, credentials, backends);
the database holds user-managed domain data (authorized callers,
forwarding rules, study mappings) for the future frontend. See the
README "Configuration" section.

## Testing

- `cargo test --workspace` — includes container tests
  (testcontainers: Postgres, LocalStack), so a running **Docker
  daemon is required**; they fail rather than skip without it
