use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::tempdir;

#[test]
fn failed_turn_does_not_overwrite_output_last_message_file() {
    let tempdir = tempdir().expect("create tempdir");
    let output_path = tempdir.path().join("last-message.txt");
    std::fs::write(&output_path, "keep existing contents").expect("seed output file");

    let mut processor = EventProcessorWithJsonOutput::new(Some(output_path.clone()));

    let collected = processor.collect_thread_events(ServerNotification::ItemCompleted(
        codex_app_server_protocol::ItemCompletedNotification {
            item: ThreadItem::AgentMessage {
                id: "msg-1".to_string(),
                text: "partial answer".to_string(),
                phase: None,
                memory_citation: None,
                delivery: None,
                questions: None,
            },
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            completed_at_ms: 0,
        },
    ));

    assert_eq!(collected.status, CodexStatus::Running);
    assert_eq!(processor.final_message(), Some("partial answer"));

    let status = processor.process_server_notification(ServerNotification::TurnCompleted(
        codex_app_server_protocol::TurnCompletedNotification {
            thread_id: "thread-1".to_string(),
            turn: codex_app_server_protocol::Turn {
                id: "turn-1".to_string(),
                items_view: codex_app_server_protocol::TurnItemsView::Full,
                items: Vec::new(),
                status: TurnStatus::Failed,
                error: Some(codex_app_server_protocol::TurnError {
                    misalignment: None,
                    message: "turn failed".to_string(),
                    additional_details: None,
                    codex_error_info: None,
                }),
                started_at: None,
                completed_at: Some(0),
                duration_ms: None,
            },
        },
    ));

    assert_eq!(status, CodexStatus::InitiateShutdown);
    assert_eq!(processor.final_message(), None);

    EventProcessor::print_final_output(&mut processor);

    assert_eq!(
        std::fs::read_to_string(&output_path).expect("read output file"),
        "keep existing contents"
    );
}

#[test]
fn runtime_warning_emits_a_non_fatal_error_item() {
    let mut processor = EventProcessorWithJsonOutput::new(/*last_message_path*/ None);

    let collected = processor.collect_thread_events(ServerNotification::Warning(
        codex_app_server_protocol::WarningNotification {
            thread_id: Some("thread-1".to_string()),
            message: "invalid global instructions".to_string(),
        },
    ));

    assert_eq!(
        collected,
        CollectedThreadEvents {
            events: vec![ThreadEvent::ItemCompleted(ItemCompletedEvent {
                item: ExecThreadItem {
                    id: "item_0".to_string(),
                    details: ThreadItemDetails::Error(ErrorItem {
                        message: "invalid global instructions".to_string(),
                    }),
                },
            })],
            status: CodexStatus::Running,
        }
    );
}

#[test]
fn mcp_tool_call_result_preserves_meta_in_jsonl_event() {
    let mut processor = EventProcessorWithJsonOutput::new(/*last_message_path*/ None);

    let collected = processor.collect_thread_events(ServerNotification::ItemCompleted(
        codex_app_server_protocol::ItemCompletedNotification {
            item: ThreadItem::McpToolCall {
                id: "mcp-1".to_string(),
                server: "search service".to_string(),
                tool: "web_run".to_string(),
                status: McpToolCallStatus::Completed,
                arguments: json!({"search_query": [{"q": "OpenAI Codex CLI documentation"}]}),
                app_context: None,
                mcp_app_resource_uri: None,
                mcp_app_ui: None,
                plugin_id: None,
                read_only_hint: None,
                result: Some(Box::new(codex_app_server_protocol::McpToolCallResult {
                    content: vec![json!({"type": "text", "text": "search result"})],
                    structured_content: None,
                    meta: Some(json!({"raw_messages": [{"ref_id": "turn0search0"}]})),
                })),
                error: None,
                duration_ms: Some(42),
            },
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            completed_at_ms: 0,
        },
    ));

    assert_eq!(collected.status, CodexStatus::Running);
    assert_eq!(collected.events.len(), 1);

    let ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) = &collected.events[0] else {
        panic!("expected item.completed event");
    };
    let ThreadItemDetails::McpToolCall(item) = &item.details else {
        panic!("expected MCP tool call item");
    };
    let result = item.result.as_ref().expect("expected MCP tool result");
    assert_eq!(
        result.meta,
        Some(json!({"raw_messages": [{"ref_id": "turn0search0"}]}))
    );

    let serialized = serde_json::to_value(&collected.events[0]).expect("serialize event");
    assert_eq!(
        serialized["item"]["result"]["_meta"],
        json!({"raw_messages": [{"ref_id": "turn0search0"}]})
    );
    assert!(serialized["item"]["result"].get("meta").is_none());
}

