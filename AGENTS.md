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

One package (`src/lib.rs` is the app, `src/main.rs` the wiring) —
hexagonal boundaries are expressed as **modules**, not crates. The
binary is the only consumer; crate-splitting bought nothing here and
cost dependency cycles. The rules below are enforced by discipline
and review, not the compiler — keep them.

- `src/main.rs` + `src/config.rs` — the binary: env config loading +
  wiring (composition root)
- `src/app/` — the application core (synapse's `internal/app`
  analog); nothing here touches SQL, sockets, files, or cloud SDKs:
  - `app/models/` — domain models, one module per aggregate (`auth`,
    `rules`, `mappings`, `modality`, `filter`, `job`). **Types only —
    no traits, no impls**
  - `app/ports.rs` — every port trait, in one module; signatures over
    domain model types, no bodies
  - `app/service/` — orchestrators (`intake`, `forward`) composing
    ports only; always DI structs
  - `app/dicom/` — pure DICOM logic, functions only (`dimse` command
    sets, `anonymize` tag transforms); no sockets, no ports
- `src/infra/` — every adapter that touches the outside world, named
  for what it is: `scp` (DICOM server, drives intake), `scu` (DICOM
  client), `store` (filesystem object store), `db/` (Postgres
  repositories, one module per table; row models and row ↔ domain
  conversions live here, never in `app/models/`), `cloud/` (external
  systems, one module per system — `aws::s3`, `aws::sqs`). Driving
  vs driven is documented in `infra/mod.rs`; nothing in `app`
  imports from `infra`
- `tests/fixtures/` — static port adapters for integration tests
  (Null Objects over `Vec`); no logic, ever
- `migrations/` — sqlx migrations at the workspace root, embedded at
  compile time, run at startup under an advisory lock. **Every
  migration is a reversible pair** (`<version>_<desc>.up.sql` +
  `<version>_<desc>.down.sql`) so prod can be rolled back with
  `sqlx migrate revert`. Every table carries `created_at` /
  `updated_at` (trigger-maintained) / `deleted_at` (soft delete);
  these stay out of the row models

Dependency direction is one-way: `service` → `ports` → `models` ←
`infra`.

## Config boundary

Env holds deployment config (node identity, credentials, backends);
the database holds user-managed domain data (authorized callers,
forwarding rules, study mappings) for the future frontend. See the
README "Configuration" section.

## Testing

- `cargo test` — includes container tests (testcontainers: Postgres,
  LocalStack), so a running **Docker daemon is required**; they fail
  rather than skip without it
- Keep the tree lint-clean: `cargo fmt --all -- --check` and
  `cargo clippy --workspace --all-targets` must both pass before
  pushing
