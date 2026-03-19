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


- **ID:** D-0004
  - **Status:** Accepted
  - **Date:** 2026-03-18
  - **Goals:** G4, G5
  - **Decision:** `NA-0003` implements only the smallest operational controls needed for truthful constrained-host validation: a storage-headroom reserve with deterministic `REJECT_QATTSVC_QUOTA` rejects before disk exhaustion, operator-safe startup configuration logging, and a `101 MiB` ciphertext ceiling so the `100 MiB` target class can succeed despite part-cipher overhead. The constrained-host ladder was then executed over the restored real relay and the deployed single-node service on `qsl`; the service-backed ladder, upload-resume stage, direct service-restart stage, reject/expiry paths, and limited concurrency stage all remained contract-faithful, while the exact `4 MiB` legacy-path boundary exposed bounded weak-relay saturation that failed closed without any attachment-service correctness break.
  - **Invariants:**
    - no plaintext attachment handling on service surfaces
    - no capability-like secrets in canonical URLs
    - constrained-host evidence must distinguish bounded saturation from correctness failure
    - weak-relay queue pressure must fail closed without silent state mutation or false delivery semantics
    - qsl-protocol canonical docs remain authoritative for attachment semantics
    - qsl-server remains separate and transport-only
  - **Alternatives Considered:**
    - keep the raw `100 MiB` ciphertext ceiling (rejected: a truthful `100 MiB` target-class run needs room for part-cipher overhead)
    - add broader metrics or deployment automation in this item (rejected: overbuild beyond the smallest controls needed for constrained-host validation)
    - classify the exact `4 MiB` legacy-path queue-full result as an attachment-service correctness failure (rejected: the service path was idle, retries remained bounded, and the relay failed closed)
  - **References:** `README.md`; `START_HERE.md`; `TRACEABILITY.md`; `Cargo.toml`; `src/lib.rs`; `src/main.rs`; `tests/service_contract.rs`; `tests/NA-0003_constrained_host_validation_evidence.md`; qsl-protocol `docs/design/DOC-ATT-002_qsl-attachments_Deployment_and_Operational_Hardening_Contract_v0.1.0_DRAFT.md`


- **ID:** D-0005
  - **Status:** Accepted
  - **Date:** 2026-03-19
  - **Goals:** G4, G5
  - **Decision:** `NA-0004` establishes a materially stronger reference deployment on `qatt` while keeping the real relay on `qsl`, documents the install/update/verify path without storing secrets, and records mixed message + attachment validation over that stronger profile. The service-backed `5 MiB`, `16 MiB`, `64 MiB`, and `100 MiB` attachment runs, mixed traffic, upload-resume, direct service restart, bounded concurrency, and short soak all remained contract-faithful on the stronger host. The remaining degraded threshold cases stayed on the weak relay / legacy path: corrected `< 4 MiB` timed out while still making forward progress, and a fresh exact `4 MiB` rerun failed closed with `relay_inbox_queue_full` on the sender-side final chunk after bounded retries while the attachment-service host remained idle. No qsl-attachments runtime correction was required for the stronger reference deployment, and the honest next blocker becomes a broader mixed message + attachment stress/soak/chaos lane rather than more reference-host hardening or an immediate promotion decision.
  - **Invariants:**
    - no plaintext attachment handling on service surfaces
    - no capability-like secrets in canonical URLs
    - the stronger reference deployment must preserve the single-node local-disk runtime truthfully
    - weak-relay threshold degradation must not be misclassified as an attachment-service correctness failure
    - qsl-protocol canonical docs remain authoritative for attachment semantics
    - qsl-server remains separate and transport-only
  - **Alternatives Considered:**
    - continue using only constrained-host `qsl` for promotion-gate evidence (rejected: does not separate weak-host effects from stronger reference-host evidence)
    - treat the exact `4 MiB` weak-relay queue saturation as a qsl-attachments reference-host defect (rejected: `qatt` remained effectively idle and the service path was not the bottleneck)
    - jump directly to default-path promotion / legacy deprecation after the reference runs (rejected: broader mixed message + attachment stress/soak/chaos evidence still outranks that decision)
  - **References:** `README.md`; `START_HERE.md`; `TRACEABILITY.md`; `docs/NA-0004_reference_deployment_runbook.md`; `tests/NA-0004_reference_deployment_validation_evidence.md`; qsl-protocol `docs/design/DOC-ATT-002_qsl-attachments_Deployment_and_Operational_Hardening_Contract_v0.1.0_DRAFT.md`
