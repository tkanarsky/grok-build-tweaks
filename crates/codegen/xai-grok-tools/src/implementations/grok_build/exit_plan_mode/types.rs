//! Wire types for the `x.ai/exit_plan_mode` ACP ext_method.
//!
//! Shared between the shell (serializer) and the pager/desktop/VS Code
//! (deserializer) so both sides stay in sync.

use std::ops::Range;

fn u64_is_zero(n: &u64) -> bool {
    *n == 0
}

/// Advertised on `initialize` response `_meta`. Absent/`false` means the shell
/// only understands the legacy `feedback` slug (rendered comments + freeform).
/// `true` means `comments[]` plus `feedback` as raw freeform.
pub const PLAN_REVIEW_COMMENTS_CAPABILITY: &str = "x.ai/planReviewComments";

/// A line-anchored review comment on a plan body.
///
/// `line_range` is 1-based, half-open (`start..end`), the same shape the pager
/// already uses. serde emits `{ "start": n, "end": m }` for the range.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanComment {
    pub id: u64,
    pub line_range: Range<usize>,
    pub text: String,
}

/// Saved chips on an in-progress review. Does **not** snapshot the plan body:
/// the current file is `plan.md`, and the parked reverse-request already
/// carries `planContent`. Duplicating the body here would be a second source
/// of truth that drifts the moment the file is edited.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanCommentSet {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<PlanComment>,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub next_comment_id: u64,
}

/// ACP `ext_method` request payload (shell coordinator sends to client/pager).
///
/// Serialized as `camelCase` for the ACP JSON-RPC wire format.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitPlanModeExtRequest {
    pub session_id: String,
    pub tool_call_id: String,
    pub plan_content: Option<String>,
    /// Open chips to hydrate the overlay. Default empty for old shells.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<PlanComment>,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub next_comment_id: u64,
}

/// ACP `ext_method` response payload (client/pager returns to shell coordinator).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExitPlanModeExtResponse {
    /// `"approved"`, `"cancelled"`, or `"abandoned"`.
    pub outcome: String,
    /// Line comments attached to this verdict. Default empty for old clients.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<PlanComment>,
    /// Freeform notes. Old clients that flattened chips into this field still
    /// work when `comments` is empty: the shell treats `feedback` as the whole
    /// user message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
}

/// Model-facing review text from chips + optional freeform.
///
/// When `plan_content` is present, each chip quotes those 1-based lines.
/// Otherwise it falls back to `@plan.md:N`.
pub fn format_plan_review(
    comments: &[PlanComment],
    plan_content: Option<&str>,
    feedback: Option<&str>,
) -> String {
    let mut parts: Vec<String> = comments
        .iter()
        .map(|comment| {
            if let Some(body) = plan_content {
                let label = if comment.line_range.len() == 1 {
                    format!("Proposed plan line {}:", comment.line_range.start)
                } else {
                    format!(
                        "Proposed plan lines {}-{}:",
                        comment.line_range.start,
                        comment.line_range.end.saturating_sub(1)
                    )
                };
                let snippets = plan_line_snippets(body, &comment.line_range);
                format!("{label}\n{snippets}\n\nComment:\n{}", comment.text)
            } else {
                let line = comment.line_range.start;
                format!("@plan.md:{line}\n{}", comment.text)
            }
        })
        .collect();
    if let Some(text) = feedback.map(str::trim).filter(|s| !s.is_empty()) {
        if comments.is_empty() {
            parts.push(text.to_owned());
        } else {
            parts.push(format!("Additional feedback:\n{text}"));
        }
    }
    parts.join("\n\n")
}

