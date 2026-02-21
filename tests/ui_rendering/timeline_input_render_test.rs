use anyhow::Result;

use crate::ui_rendering::common::{
    input_snapshot, new_harness, press_ctrl_j, press_ctrl_t, press_tab, status_snapshot,
    submit_line, timeline_snapshot, type_text,
};

#[tokio::test]
async fn initial_render_shows_welcome_and_status_with_session() -> Result<()> {
    let mut harness = new_harness("phase3-welcome", 100, 24)?;
    harness.render()?;

    let timeline = timeline_snapshot(&harness)?;
    assert!(timeline.contains("Welcome to PyChat.ai"));

    let status = status_snapshot(&harness)?;
    assert!(status.contains("Mode: Python"));
    assert!(status.contains("Show agent thinking: On (Ctrl-T)"));
    assert!(status.contains("PyChat.ai | Session: phase3-welcome"));

    Ok(())
}

#[tokio::test]
async fn prompt_changes_for_python_assistant_and_command_input() -> Result<()> {
    let mut harness = new_harness("phase3-prompt", 100, 24)?;

    assert_eq!(harness.ui_state_view().prompt, "py> ");

    press_tab(&mut harness).await?;
    assert_eq!(harness.ui_state_view().prompt, "ai> ");

    type_text(&mut harness, "/trace").await?;
    assert_eq!(harness.ui_state_view().prompt, "cmd> ");

    Ok(())
}

#[tokio::test]
async fn multiline_input_scroll_keeps_latest_lines_visible() -> Result<()> {
    let mut harness = new_harness("phase3-multiline", 100, 24)?;

    for line_no in 1..=8 {
        type_text(&mut harness, &format!("line-{line_no}")).await?;
        if line_no < 8 {
            press_ctrl_j(&mut harness).await?;
        }
    }

    harness.render()?;

    let view = harness.ui_state_view();
    assert!(view.input.contains("line-1\nline-2\nline-3"));
    assert!(view.input.ends_with("line-8"));

    let input = input_snapshot(&harness)?;
    assert!(!input.contains("line-1"));
    assert!(input.contains("line-8"));

    let regions = harness.regions()?;
    let last_content_row = regions.input.y + regions.input.height.saturating_sub(2);
    let last_row = harness.line(last_content_row).unwrap_or_default();
    assert!(last_row.contains("line-8"));

    Ok(())
}

#[tokio::test]
async fn assistant_thinking_block_toggle_is_retroactive() -> Result<()> {
    let mut harness = new_harness("phase3-thinking", 100, 24)?;

    harness.seed_assistant_turn_completed(
        "inspect value",
        &[
            ("request", "-> Inspecting: value"),
            ("result", "<- Inspection complete: int"),
        ],
        "value is 7",
    )?;
    harness.render()?;

    let shown = timeline_snapshot(&harness)?;
    assert!(shown.contains("  Thinking..."));
    assert!(shown.contains("  -> Inspecting: value"));
    assert!(shown.contains("  <- Inspection complete: int"));
    assert!(shown.contains("value is 7"));

    press_ctrl_t(&mut harness).await?;
    harness.render()?;

    let hidden = timeline_snapshot(&harness)?;
    assert!(!hidden.contains("  Thinking..."));
    assert!(!hidden.contains("  -> Inspecting: value"));
    assert!(!hidden.contains("  <- Inspection complete: int"));
    assert!(hidden.contains("value is 7"));

    Ok(())
}

#[tokio::test]
async fn scoped_snapshots_for_timeline_and_status() -> Result<()> {
    let mut harness = new_harness("phase3-snapshot", 100, 24)?;

    insta::assert_snapshot!("ui_timeline_baseline", timeline_snapshot(&harness)?);
    insta::assert_snapshot!("ui_status_python", status_snapshot(&harness)?);

    submit_line(&mut harness, "x = 41").await?;
    submit_line(&mut harness, "x + 1").await?;
    harness.seed_assistant_turn_completed(
        "summarize x",
        &[
            ("request", "-> Inspecting: x"),
            ("result", "<- Inspection complete: int"),
        ],
        "x is 42",
    )?;
    harness.render()?;

    insta::assert_snapshot!("ui_timeline_mixed", timeline_snapshot(&harness)?);

    press_tab(&mut harness).await?;
    harness.render()?;
    insta::assert_snapshot!("ui_status_assistant", status_snapshot(&harness)?);

    Ok(())
}
