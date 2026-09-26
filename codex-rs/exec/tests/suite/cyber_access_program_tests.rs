use core_test_support::responses;
use core_test_support::test_codex_exec::test_codex_exec;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cyber_access_program_applies_to_fork_turn_only() -> anyhow::Result<()> {
    core_test_support::skip_if_no_network!(Ok(()));
    let test = test_codex_exec();
    let server = responses::start_mock_server().await;
    let response_mock = responses::mount_sse_sequence(
        &server,
        (0..3)
            .map(|index| {
                responses::sse(vec![
                    responses::ev_response_created(&format!("response_{index}")),
                    responses::ev_assistant_message(&format!("message_{index}"), "OK"),
                    responses::ev_completed(&format!("response_{index}")),
                ])
            })
            .collect(),
    )
    .await;

    let output = test
        .cmd_with_server(&server)
        .args([
            "--skip-git-repo-check",
            "--json",
            "--cyber-access-program",
            "daybreak_blue",
            "Respond only OK.",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let events = String::from_utf8(output.stdout)?
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    let thread_id = events[0]["thread_id"]
        .as_str()
        .expect("fresh turn should emit its thread id");

    for program in [Some("standard"), None] {
        let mut cmd = test.cmd_with_server(&server);
        cmd.args(["--skip-git-repo-check", "fork", thread_id]);
        if let Some(program) = program {
            cmd.args(["--cyber-access-program", program]);
        }
        cmd.arg("Respond only OK.").assert().success();
    }

    assert_eq!(
        response_mock
            .requests()
            .iter()
            .map(|request| request.body_json().get("access_programs").cloned())
            .collect::<Vec<_>>(),
        vec![
            Some(json!({"cyber": "daybreak_blue"})),
            Some(json!({"cyber": "standard"})),
            None
        ]
    );
    Ok(())
}

#[test]
fn cyber_access_program_requires_a_supported_turn() {
    let test = test_codex_exec();
    for (args, message) in [
        (
            vec!["review", "--uncommitted"],
            "is not supported with `codex exec review`",
        ),
        (vec!["fork", "synthetic-session"], "requires a prompt"),
    ] {
        test.cmd()
            .args(["--cyber-access-program", "daybreak_blue"])
            .args(args)
            .assert()
            .failure()
            .stderr(predicates::str::contains(message));
    }
}
