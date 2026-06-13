//! Chat delivery adapters (F2, ADR 0002).
//!
//! Pure formatters turning a neutral `(event, data)` webhook payload into the
//! message JSON Slack and Discord expect. Kept dry and terse per
//! `docs/PERSONALITY.md` — a one-line summary plus a link, no frosting.

use serde_json::{json, Value};

/// The task key a payload refers to, if any. Producers use `key` (task
/// events) or `task_key` (comment events).
pub fn task_key(data: &Value) -> Option<&str> {
    data.get("key")
        .or_else(|| data.get("task_key"))
        .and_then(Value::as_str)
}

/// A terse human summary of the event.
pub fn summary(event: &str, data: &Value) -> String {
    let key = task_key(data).unwrap_or("a task");
    match event {
        "task.created" => format!("{key} created"),
        "task.updated" => format!("{key} updated"),
        "task.moved" => format!("{key} moved"),
        "task.deleted" => format!("{key} deleted"),
        "comment.created" => format!("new comment on {key}"),
        "test" => "test event from Sprintly".to_string(),
        other => other.to_string(),
    }
}

/// Slack incoming-webhook body. Slack renders `<url|label>` mrkdwn links in
/// `text`.
pub fn slack_message(event: &str, data: &Value, link: Option<&str>) -> String {
    let text = match link {
        Some(l) => format!("{} <{}|open>", summary(event, data), l),
        None => summary(event, data),
    };
    json!({ "text": text }).to_string()
}

/// Discord webhook body. Discord auto-embeds a bare URL in `content`.
pub fn discord_message(event: &str, data: &Value, link: Option<&str>) -> String {
    let content = match link {
        Some(l) => format!("{} — {}", summary(event, data), l),
        None => summary(event, data),
    };
    json!({ "content": content }).to_string()
}

/// Build the message body for a chat `target_type`. Returns `None` for
/// non-chat targets (generic `outbound` signs its own `{event,data}` body).
pub fn format_message(
    target_type: &str,
    event: &str,
    data: &Value,
    link: Option<&str>,
) -> Option<String> {
    match target_type {
        "slack" => Some(slack_message(event, data, link)),
        "discord" => Some(discord_message(event, data, link)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn task_key_reads_either_field() {
        assert_eq!(task_key(&json!({ "key": "DEMO-1" })), Some("DEMO-1"));
        assert_eq!(task_key(&json!({ "task_key": "WEB-9" })), Some("WEB-9"));
        assert_eq!(task_key(&json!({ "nope": 1 })), None);
    }

    #[test]
    fn summaries_are_terse_and_event_specific() {
        let d = json!({ "key": "DEMO-1" });
        assert_eq!(summary("task.created", &d), "DEMO-1 created");
        assert_eq!(summary("task.moved", &d), "DEMO-1 moved");
        assert_eq!(
            summary("comment.created", &json!({ "task_key": "DEMO-2" })),
            "new comment on DEMO-2"
        );
        // Unknown event falls back to its name; missing key degrades gracefully.
        assert_eq!(summary("weird.thing", &json!({})), "weird.thing");
    }

    #[test]
    fn slack_uses_mrkdwn_link_and_valid_json() {
        let body = slack_message(
            "task.created",
            &json!({ "key": "DEMO-1" }),
            Some("https://pm/t/DEMO-1"),
        );
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["text"], "DEMO-1 created <https://pm/t/DEMO-1|open>");
        // No link → bare summary, still valid JSON.
        let nolink = slack_message("task.moved", &json!({ "key": "X-1" }), None);
        assert_eq!(
            serde_json::from_str::<Value>(&nolink).unwrap()["text"],
            "X-1 moved"
        );
    }

    #[test]
    fn discord_uses_content_with_bare_url() {
        let body = discord_message(
            "task.updated",
            &json!({ "key": "DEMO-1" }),
            Some("https://pm/t/DEMO-1"),
        );
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["content"], "DEMO-1 updated — https://pm/t/DEMO-1");
    }

    #[test]
    fn format_message_routes_by_target() {
        let d = json!({ "key": "DEMO-1" });
        assert!(format_message("slack", "task.created", &d, None).is_some());
        assert!(format_message("discord", "task.created", &d, None).is_some());
        assert!(format_message("outbound", "task.created", &d, None).is_none());
    }

    #[test]
    fn json_escaping_is_safe() {
        // A title-like value with quotes must not break the JSON body.
        let d = json!({ "key": "DEMO-1 \"quoted\" & <html>" });
        let body = slack_message("task.created", &d, None);
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["text"], "DEMO-1 \"quoted\" & <html> created");
    }
}
