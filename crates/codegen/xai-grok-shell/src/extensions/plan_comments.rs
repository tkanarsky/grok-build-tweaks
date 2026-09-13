//! `x.ai/plan_comments` — persist or read the open plan-review chip set.

use agent_client_protocol as acp;
use tokio::sync::oneshot;
use xai_grok_tools::implementations::grok_build::exit_plan_mode::PlanCommentSet;

use super::{ExtResult, parse_params, to_raw_response};
use crate::agent::MvpAgent;
use crate::session::{PlanReviewCommit, SessionCommand};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanCommentsRequest {
    session_id: String,
    #[serde(default)]
    comments: Option<Vec<xai_grok_tools::implementations::grok_build::exit_plan_mode::PlanComment>>,
    #[serde(default)]
    next_comment_id: u64,
    #[serde(default)]
    commit_outcome: Option<String>,
    #[serde(default)]
    plan_content: Option<String>,
    #[serde(default)]
    tool_call_id: Option<String>,
    #[serde(default)]
    feedback: Option<String>,
}

/// Handle `x.ai/plan_comments`. Presence of `comments` is a full-set replace; omit it to get.
pub async fn handle(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    let req: PlanCommentsRequest = parse_params(args)?;
    let sid: acp::SessionId = req.session_id.clone().into();
    let session_handle = agent.session_handle_waiting_for_load(&sid).await;
    let Some(session) = session_handle else {
        return Err(
            acp::Error::invalid_params().data(format!("session not found: {}", req.session_id))
        );
    };

    let set = req.comments.map(|comments| PlanCommentSet {
        comments,
        next_comment_id: req.next_comment_id,
    });
    let commit = req.commit_outcome.map(|outcome| PlanReviewCommit {
        outcome,
        tool_call_id: req.tool_call_id.unwrap_or_default(),
        plan_content: req.plan_content.unwrap_or_default(),
        feedback: req.feedback,
    });
    let (tx, rx) = oneshot::channel();
    if session
        .cmd_tx
        .send(SessionCommand::PlanComments {
            set,
            commit,
            respond_to: tx,
        })
        .is_err()
    {
        return Err(acp::Error::internal_error().data("session closed"));
    }
    let stored = rx
        .await
        .map_err(|_| acp::Error::internal_error().data("session dropped plan-comments reply"))?;
    to_raw_response(&stored)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omit_comments_is_a_get() {
        let req: PlanCommentsRequest = serde_json::from_value(serde_json::json!({
            "sessionId": "s1",
        }))
        .unwrap();
        assert!(req.comments.is_none());
        assert_eq!(req.next_comment_id, 0);
    }

    #[test]
    fn comments_array_is_a_set() {
        let req: PlanCommentsRequest = serde_json::from_value(serde_json::json!({
            "sessionId": "s1",
            "comments": [{ "id": 1, "lineRange": { "start": 2, "end": 3 }, "text": "hi" }],
            "nextCommentId": 2,
        }))
        .unwrap();
        assert_eq!(req.comments.as_ref().unwrap().len(), 1);
        assert_eq!(req.next_comment_id, 2);
    }
}
