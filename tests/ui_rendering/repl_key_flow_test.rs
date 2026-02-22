use anyhow::Result;

use crate::ui_rendering::common::{
    new_harness, press_down, press_enter, press_tab, press_up, submit_line, timeline_snapshot,
    type_text,
};

#[tokio::test]
async fn enter_on_incomplete_python_input_inserts_newline_then_executes_when_complete() -> Result<()>
{
    let mut harness = new_harness("phase3-incomplete-enter", 100, 24)?;

    type_text(&mut harness, "x = (").await?;
    press_enter(&mut harness).await?;
    let view = harness.ui_state_view();
    assert_eq!(view.prompt, "py> ");
    assert_eq!(view.input, "x = (\n");

    type_text(&mut harness, "1 + 2)").await?;
    press_enter(&mut harness).await?;
    submit_line(&mut harness, "x").await?;
    harness.render()?;

    let timeline = timeline_snapshot(&harness)?;
    assert!(timeline.contains("x = ("));
    assert!(timeline.contains("1 + 2)"));
    assert!(timeline.contains("3"));

    Ok(())
}

#[tokio::test]
async fn up_down_history_navigation_works_across_python_and_assistant_modes() -> Result<()> {
    let mut harness = new_harness("phase3-history-nav", 100, 24)?;

    submit_line(&mut harness, "a = 1").await?;
    submit_line(&mut harness, "/help").await?;

    press_up(&mut harness).await?;
    assert_eq!(harness.ui_state_view().input, "/help");
    press_up(&mut harness).await?;
    assert_eq!(harness.ui_state_view().input, "a = 1");
    press_down(&mut harness).await?;
    assert_eq!(harness.ui_state_view().input, "/help");

    press_tab(&mut harness).await?;
    assert_eq!(harness.ui_state_view().prompt, "ai> ");

    submit_line(&mut harness, "what is a?").await?;

    press_up(&mut harness).await?;
    assert_eq!(harness.ui_state_view().input, "what is a?");
    press_up(&mut harness).await?;
    assert_eq!(harness.ui_state_view().input, "/help");
    press_down(&mut harness).await?;
    assert_eq!(harness.ui_state_view().input, "what is a?");
    press_down(&mut harness).await?;
    assert_eq!(harness.ui_state_view().input, "");

    Ok(())
}

#[tokio::test]
async fn python_failure_does_not_prevent_next_successful_submission() -> Result<()> {
    let mut harness = new_harness("phase3-python-recovery", 100, 24)?;

    submit_line(&mut harness, "1 / 0").await?;
    submit_line(&mut harness, "2 + 2").await?;
    harness.render()?;

    let timeline = timeline_snapshot(&harness)?;
    assert!(timeline.contains("ZeroDivisionError"));
    assert!(timeline.contains("2 + 2"));
    assert!(timeline.contains("4"));

    Ok(())
}