fn plan_line_snippets(content: &str, range: &Range<usize>) -> String {
    let quoted: Vec<String> = content
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let n = i + 1;
            (range.start..range.end)
                .contains(&n)
                .then(|| format!("> {line}"))
        })
        .collect();
    if quoted.is_empty() {
        "> [selected lines unavailable]".to_owned()
    } else {
        quoted.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ext_request_serializes_camel_case() {
        let req = ExitPlanModeExtRequest {
            session_id: "sess-1".into(),
            tool_call_id: "tc-1".into(),
            plan_content: Some("# Plan".into()),
            ..Default::default()
        };
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("sessionId").is_some());
        assert!(json.get("toolCallId").is_some());
        assert!(json.get("planContent").is_some());
        // Must NOT contain snake_case keys
        assert!(json.get("session_id").is_none());
        assert!(json.get("tool_call_id").is_none());
        assert!(json.get("plan_content").is_none());
    }

    #[test]
    fn ext_request_round_trips() {
        let req = ExitPlanModeExtRequest {
            session_id: "sess-1".into(),
            tool_call_id: "tc-1".into(),
            plan_content: Some("# Plan\n\n## Step 1\nDo something".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: ExitPlanModeExtRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.session_id, "sess-1");
        assert_eq!(back.tool_call_id, "tc-1");
        assert_eq!(
            back.plan_content.as_deref(),
            Some("# Plan\n\n## Step 1\nDo something")
        );
    }

    #[test]
    fn ext_request_round_trips_no_plan() {
        let req = ExitPlanModeExtRequest {
            session_id: "sess-2".into(),
            tool_call_id: "tc-2".into(),
            plan_content: None,
            ..Default::default()
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: ExitPlanModeExtRequest = serde_json::from_str(&json).unwrap();
        assert!(back.plan_content.is_none());
    }

    #[test]
    fn ext_response_approved_round_trips() {
        let resp = ExitPlanModeExtResponse {
            outcome: "approved".into(),
            comments: Vec::new(),
            feedback: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: ExitPlanModeExtResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.outcome, "approved");
        assert!(back.comments.is_empty());
        assert!(back.feedback.is_none());
    }

    #[test]
    fn ext_response_cancelled_with_feedback_round_trips() {
        let resp = ExitPlanModeExtResponse {
            outcome: "cancelled".into(),
            comments: Vec::new(),
            feedback: Some("Please add error handling".into()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: ExitPlanModeExtResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.outcome, "cancelled");
        assert_eq!(back.feedback.as_deref(), Some("Please add error handling"));
    }

    #[test]
    fn ext_response_omits_none_feedback() {
        let resp = ExitPlanModeExtResponse {
            outcome: "approved".into(),
            comments: Vec::new(),
            feedback: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("feedback").is_none());
        assert!(json.get("comments").is_none());
    }

    #[test]
    fn ext_response_abandoned_round_trips() {
        let resp = ExitPlanModeExtResponse {
            outcome: "abandoned".into(),
            comments: Vec::new(),
            feedback: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: ExitPlanModeExtResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.outcome, "abandoned");
        assert!(back.feedback.is_none());
    }

    #[test]
    fn plan_comment_range_serializes_as_start_end() {
        let comment = PlanComment {
            id: 3,
            line_range: 2..5,
            text: "nit".into(),
        };
        let json = serde_json::to_value(&comment).unwrap();
        assert_eq!(json["id"], 3);
        assert_eq!(json["lineRange"]["start"], 2);
        assert_eq!(json["lineRange"]["end"], 5);
        assert_eq!(json["text"], "nit");
        let back: PlanComment = serde_json::from_value(json).unwrap();
        assert_eq!(back.line_range, 2..5);
    }

    #[test]
    fn old_request_json_defaults_comments() {
        let back: ExitPlanModeExtRequest =
            serde_json::from_str(r#"{"sessionId":"s","toolCallId":"t","planContent":"plan"}"#)
                .unwrap();
        assert!(back.comments.is_empty());
        assert_eq!(back.next_comment_id, 0);
    }

    #[test]
    fn old_response_json_defaults_comments_and_keeps_feedback() {
        let back: ExitPlanModeExtResponse =
            serde_json::from_str(r#"{"outcome":"cancelled","feedback":"please rewrite auth"}"#)
                .unwrap();
        assert!(back.comments.is_empty());
        assert_eq!(back.feedback.as_deref(), Some("please rewrite auth"));
    }

    #[test]
    fn request_with_comments_round_trips() {
        let req = ExitPlanModeExtRequest {
            session_id: "s".into(),
            tool_call_id: "t".into(),
            plan_content: Some("# p".into()),
            comments: vec![PlanComment {
                id: 0,
                line_range: 1..2,
                text: "hello".into(),
            }],
            next_comment_id: 1,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: ExitPlanModeExtRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.comments.len(), 1);
        assert_eq!(back.comments[0].line_range, 1..2);
        assert_eq!(back.next_comment_id, 1);
    }

    #[test]
    fn format_plan_review_quotes_plan_lines() {
        let comments = vec![PlanComment {
            id: 0,
            line_range: 2..3,
            text: "use middleware".into(),
        }];
        let out = format_plan_review(&comments, Some("# Plan\nFix auth\n"), None);
        assert!(out.contains("Proposed plan line 2:"));
        assert!(out.contains("> Fix auth"));
        assert!(out.contains("use middleware"));
    }

    #[test]
    fn format_plan_review_feedback_only_is_passthrough() {
        let out = format_plan_review(&[], None, Some("rewrite auth"));
        assert_eq!(out, "rewrite auth");
    }
}
