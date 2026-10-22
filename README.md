# QSL Attachments

**A research-stage service for opaque encrypted file parts.**

This repository implements the existing QSL encrypted-object service: upload sessions, ciphertext parts, commit, retrieval and local storage recovery. Clients encrypt content before uploading it; this service is not a plaintext file processor.

> [!WARNING]
> Not independently audited or production-ready. The runtime uses one node and one local storage root; it does not establish high availability or public deployment safety.

## Scope

The canonical contracts live in [qsl-protocol](https://github.com/QuantumShieldLabs/qsl-protocol/tree/main/docs/canonical): DOC-CAN-005, DOC-CAN-006 and DOC-CAN-007.

This is the existing separate-service attachment plane. It should not be read as evidence that a proposed successor inline-file design, desktop integration or new delivery architecture is implemented.

## Lifecycle and boundaries

```text
Create session -> Upload encrypted parts -> Inspect status -> Commit -> Fetch
                         |
                         +-> Abort or expire
```

| Boundary | Current contract |
|---|---|
| Authorization | Operator-scoped policy plus per-session/object capabilities; no separate end-user service principal |
| Storage | One local disk root on one node |
| Restart | Graceful same-root restart; startup reconciles retained local artifacts |
| Backup | Cold full-root backup/restore with matching configuration |
| Unsupported assurance | Hot/partial restore, cross-file transactional crash durability and multi-node availability |

Within the single-node contract, graceful same-root restart is in scope. The supported backup shape is a cold full-root backup/restore plus matching service configuration. The limits are explicit: hot/live backup and partial restore remain unsupported. Abrupt power-loss and cross-file transactional durability are not established.

Capabilities authorize specific resources. They are not general account credentials and do not establish a person's identity. Ciphertext storage still exposes operational metadata such as access patterns and object sizes.

## Inspect the implementation

Follow [START_HERE.md](START_HERE.md) for the service contracts, existing runbook and evidence. The [CI workflow](.github/workflows/rust.yml) records the local Rust build/test commands. Evidence applies to its named workload and configuration; it is not a production certificate.

## Contribute

Use [CONTRIBUTING.md](CONTRIBUTING.md). Useful reports cover capability scope, quota refusal, recovery, expiry, corrupt/incomplete parts and reproducibility. Use synthetic data and keep secrets out of URLs, screenshots and logs. Follow [SECURITY.md](SECURITY.md) for sensitive findings.

## Related components and license

[Protocol and core](https://github.com/QuantumShieldLabs/qsl-protocol) · [Desktop prototype](https://github.com/QuantumShieldLabs/qsl-desktop) · [Relay](https://github.com/QuantumShieldLabs/qsl-server)

`AGPL-3.0-only`. See [LICENSE](LICENSE).
