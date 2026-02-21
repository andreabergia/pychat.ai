use anyhow::Result;

use crate::ui_rendering::common::{new_harness, scroll_down, scroll_up};

#[tokio::test]
async fn mouse_wheel_scroll_only_applies_inside_timeline_region() -> Result<()> {
    let mut harness = new_harness("phase3-mouse-area", 100, 24)?;

    for i in 0..24 {
        harness.seed_assistant_turn_completed(
            &format!("inspect item_{i}"),
            &[("request", "-> Inspecting"), ("result", "<- Done")],
            "ok",
        )?;
    }
    harness.render()?;

    let regions = harness.regions()?;

    assert_eq!(harness.ui_state_view().timeline_scroll, 0);

    scroll_up(&mut harness, regions.input.x + 1, regions.input.y + 1)?;
    assert_eq!(harness.ui_state_view().timeline_scroll, 0);

    scroll_up(&mut harness, regions.timeline.x + 1, regions.timeline.y + 1)?;
    let after_timeline_up = harness.ui_state_view().timeline_scroll;
    assert!(after_timeline_up > 0);

    scroll_down(&mut harness, regions.status.x + 1, regions.status.y)?;
    assert_eq!(harness.ui_state_view().timeline_scroll, after_timeline_up);

    scroll_down(&mut harness, regions.timeline.x + 1, regions.timeline.y + 1)?;
    assert!(harness.ui_state_view().timeline_scroll < after_timeline_up);

    Ok(())
}

#[tokio::test]
async fn timeline_scroll_clamps_to_valid_bounds() -> Result<()> {
    let mut harness = new_harness("phase3-mouse-clamp", 100, 24)?;

    for i in 0..32 {
        harness.seed_assistant_turn_completed(
            &format!("inspect value_{i}"),
            &[("request", "-> Inspecting"), ("result", "<- Done")],
            "ok",
        )?;
    }
    harness.render()?;

    let regions = harness.regions()?;
    for _ in 0..200 {
        scroll_up(&mut harness, regions.timeline.x + 1, regions.timeline.y + 1)?;
    }

    let max_reached = harness.ui_state_view().timeline_scroll;
    assert!(max_reached > 0);

    scroll_up(&mut harness, regions.timeline.x + 1, regions.timeline.y + 1)?;
    assert_eq!(harness.ui_state_view().timeline_scroll, max_reached);

    for _ in 0..200 {
        scroll_down(&mut harness, regions.timeline.x + 1, regions.timeline.y + 1)?;
    }

    assert_eq!(harness.ui_state_view().timeline_scroll, 0);

    Ok(())
}
