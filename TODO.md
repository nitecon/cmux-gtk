# Remaining work

Use the requirement matrix and evidence in [RefactorAudit](Docs/RefactorAudit.md) for the active architecture refactor. The inherited macOS checklist has been removed because its checked items did not describe this Linux implementation; it remains available in Git history.

Product requirements worth reassessing during the relevant work include caller-relative command targeting for background agents, consistent split direction and identity semantics, tab movement and focus preservation, browser/remote lifecycle diagnostics, and user-visible loading and failure states. These are requirements to verify against current behavior, not claims that each feature is absent or newly authorized.

Session resume, agent resume hooks, browser automation parity and notifications/inbox enhancements remain deferred until explicitly started. Their saved specification is separate from the active refactor. The historical [browser port specification](docs/agent-browser-port-spec.md) supplies upstream design context, not current Linux API or test guarantees.
