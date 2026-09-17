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
use core_test_support::wait_for_event_match;
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test]
async fn native_v2_spawn_at_capacity_emits_a_failed_runtime_outcome() -> Result<()> {
    let server = start_mock_server().await;
    let requests = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_function_call_with_namespace(
                    "spawn-at-capacity",
                    "collaboration",
                    "spawn_agent",
                    &json!({"task_name": "worker", "message": "synthetic task"}).to_string(),
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
        .with_config(|config| {
            config
                .features
                .enable(Feature::Collab)
                .expect("enable collaboration");
            config
                .features
                .enable(Feature::MultiAgentV2)
                .expect("enable native v2");
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
    let outcome = wait_for_event_match(&test.codex, |event| match event {
        EventMsg::ItemCompleted(completed)
            if matches!(
                &completed.item,
                TurnItem::CollabAgentToolCall(item) if item.id == "spawn-at-capacity"
            ) =>
        {
            Some(completed.clone())
        }
        _ => None,
    })
    .await;
    assert_eq!(
        serde_json::to_value(&outcome.item)?,
        serde_json::to_value(TurnItem::CollabAgentToolCall(CollabAgentToolCallItem {
            id: "spawn-at-capacity".to_string(),
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
    assert!(
        outcome
            .started_at_ms
            .is_some_and(|start| start <= outcome.completed_at_ms)
    );
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    let requests = requests.requests();
    assert_eq!(requests.len(), 2);
    let (output, _) = requests[1]
        .function_call_output_content_and_success("spawn-at-capacity")
        .expect("spawn tool output");
    assert!(
        output
            .as_deref()
            .is_some_and(|text| text.contains("agent thread limit reached"))
    );
    Ok(())
}
