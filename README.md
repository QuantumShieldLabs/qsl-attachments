# qsl-attachments

Public runtime home for the QSL opaque encrypted attachment plane.

This repository implements the attachment service/runtime defined by qsl-protocol:
- control-plane descriptor contract: `DOC-CAN-005`
- attachment service contract: `DOC-CAN-006`
- attachment encryption-context and part-cipher contract: `DOC-CAN-007`

Public posture:
- AGPL-3.0-only
- opaque encrypted attachment handling only
- no plaintext attachment handling on service surfaces
- no capability-like secrets in canonical URLs
- qsl-server remains a separate transport-only relay surface
- qsl-protocol remains the canonical source of truth for attachment control-plane and service-plane docs

Current state:
- single-node local-disk runtime implementation
- deterministic contract-faithfulness tests and minimal runtime CI
- qsc/client integration exists upstream in qsl-protocol
- constrained-host operational hardening and real-world validation now have direct evidence in `tests/NA-0003_constrained_host_validation_evidence.md`
- stronger reference-deployment validation and promotion-gate evidence now exist in `tests/NA-0004_reference_deployment_validation_evidence.md`
- bounded stress/soak/chaos evidence now exists in `tests/NA-0005_stress_soak_chaos_evidence.md`

Runtime shape in this item:
- opaque ciphertext part files on local disk
- local metadata/session journals persisted as JSON
- create/upload/status/commit/abort/retrieval lifecycle from `DOC-CAN-006`
- single-range ciphertext retrieval support

Operational posture:
- this repo is still only the current single-node local-disk runtime
- `main` currently requires only the `rust` check
- startup now emits an operator-safe runtime configuration summary, and storage-headroom rejects fail closed before weak hosts exhaust disk during validation
- no deployment automation or multi-node storage backend is present yet
- the authn/authz / policy-subject contract now lives in `docs/NA-0007_authn_authz_policy_subject_contract.md`
- the durability / recovery contract now lives in `docs/NA-0009_durability_recovery_contract.md`
- the runtime now exposes an explicit operator policy surface and startup summary stating that the sole current service policy subject is the operator-scoped deployment, quotas are deployment-global, resource refs are not principals, and `Authorization` remains reserved/undefined
- current service auth remains operator-scoped deployment policy plus per-session/object capability authorization; many transfers remain allowed when deployment policy/quota allows them, but each `resume_token`/`fetch_capability` still authorizes exactly one session/object and no separate end-user service principal exists today
- the current durability boundary is one local storage root on one node; graceful same-root restart is in scope; cold full-root backup/restore plus matching service configuration is the only supported backup shape; hot/live backup and partial restore remain unsupported; and abrupt-crash/open-session recovery remains fail-closed plus bounded operator cleanup rather than cross-file transactional durability
- startup now reconciles that storage root explicitly: only coherent open sessions and committed objects are re-exposed, orphaned staged/object artifacts are discarded, and the service emits an operator-safe recovery summary instead of inventing stronger crash semantics
- the implementation-grade operational contract now lives in `docs/NA-0002_operational_hardening_contract.md`
- constrained-host execution evidence now lives in `tests/NA-0003_constrained_host_validation_evidence.md`
- the stronger reference-host install/update path now lives in `docs/NA-0004_reference_deployment_runbook.md`
- stronger reference-deployment validation evidence now lives in `tests/NA-0004_reference_deployment_validation_evidence.md`
- bounded stress/soak/chaos evidence now lives in `tests/NA-0005_stress_soak_chaos_evidence.md`

Canonical references:
- https://github.com/QuantumShieldLabs/qsl-protocol/blob/main/docs/canonical/DOC-CAN-005_QSP_Attachment_Descriptor_and_Control_Plane_v0.1.0_DRAFT.md
- https://github.com/QuantumShieldLabs/qsl-protocol/blob/main/docs/canonical/DOC-CAN-006_QATT_Attachment_Service_Contract_v0.1.0_DRAFT.md
- https://github.com/QuantumShieldLabs/qsl-protocol/blob/main/docs/canonical/DOC-CAN-007_QATT_Attachment_Encryption_Context_and_Part_Cipher_v0.1.0_DRAFT.md
- https://github.com/QuantumShieldLabs/qsl-protocol/blob/main/docs/design/DOC-ATT-002_qsl-attachments_Deployment_and_Operational_Hardening_Contract_v0.1.0_DRAFT.md
