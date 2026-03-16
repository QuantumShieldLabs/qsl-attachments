# DECISIONS

- **ID:** D-0001
  - **Status:** Accepted
  - **Date:** 2026-03-16
  - **Decision:** `QuantumShieldLabs/qsl-attachments` exists as the public repo-local runtime lane for the QSL opaque encrypted attachment plane. qsl-protocol owns the canonical control-plane and service-plane contracts; this repo will implement the runtime faithfully against those contracts.
  - **Invariants:**
    - no plaintext attachment handling on service surfaces
    - no capability-like secrets in canonical URLs
    - qsl-server remains separate and transport-only
    - `DOC-CAN-005` and `DOC-CAN-006` remain authoritative until explicitly superseded
  - **References:** qsl-protocol `DOC-CAN-005`; qsl-protocol `DOC-CAN-006`; `NEXT_ACTIONS.md`; `TRACEABILITY.md`


- **ID:** D-0002
  - **Status:** Accepted
  - **Date:** 2026-03-16
  - **Decision:** `NA-0001` uses the smallest faithful runtime implementation for `DOC-CAN-006`: a single-node local-disk service with opaque ciphertext part files, JSON metadata/session journals, process-local locking, dedicated secret-bearing headers (`X-QATT-Resume-Token`, `X-QATT-Fetch-Capability`), deterministic reject bodies, and a minimal `rust` CI lane.
  - **Invariants:**
    - no plaintext attachment handling on service surfaces
    - no capability-like secrets in canonical URLs
    - `DOC-CAN-005` and `DOC-CAN-006` remain authoritative for contract semantics
    - qsl-server remains separate and transport-only
    - branch protection on `main` must require the runtime repo `rust` check once the implementation PR is merged
  - **Alternatives Considered:**
    - cloud/object-store integration in NA-0001 (rejected: out of scope for the smallest faithful runtime)
    - SQLite metadata persistence (not chosen: filesystem JSON journals are smaller for the seed runtime and preserve the same contract semantics)
    - distributed or multi-node locking (rejected: out of scope for the single-node faithful implementation)
  - **References:** `DOC-CAN-005`; `DOC-CAN-006`; `README.md`; `NEXT_ACTIONS.md`; `TRACEABILITY.md`
