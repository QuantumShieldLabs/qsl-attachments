# NA-0001 Runtime Contract Faithfulness

## Source Of Truth
- `DOC-CAN-005` defines the attachment descriptor/control-plane contract.
- `DOC-CAN-006` defines the attachment service/session/object runtime contract.
- This repo implements the service runtime only. It does not change canonical semantics.

## Runtime Shape
- Single-node local-disk runtime.
- Opaque ciphertext parts and committed objects stored on disk.
- Session and object metadata stored as local JSON journals.
- Secret-bearing material carried only in:
  - `X-QATT-Resume-Token`
  - `X-QATT-Fetch-Capability`

## Contract To Code
- Session creation, upload, status, commit, abort, retrieval, and range handling:
  - `src/lib.rs`
- Runtime entrypoint:
  - `src/main.rs`
- Deterministic contract-faithfulness tests:
  - `tests/service_contract.rs`
- Minimal runtime CI:
  - `.github/workflows/rust.yml`

## Deterministic Coverage
- Create session success.
- Upload part success.
- Status / resume visibility.
- Commit success after complete parts.
- Abort success and post-abort reject behavior.
- Retrieval success only after commit.
- Missing / invalid resume token rejects without mutation.
- Missing / invalid fetch capability rejects without mutation.
- Part index / part shape rejects without mutation.
- Session and object expiry behavior.
- Quota-limit rejects.
- Valid single-range retrieval.
- Audit-log secret / plaintext redaction.
- Invalid fetch capability abuse escalation.
- Canonical URL query-string secret-carriage rejection.

## Safety Invariants
- No plaintext attachment handling on service surfaces.
- No capability-like secrets in canonical URLs.
- qsl-server remains untouched and transport-only.
- qsc/client integration remains deferred to qsl-protocol `NA-0197C`.
