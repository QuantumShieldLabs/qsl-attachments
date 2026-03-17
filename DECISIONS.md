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


- **ID:** D-0003
  - **Status:** Accepted
  - **Date:** 2026-03-17
  - **Goals:** G4, G5
  - **Decision:** `NA-0002` freezes the repo-local deployment / operational hardening contract for `qsl-attachments` without changing runtime code. The repo now states the current single-node local-disk posture truthfully, aligns with qsl-protocol `DOC-ATT-002`, defines the constrained-host validation ladder and readiness categories that the runtime must satisfy next, and makes `NA-0003` the implementation-grade follow-on for operational hardening plus real-world validation.
  - **Invariants:**
    - no plaintext attachment handling on service surfaces
    - no capability-like secrets in canonical URLs
    - no runtime or workflow changes occur in `NA-0002`
    - constrained-host and weak-relay results must distinguish saturation from correctness failure
    - qsl-protocol canonical docs remain authoritative for attachment semantics
    - qsl-server remains separate and transport-only
  - **Alternatives Considered:**
    - treat the single-node local-disk runtime plus minimal `rust` CI as sufficient operational maturity (rejected: does not define deployment/readiness expectations or the constrained-host ladder)
    - implement runtime/deployment changes in `NA-0002` (rejected: out of scope; contract must be frozen before implementation)
    - begin default-path promotion or legacy deprecation from this repo-local item (rejected: blocked until the operational ladder is executed truthfully)
  - **References:** `README.md`; `START_HERE.md`; `NEXT_ACTIONS.md`; `TRACEABILITY.md`; `docs/NA-0002_operational_hardening_contract.md`; `tests/NA-0002_operational_hardening_contract_evidence.md`; qsl-protocol `docs/design/DOC-ATT-002_qsl-attachments_Deployment_and_Operational_Hardening_Contract_v0.1.0_DRAFT.md`
