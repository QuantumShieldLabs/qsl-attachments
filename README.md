# qsl-attachments

Public runtime home for the QSL opaque encrypted attachment plane.

This repository exists to implement the attachment service/runtime defined by qsl-protocol:
- control-plane descriptor contract: `DOC-CAN-005`
- attachment service contract: `DOC-CAN-006`

Public posture:
- AGPL-3.0-only
- opaque encrypted attachment handling only
- no plaintext attachment handling on service surfaces
- no capability-like secrets in canonical URLs
- qsl-server remains a separate transport-only relay surface
- qsl-protocol remains the canonical source of truth for attachment control-plane and service-plane docs

Current state:
- governance/bootstrap only
- no runtime implementation has landed yet

Primary next item:
- `NA-0001 — Attachment Service Runtime Implementation`

Canonical references:
- https://github.com/QuantumShieldLabs/qsl-protocol/blob/main/docs/canonical/DOC-CAN-005_QSP_Attachment_Descriptor_and_Control_Plane_v0.1.0_DRAFT.md
- https://github.com/QuantumShieldLabs/qsl-protocol/blob/main/docs/canonical/DOC-CAN-006_QATT_Attachment_Service_Contract_v0.1.0_DRAFT.md
