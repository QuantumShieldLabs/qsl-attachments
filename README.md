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
- deployment / operational hardening and constrained-host real-world validation are the current blocker before any default-path promotion or legacy deprecation discussion

Runtime shape in this item:
- opaque ciphertext part files on local disk
- local metadata/session journals persisted as JSON
- create/upload/status/commit/abort/retrieval lifecycle from `DOC-CAN-006`
- single-range ciphertext retrieval support

Operational posture:
- this repo is still only the current single-node local-disk runtime
- `main` currently requires only the `rust` check
- no deployment automation, multi-node storage backend, or operational hardening implementation is present yet
- the implementation-grade operational contract now lives in `docs/NA-0002_operational_hardening_contract.md`

Canonical references:
- https://github.com/QuantumShieldLabs/qsl-protocol/blob/main/docs/canonical/DOC-CAN-005_QSP_Attachment_Descriptor_and_Control_Plane_v0.1.0_DRAFT.md
- https://github.com/QuantumShieldLabs/qsl-protocol/blob/main/docs/canonical/DOC-CAN-006_QATT_Attachment_Service_Contract_v0.1.0_DRAFT.md
- https://github.com/QuantumShieldLabs/qsl-protocol/blob/main/docs/canonical/DOC-CAN-007_QATT_Attachment_Encryption_Context_and_Part_Cipher_v0.1.0_DRAFT.md
- https://github.com/QuantumShieldLabs/qsl-protocol/blob/main/docs/design/DOC-ATT-002_qsl-attachments_Deployment_and_Operational_Hardening_Contract_v0.1.0_DRAFT.md
