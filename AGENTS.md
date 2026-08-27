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

- `src/` — the binary: env config loading + wiring (composition root)
- `crates/ferritin-core/src/models/` — domain models, one module per
  aggregate (`auth`, `rules`, `mappings`, `modality`); nothing here
  knows SQL or sockets exist. **Types only — no traits, no impls**
- `crates/ferritin-core/src/ports.rs` — every port trait, in one
  module (synapse's `app/ports` analog); signatures over domain
  model types, no bodies
- `crates/ferritin-core/src/service/` — orchestrators (`intake`,
  `forward`) composing domain ports only; no concrete infrastructure
  types. Services here are always DI structs; pure DICOM logic lives
  in `dicom/`, not here
- `crates/ferritin-core/src/dicom/` — pure DICOM logic, functions
  only (`dimse` command sets, `anonymize` tag transforms); no
  sockets, no ports
- `crates/ferritin-core/src/db/` — database layer, one repository
  module per table/aggregate implementing its port for `PgStore`;
  a new table gets its own module. Row models and row ↔ domain
  conversions live here, never in `models/`
- `crates/ferritin-core/tests/fixtures/` — static port adapters for
  integration tests (Null Objects over `Vec`); no logic, ever
- `crates/ferritin-core/src/{scp,scu,store}.rs` — edge
  adapters: DICOM network I/O and the filesystem object store
- `crates/ferritin-cloud/` — adapters to external systems, one module
  per system (`aws::s3`, `aws::sqs`)
- `migrations/` — sqlx migrations at the workspace root, embedded at
  compile time, run at startup under an advisory lock. **Every
  migration is a reversible pair** (`<version>_<desc>.up.sql` +
  `<version>_<desc>.down.sql`) so prod can be rolled back with
  `sqlx migrate revert`. Every table carries `created_at` /
  `updated_at` (trigger-maintained) / `deleted_at` (soft delete);
  these stay out of the row models

Dependency direction is one-way: `service` → `ports` → `models` ←
`db` / edge adapters / `ferritin-cloud`.

## Config boundary

Env holds deployment config (node identity, credentials, backends);
the database holds user-managed domain data (authorized callers,
forwarding rules, study mappings) for the future frontend. See the
README "Configuration" section.

## Testing

- `cargo test --workspace` — includes container tests
  (testcontainers: Postgres, LocalStack), so a running **Docker
  daemon is required**; they fail rather than skip without it
- Keep the tree lint-clean: `cargo fmt --all -- --check` and
  `cargo clippy --workspace --all-targets` must both pass before
  pushing
