//! Bounded notification history and validated operations shared by native and socket delivery.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

const MAX_RECORDS: usize = 256;
const MAX_BYTES: usize = 1024 * 1024;

/// User-visible notification content, validated before admission to GTK.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Content {
    #[serde(default = "default_title")]
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub body: String,
}

/// Supply the protocol title when callers omit it.
fn default_title() -> String {
    "Notification".into()
}

impl Content {
    /// Bound each field and reject NUL without logging potentially private message text.
    pub fn validate(&self) -> Result<(), &'static str> {
        for (text, limit) in [
            (&self.title, 512),
            (&self.subtitle, 1024),
            (&self.body, 8192),
        ] {
            if text.len() > limit || text.contains('\0') {
                return Err("notification fields exceed limits or contain NUL");
            }
        }
        Ok(())
    }

    /// Retained content bytes, excluding the fixed-size routing identities.
    fn bytes(&self) -> usize {
        self.title.len() + self.subtitle.len() + self.body.len()
    }
}

/// Stable message identity retains its proven target after closure; None is workspace-only attribution.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Record {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub surface_id: Option<Uuid>,
    #[serde(flatten)]
    pub content: Content,
    pub created_at: String,
    pub is_read: bool,
}

/// Session-owned history; evict read messages first, then the oldest unread message under pressure.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Inbox {
    pub records: Vec<Record>,
}

impl Inbox {
    /// Validate restored content and enforce the same retained-memory bounds as live delivery.
    pub fn validated(mut self) -> Self {
        self.records
            .retain(|record| record.content.validate().is_ok() && record.created_at.len() <= 64);
        self.trim();
        self
    }

    /// Append one validated message and report the number evicted by bounded retention.
    pub fn push(&mut self, record: Record) -> usize {
        self.records.push(record);
        self.trim()
    }

    /// Enforce both record and byte limits without keeping a second content copy.
    fn trim(&mut self) -> usize {
        let mut bytes: usize = self
            .records
            .iter()
            .map(|record| record.content.bytes() + record.created_at.len() + 128)
            .sum();
        let mut evicted = 0;
        while self.records.len() > MAX_RECORDS || bytes > MAX_BYTES {
            let index = self
                .records
                .iter()
                .position(|record| record.is_read)
                .unwrap_or(0);
            let removed = self.records.remove(index);
            bytes -= removed.content.bytes() + removed.created_at.len() + 128;
            evicted += 1;
        }
        evicted
    }
}

/// A scope never silently substitutes another terminal for an explicit missing identity.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Scope {
    #[serde(default, alias = "tab_id")]
    pub workspace_id: Option<Uuid>,
    #[serde(default)]
    pub surface_id: Option<Uuid>,
}

/// Caller evidence is distinct from focus: an ambient workspace may become stale after a pane move.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Caller {
    pub preferred_workspace_id: Option<Uuid>,
    pub preferred_surface_id: Option<Uuid>,
    #[serde(default)]
    pub preferred_workspace_is_explicit: bool,
    pub caller_tty: Option<String>,
    #[serde(default)]
    pub prefer_tty: bool,
}

impl Caller {
    /// Validate bounded caller evidence before crossing to GTK; never interpret a TTY as a file path to open.
    fn parse(params: &Value) -> Result<Self, &'static str> {
        let caller: Self = Self::deserialize(params).map_err(|_| "invalid caller selector")?;
        if caller
            .caller_tty
            .as_ref()
            .is_some_and(|tty| tty.len() > 256 || tty.contains('\0'))
        {
            return Err("invalid caller TTY");
        }
        Ok(caller)
    }
}

/// Fully validated worker-side intent, independent of widgets and transport buffers.
pub enum Action {
    Create {
        scope: Scope,
        content: Content,
    },
    CreateForCaller {
        caller: Caller,
        content: Content,
    },
    ClearForCaller(Caller),
    Clear(Scope),
    MarkRead {
        id: Option<Uuid>,
        scope: Scope,
        all: bool,
    },
    Dismiss {
        id: Option<Uuid>,
        all_read: bool,
    },
    Open(Uuid),
    JumpToUnread,
}

