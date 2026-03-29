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


- **ID:** D-0006
  - **Status:** Accepted
  - **Date:** 2026-03-20
  - **Goals:** G4, G5
  - **Decision:** `NA-0005` executes the bounded kitchen-sink validation lane without changing attachment semantics: `qsl` remains the weak-host / weak-relay baseline, `qatt` remains the stronger reference deployment, and the integrated message + attachment system is exercised through mixed traffic, concurrency ramps, restart/recovery, and a `30` minute mixed soak. The reference host stayed bounded through large files, restart, resumed upload, concurrency up to `8`, and short soak. The only degraded required stages remained on the weak-host / weak-relay legacy threshold path, where `< 4 MiB` and exact `4 MiB` failed closed with explicit `relay_inbox_queue_full` pressure after bounded retries. No qsl-attachments correctness failure or load-bearing deployment immaturity was proven, so the honest next blocker becomes the default attachment-path promotion / legacy in-message deprecation decision rather than another stress or hardening lane.
  - **Invariants:**
    - no plaintext attachment handling on service surfaces
    - no capability-like secrets in canonical URLs
    - weak-host / weak-relay degradation must stay distinct from attachment-service correctness
    - the stronger reference deployment must preserve the current single-node local-disk runtime truthfully
    - qsl-protocol canonical docs remain authoritative for attachment semantics
    - qsl-server remains separate and transport-only
  - **Alternatives Considered:**
    - classify the weak threshold failures as qsl-attachments correctness defects (rejected: they remained explicit relay-side bounded saturation with no dishonest delivery state)
    - open another qsl-attachments-local hardening lane after this evidence (rejected: no direct repo-local blocker outranked the default-path / legacy decision)
    - continue the kitchen-sink lane beyond concurrency `8` and a `30` minute soak in this item (rejected: bounded evidence is already strong enough to move the blocker back to qsl-protocol)
  - **References:** `README.md`; `START_HERE.md`; `TRACEABILITY.md`; `tests/NA-0005_stress_soak_chaos_evidence.md`; qsl-protocol `NEXT_ACTIONS.md`; qsl-protocol `TRACEABILITY.md`


- **ID:** D-0007
  - **Status:** Accepted
  - **Date:** 2026-03-28
  - **Goals:** G4, G5
  - **Decision:** `NA-0007` freezes the qsl-attachments authn/authz / policy-subject contract as an operator-scoped single-node deployment model. The sole current service policy subject is the operator-controlled deployment; `resume_token` and `fetch_capability` remain per-resource authorizers rather than service-account identities; deployment-global quota and abuse ceilings remain owned by that operator-scoped deployment subject; and any later repo-local `Authorization` layer must stay deployment-local unless a new contract item explicitly broadens the model.
  - **Invariants:**
    - no plaintext attachment handling on service surfaces
    - no capability-like secrets in canonical URLs
    - qsl-attachments remains opaque ciphertext-only
    - qsl-server remains separate and transport-only
    - `session_id`, `locator_ref`, and `attachment_id` remain resource references, not service principals
    - passive logs and evidence continue to prefer short deterministic handles over full stable identifiers
  - **Alternatives Considered:**
    - treat `attachment_id`, `session_id`, or `locator_ref` as implicit service identities (rejected: they are resource references and would create dishonest auth semantics)
    - hold the repo in an operator-scoped continued-support posture without freezing the contract (rejected: the evidence is already sufficient to freeze the boundary and move to explicit implementation)
    - define a multi-tenant or peer-identity service auth layer now (rejected: not supported by current evidence and would invent new semantics)
  - **References:** `README.md`; `START_HERE.md`; `TRACEABILITY.md`; `docs/NA-0007_authn_authz_policy_subject_contract.md`; `tests/NA-0003_constrained_host_validation_evidence.md`; `tests/NA-0004_reference_deployment_validation_evidence.md`; `tests/NA-0005_stress_soak_chaos_evidence.md`; `src/lib.rs`; `tests/service_contract.rs`; qsl-protocol `docs/canonical/DOC-CAN-006_QATT_Attachment_Service_Contract_v0.1.0_DRAFT.md`; qsl-protocol `docs/design/DOC-G5-004_Metadata_Leakage_Surface_Review_and_Logging_Contract_v0.1.0_DRAFT.md`


- **ID:** D-0008
  - **Status:** Accepted
  - **Date:** 2026-03-29
  - **Goals:** G4, G5
  - **Decision:** `NA-0008` implements the frozen operator-scoped authn/authz / policy-subject contract by making the deployment subject explicit on operator-safe runtime surfaces instead of adding a new service auth layer. `Config::operator_policy_surface()` and the startup summary now state that the sole current service policy subject is the operator-scoped deployment, quotas are deployment-global, `Authorization` remains reserved/undefined, resource refs are not principals, and many transfers remain allowed when deployment policy/quota allows them even though each `resume_token` / `fetch_capability` remains scoped to one session/object.
  - **Invariants:**
    - no plaintext attachment handling on service surfaces
    - no capability-like secrets in canonical URLs
    - qsl-attachments remains opaque ciphertext-only
    - qsl-server remains separate and transport-only
    - no per-user, per-peer, per-device, or attachment-owner service identity is introduced
    - resource-scoped capabilities remain exact-match authorizers for one session/object only
  - **Alternatives Considered:**
    - add a repo-local `Authorization` layer in this item (rejected: not required to implement the frozen contract faithfully and would widen the validated deployment surface)
    - keep the operator-scoped deployment subject implicit in runtime/operator surfaces (rejected: leaves the direct implementation lane unfinished and does not provide deterministic proof of the frozen wording)
    - reinterpret resource refs or capabilities as service principals (rejected: dishonest to the frozen contract and would invent new semantics)
  - **References:** `README.md`; `START_HERE.md`; `docs/NA-0007_authn_authz_policy_subject_contract.md`; `src/lib.rs`; `src/main.rs`; `tests/service_contract.rs`; qsl-protocol `docs/canonical/DOC-CAN-006_QATT_Attachment_Service_Contract_v0.1.0_DRAFT.md`; qsl-protocol `DECISIONS.md`


