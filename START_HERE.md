# Start here

Begin with the [overview](README.md), then select the evidence relevant to your question.

| Question | Document |
|---|---|
| Who is authorized? | [Authentication and authorization](docs/NA-0007_authn_authz_policy_subject_contract.md) |
| What survives restart or restore? | [Durability and recovery](docs/NA-0009_durability_recovery_contract.md) |
| What bounds operational behavior? | [Operational hardening](docs/NA-0002_operational_hardening_contract.md) |
| What did the constrained-host scenario check? | [Validation evidence](tests/NA-0003_constrained_host_validation_evidence.md) |
| What does the reference setup require? | [Reference runbook](docs/NA-0004_reference_deployment_runbook.md) |
| How do I contribute? | [CONTRIBUTING.md](CONTRIBUTING.md) |

The protocol contracts are DOC-CAN-005, DOC-CAN-006 and DOC-CAN-007 in the [canonical directory](https://github.com/QuantumShieldLabs/qsl-protocol/tree/main/docs/canonical).

Each evidence record retains its own source, workload and limits. The reference runbook is not production approval.

Within the single-node contract, graceful same-root restart is in scope. The supported backup shape is a cold full-root backup/restore plus matching service configuration. The limits are explicit: hot/live backup and partial restore remain unsupported. Abrupt power-loss and cross-file transactional durability are not established.

This service is distinct from proposed successor inline-file delivery. See [SECURITY.md](SECURITY.md) before reporting sensitive findings.
