//! Fixed agent launch choices persisted independently for each terminal tab.

/// Agent selected by the pane's launch buttons; arbitrary commands are not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentKind {
    Codex,
    Claude,
}

impl AgentKind {
    /// Human-readable tab and button label.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude",
        }
    }
}

/// A terminal's launch choice; changing resume_last affects its next restoration only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AgentTerminal {
    pub(crate) kind: AgentKind,
    #[serde(default)]
    pub(crate) resume_last: bool,
}

impl AgentTerminal {
    /// Start a fresh agent in a newly created tab, with restoration opt-in initially off.
    pub(crate) fn new(kind: AgentKind) -> Self {
        Self {
            kind,
            resume_last: false,
        }
    }

    /// Literal shell input for this fixed launch choice; the transport supplies Enter separately.
    pub(crate) fn command(self) -> &'static str {
        match (self.kind, self.resume_last) {
            (AgentKind::Codex, false) => "codex",
            (AgentKind::Codex, true) => "codex resume --last",
            (AgentKind::Claude, false) => "claude",
            (AgentKind::Claude, true) => "claude --continue",
        }
    }
}