- **ID:** D-0009
  - **Status:** Accepted
  - **Date:** 2026-03-29
  - **Goals:** G4, G5
  - **Decision:** `NA-0009` freezes the qsl-attachments durability / recovery contract as a single-node local-disk boundary. One operator-managed storage root remains the whole durability domain; graceful same-root restart is in scope; cold whole-root backup/restore plus matching service configuration is the only supported backup shape; committed-object recovery requires both `object.json` and `ciphertext.bin`; and abrupt-crash/open-session survival plus cross-file transactional durability are not promised under the current runtime. The truthful result is `DRC0` / closeout path `AV1`: implementation of deterministic crash/recovery handling is now the next blocker.
  - **Invariants:**
    - no plaintext attachment handling on service surfaces
    - no capability-like secrets in canonical URLs
    - qsl-attachments remains opaque ciphertext-only
    - qsl-server remains separate and transport-only
    - the durability boundary remains one operator-managed local storage root on one node only
    - hot/live backup, multi-node semantics, and cross-file transactional durability are not claimed by this contract
  - **Alternatives Considered:**
    - claim one smaller durability-design gap still blocks implementation (`DRC1`) (rejected: the contract can already freeze the unsupported hot-backup/open-session guarantees and the committed-object recovery boundary without further semantic invention)
    - keep the repo in a continued-support/operator-scoped posture without freezing the contract (`DRC2`) (rejected: that would hide the real next blocker, which is deterministic implementation of crash/recovery handling under the already-evident local-disk boundary)
    - define distributed, object-store, or replicated durability semantics now (rejected: not supported by current evidence and out of scope for the current operator-scoped service)
  - **References:** `README.md`; `START_HERE.md`; `TRACEABILITY.md`; `docs/NA-0009_durability_recovery_contract.md`; `src/lib.rs`; `tests/service_contract.rs`; `tests/NA-0003_constrained_host_validation_evidence.md`; `tests/NA-0004_reference_deployment_validation_evidence.md`; `tests/NA-0005_stress_soak_chaos_evidence.md`; qsl-protocol `docs/canonical/DOC-CAN-006_QATT_Attachment_Service_Contract_v0.1.0_DRAFT.md`; qsl-protocol `docs/design/DOC-ATT-002_qsl-attachments_Deployment_and_Operational_Hardening_Contract_v0.1.0_DRAFT.md`; qsl-protocol `docs/design/DOC-G5-004_Metadata_Leakage_Surface_Review_and_Logging_Contract_v0.1.0_DRAFT.md`


- **ID:** D-0010
  - **Status:** Accepted
  - **Date:** 2026-03-29
  - **Goals:** G4, G5
  - **Decision:** `NA-0010A` completes merged-lane durability / recovery validation and cleanup without changing attachment-service runtime semantics. The repo-local operator surfaces now state on the same top-level paths that hot/live backup and partial restore remain unsupported while graceful same-root restart and cold full-root backup/restore stay the only supported recovery boundary, `tests/NA-0010A_durability_recovery_validation_evidence.md` records the post-merge validation matrix truthfully, and `tests/service_contract.rs` now proves those docs/evidence surfaces remain aligned with the frozen contract while secret-safe audit-handle coverage stays intact. The truthful result is that no direct repo-local durability validation/finalization gap remains after this cleanup.
  - **Invariants:**
    - no plaintext attachment handling on service surfaces
    - no capability-like secrets in canonical URLs
    - qsl-attachments remains opaque ciphertext-only
    - qsl-server remains separate and transport-only
    - graceful restart remains same-root only, and cold full-root backup/restore plus matching service configuration remains the only supported backup shape
    - abrupt-crash/open-session survival, hot/live backup, partial restore, and cross-file transactional durability remain unsupported
  - **Alternatives Considered:**
    - promote a direct repo-local durability finalization lane (`NA-0010B`) immediately (rejected: the remaining gaps were operator-surface wording and post-merge evidence cleanup only, and those are now closed with deterministic proof)
    - widen this item into stronger storage semantics or runtime redesign (rejected: the frozen contract is already unambiguous and this lane is validation/cleanup only)
    - leave the unsupported-case wording implicit on top-level operator docs (rejected: it leaves stale assumptions in the very surfaces the validation lane is meant to clean up)
  - **References:** `README.md`; `START_HERE.md`; `TRACEABILITY.md`; `docs/NA-0009_durability_recovery_contract.md`; `tests/NA-0010A_durability_recovery_validation_evidence.md`; `tests/service_contract.rs`; qsl-protocol `DECISIONS.md`
