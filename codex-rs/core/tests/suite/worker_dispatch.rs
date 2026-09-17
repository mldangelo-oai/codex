use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_features::Feature;
use codex_protocol::items::CollabAgentTool;
use codex_protocol::items::CollabAgentToolCallItem;
use codex_protocol::items::CollabAgentToolCallStatus;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call_with_namespace;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test]
async fn spawn_failures_emit_one_runtime_outcome() -> Result<()> {
    for (namespace, arguments, expected_error) in [
        (
            "collaboration",
            json!({"task_name": "worker", "message": "synthetic task"}),
            "agent thread limit reached",
        ),
        (
            "multi_agent_v1",
            json!({"message": "synthetic task", "agent_type": "worker", "fork_context": true}),
            "Full-history forked agents inherit the parent agent type",
        ),
        (
            "multi_agent_v1",
            json!({}),
            "Provide one of: message or items",
        ),
    ] {
        let server = start_mock_server().await;
        let requests = mount_sse_sequence(
            &server,
            vec![
                sse(vec![
                    ev_function_call_with_namespace(
                        "failed-spawn",
                        namespace,
                        "spawn_agent",
                        &arguments.to_string(),
                    ),
                    ev_completed("response-1"),
                ]),
                sse(vec![
                    ev_assistant_message("message-1", "done"),
                    ev_completed("response-2"),
                ]),
            ],
        )
        .await;
        let test = test_codex()
            .with_model("koffing")
            .with_config(move |config| {
                config
                    .features
                    .enable(Feature::Collab)
                    .expect("enable collaboration");
                if namespace == "collaboration" {
                    config
                        .features
                        .enable(Feature::MultiAgentV2)
                        .expect("enable native v2");
                } else {
                    config
                        .features
                        .disable(Feature::MultiAgentV2)
                        .expect("disable native v2");
                }
                // The parent occupies the only available execution slot.
                config.multi_agent_v2.max_concurrent_threads_per_session = 1;
            })
            .build_with_auto_env(&server)
            .await?;
        test.codex
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "Try to delegate a task".to_string(),
                text_elements: Vec::new(),
            }]))
            .await?;
        let mut outcomes = Vec::new();
        loop {
            match wait_for_event(&test.codex, |event| {
                matches!(
                    event,
                    EventMsg::ItemCompleted(_) | EventMsg::TurnComplete(_)
                )
            })
            .await
            {
                EventMsg::ItemCompleted(completed) if matches!(&completed.item, TurnItem::CollabAgentToolCall(item) if item.id == "failed-spawn") =>
                {
                    outcomes.push(completed);
                }
                EventMsg::TurnComplete(_) => break,
                _ => {}
            }
        }
        assert_eq!(outcomes.len(), 1);
        let outcome = &outcomes[0];
        assert_eq!(
            serde_json::to_value(&outcome.item)?,
            serde_json::to_value(TurnItem::CollabAgentToolCall(CollabAgentToolCallItem {
                id: "failed-spawn".to_string(),
                tool: CollabAgentTool::SpawnAgent,
                status: CollabAgentToolCallStatus::Failed,
                sender_thread_id: test.session_configured.thread_id,
                receiver_thread_ids: Vec::new(),
                receiver_agents: Vec::new(),
                prompt: None,
                model: None,
                reasoning_effort: None,
                agents_states: Default::default(),
            }))?
        );
        let requests = requests.requests();
        assert_eq!(requests.len(), 2);
        let (output, _) = requests[1]
            .function_call_output_content_and_success("failed-spawn")
            .expect("spawn tool output");
        assert!(
            output
                .as_deref()
                .is_some_and(|text| text.contains(expected_error))
        );
    }
    Ok(())
}