#[test]
fn native_v2_spawn_reports_only_the_completed_runtime_outcome() {
    let mut processor = EventProcessorWithJsonOutput::new(/*last_message_path*/ None);
    let item = ThreadItem::SubAgentActivity {
        id: "spawn-1".to_string(),
        kind: SubAgentActivityKind::Started,
        agent_thread_id: "worker-1".to_string(),
        agent_path: "/root/sensitive-task-name".to_string(),
    };
    let started = processor.collect_thread_events(ServerNotification::ItemStarted(
        codex_app_server_protocol::ItemStartedNotification {
            item: item.clone(),
            thread_id: "parent-1".to_string(),
            turn_id: "turn-1".to_string(),
            started_at_ms: 1,
        },
    ));
    assert_eq!(started.events, Vec::new());
    let completed = processor.collect_thread_events(ServerNotification::ItemCompleted(
        codex_app_server_protocol::ItemCompletedNotification {
            item: item.clone(),
            thread_id: "parent-1".to_string(),
            turn_id: "turn-1".to_string(),
            completed_at_ms: 2,
        },
    ));
    assert_eq!(
        serde_json::to_value(completed.events).unwrap(),
        json!([{
            "type": "worker.spawned",
            "dispatch_id": "turn-1:spawn-1",
            "worker_thread_id": "worker-1",
        }]),
    );
    // A tool call ID may be reused by a later turn; it is a new dispatch.
    let next_turn = processor.collect_thread_events(ServerNotification::ItemCompleted(
        codex_app_server_protocol::ItemCompletedNotification {
            item,
            thread_id: "parent-1".to_string(),
            turn_id: "turn-2".to_string(),
            completed_at_ms: 3,
        },
    ));
    assert_eq!(
        serde_json::to_value(next_turn.events).unwrap(),
        json!([{
            "type": "worker.spawned", "dispatch_id": "turn-2:spawn-1", "worker_thread_id": "worker-1",
        }])
    );
}

#[test]
fn failed_spawn_reports_an_outcome_without_error_text() {
    let mut processor = EventProcessorWithJsonOutput::new(/*last_message_path*/ None);
    let completed = processor.collect_thread_events(ServerNotification::ItemCompleted(
        codex_app_server_protocol::ItemCompletedNotification {
            item: ThreadItem::CollabAgentToolCall {
                id: "spawn-1".to_string(),
                tool: CollabAgentTool::SpawnAgent,
                status: CollabAgentToolCallStatus::Failed,
                sender_thread_id: "parent-1".to_string(),
                receiver_thread_ids: Vec::new(),
                prompt: Some("sensitive task prompt".to_string()),
                model: None,
                reasoning_effort: None,
                agents_states: HashMap::new(),
            },
            thread_id: "parent-1".to_string(),
            turn_id: "turn-1".to_string(),
            completed_at_ms: 2,
        },
    ));
    assert_eq!(
        serde_json::to_value(&completed.events[0]).unwrap(),
        json!({"type": "worker.spawn_failed", "dispatch_id": "turn-1:spawn-1"}),
    );
    assert!(matches!(
        &completed.events[1],
        ThreadEvent::ItemCompleted(_)
    ));
}

#[test]
fn a_created_worker_is_spawned_even_if_its_initial_status_is_errored() {
    let mut processor = EventProcessorWithJsonOutput::new(/*last_message_path*/ None);
    let completed = processor.collect_thread_events(ServerNotification::ItemCompleted(
        codex_app_server_protocol::ItemCompletedNotification {
            item: ThreadItem::CollabAgentToolCall {
                id: "spawn-1".to_string(),
                tool: CollabAgentTool::SpawnAgent,
                status: CollabAgentToolCallStatus::Failed,
                sender_thread_id: "parent-1".to_string(),
                receiver_thread_ids: vec!["child-1".to_string()],
                prompt: None,
                model: None,
                reasoning_effort: None,
                agents_states: HashMap::from([(
                    "child-1".to_string(),
                    codex_app_server_protocol::CollabAgentState {
                        status: codex_app_server_protocol::CollabAgentStatus::Errored,
                        message: Some("synthetic private error".to_string()),
                    },
                )]),
            },
            thread_id: "parent-1".to_string(),
            turn_id: "turn-1".to_string(),
            completed_at_ms: 2,
        },
    ));
    assert_eq!(
        serde_json::to_value(&completed.events[0]).unwrap(),
        json!({
            "type": "worker.spawned", "dispatch_id": "turn-1:spawn-1", "worker_thread_id": "child-1",
        })
    );
}
