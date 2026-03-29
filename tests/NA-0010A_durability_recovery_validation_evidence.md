Status: Authoritative
Owner: qsl-attachments governance
Last-Updated: 2026-03-29

# NA-0010A Durability / Recovery Validation Evidence

Goals: G4, G5

## 1. Validation result

`NA-0010A` is a validation/cleanup lane, not a new durability-design lane.

The merged runtime behavior from `NA-0010` remains unchanged and was revalidated locally against the frozen contract:
- Graceful same-root restart remains in scope and only coherent open sessions are re-exposed.
- Committed-object recovery still requires both `object.json` and `ciphertext.bin`.
- Incoherent/orphaned local recovery artifacts are still discarded fail-closed rather than reconstructed.
- Hot/live backup and partial restore remain unsupported.
- Abrupt-crash/open-session survival remains unsupported beyond bounded fail-closed cleanup on the same local storage root.
- Secret-safe passive surfaces remain intact through deterministic handle/redaction coverage, including `audit_log_redacts_secrets_plaintext_and_full_identifiers`.

## 2. Operator-surface cleanup completed

The top-level operator-facing docs now say the same thing as the frozen contract:
- `README.md` and `START_HERE.md` still declare graceful same-root restart as the supported restart boundary.
- Those same surfaces now also say explicitly that cold full-root backup/restore plus matching service configuration is the only supported backup shape.
- Those same surfaces now say explicitly that hot/live backup and partial restore remain unsupported.
- Those same surfaces continue to keep abrupt-crash/open-session recovery fail-closed and bounded to operator cleanup/discard rather than cross-file transactional guarantees.

## 3. Local validation bundle

The required local bundle for this validation/cleanup lane completed green on the merged cleanup diff:
- `cargo fmt --all -- --check` -> PASS
- `cargo clippy --all-targets -- -D warnings` -> PASS
- `cargo build --locked` -> PASS
- `cargo test --locked` -> PASS (`28` integration/unit tests green; `0` failed)

## 4. Deterministic proof points

Targeted proof remains present and green for the merged lane:
- `graceful_same_root_restart_recovers_coherent_session_and_discards_orphan_parts`
- `restart_discards_incoherent_session_when_journaled_part_is_missing`
- `committed_object_recovery_requires_object_json_and_ciphertext_bin`
- `durability_docs_and_validation_evidence_state_restart_backup_and_unsupported_cases_truthfully`
- `audit_log_redacts_secrets_plaintext_and_full_identifiers`

## 5. Truthful remaining posture

No new durability semantics were added here.

The current truthful posture remains:
- one operator-managed local storage root on one node is the entire durability boundary
- same-root graceful restart is supported
- cold/quiesced full-root backup/restore plus matching configuration is the only supported backup shape
- abrupt-crash open-session survival, hot/live backup, partial restore, and cross-file transactional durability remain unsupported
- bounded operator discard of orphaned or incoherent local artifacts remains allowed