/// Decode strict selector types before crossing the bounded GTK bridge.
pub fn parse(method: &str, params: &Value) -> Result<Action, &'static str> {
    #[derive(Deserialize, Default)]
    struct Selectors {
        #[serde(default)]
        id: Option<Uuid>,
        #[serde(default)]
        all: bool,
        #[serde(default)]
        all_read: bool,
    }
    let selectors: Selectors =
        Selectors::deserialize(params).map_err(|_| "invalid notification selector")?;
    let mut scope: Scope =
        Scope::deserialize(params).map_err(|_| "invalid notification target UUID")?;
    match method {
        "notification.create"
        | "notification.create_for_surface"
        | "notification.create_for_target"
        | "notification.create_for_caller" => {
            if matches!(
                method,
                "notification.create_for_surface" | "notification.create_for_target"
            ) && scope.surface_id.is_none()
            {
                return Err("surface_id is required");
            }
            if method == "notification.create_for_target" && scope.workspace_id.is_none() {
                return Err("workspace_id is required");
            }
            for (field, limit) in [("title", 512), ("subtitle", 1024), ("body", 8192)] {
                if let Some(value) = params.get(field) {
                    let text = value
                        .as_str()
                        .ok_or("notification content must be strings")?;
                    if text.len() > limit || text.contains('\0') {
                        return Err("notification fields exceed limits or contain NUL");
                    }
                }
            }
            let content =
                Content::deserialize(params).map_err(|_| "notification content must be strings")?;
            content.validate()?;
            if method == "notification.create_for_caller" {
                if scope.workspace_id.is_some() || scope.surface_id.is_some() {
                    return Err("caller targeting requires preferred selectors");
                }
                Ok(Action::CreateForCaller {
                    caller: Caller::parse(params)?,
                    content,
                })
            } else {
                Ok(Action::Create { scope, content })
            }
        }
        "notification.clear" => {
            let caller = match params.get("caller") {
                None | Some(Value::Null) => false,
                Some(Value::Bool(value)) => *value,
                _ => return Err("caller must be a boolean"),
            };
            if caller {
                if scope.workspace_id.is_some()
                    || scope.surface_id.is_some()
                    || selectors.id.is_some()
                {
                    return Err("caller clear cannot include explicit scope selectors");
                }
                return Ok(Action::ClearForCaller(Caller::parse(params)?));
            }
            if [
                "preferred_workspace_id",
                "preferred_surface_id",
                "caller_tty",
                "prefer_tty",
                "preferred_workspace_is_explicit",
            ]
            .iter()
            .any(|key| params.get(key).is_some_and(|value| !value.is_null()))
            {
                return Err("caller selectors require caller=true");
            }
            // Retain the original GTK workspace-id alias while supporting upstream scoped/global clear.
            if selectors.id.is_some() && scope.workspace_id.is_some() {
                return Err("use id or workspace_id, not both");
            }
            scope.workspace_id = scope.workspace_id.or(selectors.id);
            Ok(Action::Clear(scope))
        }
        "notification.mark_read" => {
            if usize::from(selectors.id.is_some())
                + usize::from(scope.workspace_id.is_some())
                + usize::from(selectors.all)
                != 1
                || (scope.surface_id.is_some() && scope.workspace_id.is_none())
            {
                return Err("select id, workspace_id (with optional surface_id), or all");
            }
            Ok(Action::MarkRead {
                id: selectors.id,
                scope,
                all: selectors.all,
            })
        }
        "notification.dismiss" => {
            if usize::from(selectors.id.is_some()) + usize::from(selectors.all_read) != 1 {
                return Err("select id or all_read");
            }
            Ok(Action::Dismiss {
                id: selectors.id,
                all_read: selectors.all_read,
            })
        }
        "notification.open" => selectors.id.map(Action::Open).ok_or("id is required"),
        "notification.jump_to_unread" => Ok(Action::JumpToUnread),
        _ => Err("unknown notification operation"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Repeated large deliveries evict history and malformed selectors fail before GTK admission.
    #[test]
    fn retention_and_selector_contracts() {
        let mut inbox = Inbox::default();
        for index in 0..1000 {
            inbox.push(Record {
                id: Uuid::new_v4(),
                workspace_id: Uuid::nil(),
                surface_id: Some(Uuid::nil()),
                content: Content {
                    body: "x".repeat(8192),
                    ..Default::default()
                },
                created_at: "now".into(),
                is_read: index % 2 == 0,
            });
        }
        assert!(inbox.records.len() < 128);
        assert!(inbox.records.iter().all(|record| !record.is_read));
        for params in [
            serde_json::json!({}),
            serde_json::json!({"all": "true"}),
            serde_json::json!({"id": "bad"}),
            serde_json::json!({"all": true, "workspace_id": Uuid::nil()}),
        ] {
            assert!(parse("notification.mark_read", &params).is_err());
        }
        assert!(parse(
            "notification.create_for_surface",
            &serde_json::json!({"surface_id": "missing"})
        )
        .is_err());
        assert!(parse(
            "notification.create",
            &serde_json::json!({"body": "x".repeat(8193)})
        )
        .is_err());
    }
}
