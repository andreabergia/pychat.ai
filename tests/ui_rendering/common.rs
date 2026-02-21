use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use pychat_ai::cli::test_support::{UiHarness, deterministic_app_state};
use ratatui::layout::Rect;

pub fn new_harness(session_id: &str, width: u16, height: u16) -> Result<UiHarness> {
    let state = deterministic_app_state(session_id)?;
    let mut harness = UiHarness::new(width, height, state)?;
    harness.render()?;
    Ok(harness)
}

pub async fn type_text(harness: &mut UiHarness, text: &str) -> Result<()> {
    for ch in text.chars() {
        harness
            .send_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
            .await?;
    }
    Ok(())
}

pub async fn submit_line(harness: &mut UiHarness, line: &str) -> Result<()> {
    type_text(harness, line).await?;
    press_enter(harness).await
}

pub async fn press_tab(harness: &mut UiHarness) -> Result<()> {
    harness
        .send_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .await
}

pub async fn press_enter(harness: &mut UiHarness) -> Result<()> {
    harness
        .send_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
}

pub async fn press_ctrl_t(harness: &mut UiHarness) -> Result<()> {
    harness
        .send_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL))
        .await
}

pub async fn press_ctrl_j(harness: &mut UiHarness) -> Result<()> {
    harness
        .send_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL))
        .await
}

pub fn scroll_up(harness: &mut UiHarness, column: u16, row: u16) -> Result<()> {
    harness.send_mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

pub fn scroll_down(harness: &mut UiHarness, column: u16, row: u16) -> Result<()> {
    harness.send_mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

pub fn normalized_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn region_text(harness: &UiHarness, area: Rect) -> String {
    let lines = harness.buffer_lines();
    let start_row = usize::from(area.y);
    let end_row = start_row.saturating_add(usize::from(area.height));

    let mut rendered = Vec::new();
    for line in lines.iter().take(end_row.min(lines.len())).skip(start_row) {
        let clipped = line
            .chars()
            .skip(usize::from(area.x))
            .take(usize::from(area.width))
            .collect::<String>();
        rendered.push(clipped);
    }

    normalized_text(&rendered.join("\n"))
}

pub fn timeline_snapshot(harness: &UiHarness) -> Result<String> {
    let regions = harness.regions()?;
    Ok(region_text(harness, regions.timeline))
}

pub fn input_snapshot(harness: &UiHarness) -> Result<String> {
    let regions = harness.regions()?;
    Ok(region_text(harness, regions.input))
}

pub fn status_snapshot(harness: &UiHarness) -> Result<String> {
    let regions = harness.regions()?;
    Ok(region_text(harness, regions.status))
}
