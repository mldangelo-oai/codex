use clap::error::ErrorKind;
use pretty_assertions::assert_eq;

use super::*;

#[test]
fn exec_help_documents_cyber_access_program() {
    let help = MultitoolCli::command()
        .term_width(80)
        .try_get_matches_from(["codex", "exec", "--help"])
        .expect_err("help should exit before starting a turn");
    assert_eq!(help.kind(), ErrorKind::DisplayHelp);
    let help_text = help
        .to_string()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(help_text);
}
