use crate::config::{
    HexColor, StyleOverride, ThemeConfig as UserThemeConfig, ThemeModifier, ThemePreset, ThemeToken,
};
use ratatui::style::{Color, Modifier, Style};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Theme {
    enabled: bool,
    styles: HashMap<ThemeToken, Style>,
}

impl Theme {
    #[cfg(test)]
    pub fn new(enabled: bool) -> Self {
        Self::from_config(enabled, &UserThemeConfig::default())
    }

    pub fn from_config(enabled: bool, config: &UserThemeConfig) -> Self {
        let mut styles = preset_styles(config.preset);
        for (token, override_style) in &config.styles {
            let base = styles.get(token).copied().unwrap_or_default();
            styles.insert(*token, merge_style(base, override_style));
        }

        Self { enabled, styles }
    }

    pub fn style(&self, token: ThemeToken) -> Style {
        if !self.enabled {
            return disabled_style(token);
        }

        self.styles.get(&token).copied().unwrap_or_default()
    }
}

fn preset_styles(preset: ThemePreset) -> HashMap<ThemeToken, Style> {
    ThemeToken::all()
        .iter()
        .copied()
        .map(|token| (token, preset_style(preset, token)))
        .collect()
}

fn preset_style(preset: ThemePreset, token: ThemeToken) -> Style {
    match preset {
        ThemePreset::Default => default_preset_style(token),
        ThemePreset::Light => light_preset_style(token),
        ThemePreset::HighContrast => high_contrast_preset_style(token),
    }
}

fn default_preset_style(token: ThemeToken) -> Style {
    match token {
        ThemeToken::PythonPrompt => Style::default()
            .fg(Color::Rgb(158, 206, 106))
            .add_modifier(Modifier::BOLD),
        ThemeToken::AssistantPrompt => Style::default()
            .fg(Color::Rgb(219, 75, 75))
            .add_modifier(Modifier::BOLD),
        ThemeToken::UserInputPython | ThemeToken::UserInputAssistant => {
            Style::default().fg(Color::White)
        }
        ThemeToken::PythonValue => Style::default().fg(Color::Rgb(158, 206, 106)),
        ThemeToken::PythonStdout => Style::default().fg(Color::Rgb(192, 202, 245)),
        ThemeToken::PythonStderr => Style::default().fg(Color::Rgb(255, 158, 100)),
        ThemeToken::PythonTraceback => Style::default()
            .fg(Color::Rgb(247, 118, 142))
            .add_modifier(Modifier::BOLD),
        ThemeToken::AssistantText => Style::default().fg(Color::Rgb(219, 75, 75)),
        ThemeToken::AssistantWaiting => Style::default()
            .fg(Color::Rgb(206, 120, 120))
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ThemeToken::AssistantProgressRequest => Style::default()
            .fg(Color::Rgb(138, 138, 138))
            .add_modifier(Modifier::ITALIC),
        ThemeToken::AssistantProgressResult => Style::default().fg(Color::Rgb(138, 138, 138)),
        ThemeToken::SystemInfo | ThemeToken::Status => Style::default().fg(Color::Rgb(86, 95, 137)),
        ThemeToken::SystemError => Style::default()
            .fg(Color::Rgb(247, 118, 142))
            .add_modifier(Modifier::BOLD),
        ThemeToken::InputBlock => Style::default().bg(Color::Rgb(22, 22, 30)).fg(Color::White),
    }
}

fn light_preset_style(token: ThemeToken) -> Style {
    match token {
        ThemeToken::PythonPrompt => Style::default()
            .fg(Color::Rgb(31, 111, 235))
            .add_modifier(Modifier::BOLD),
        ThemeToken::AssistantPrompt => Style::default()
            .fg(Color::Rgb(176, 64, 0))
            .add_modifier(Modifier::BOLD),
        ThemeToken::UserInputPython | ThemeToken::UserInputAssistant => {
            Style::default().fg(Color::Rgb(36, 41, 47))
        }
        ThemeToken::PythonValue => Style::default().fg(Color::Rgb(5, 80, 40)),
        ThemeToken::PythonStdout => Style::default().fg(Color::Rgb(9, 105, 218)),
        ThemeToken::PythonStderr => Style::default().fg(Color::Rgb(188, 76, 0)),
        ThemeToken::PythonTraceback => Style::default()
            .fg(Color::Rgb(176, 0, 32))
            .add_modifier(Modifier::BOLD),
        ThemeToken::AssistantText => Style::default().fg(Color::Rgb(130, 70, 0)),
        ThemeToken::AssistantWaiting => Style::default()
            .fg(Color::Rgb(130, 70, 0))
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ThemeToken::AssistantProgressRequest => Style::default()
            .fg(Color::Rgb(80, 90, 110))
            .add_modifier(Modifier::ITALIC),
        ThemeToken::AssistantProgressResult => Style::default().fg(Color::Rgb(80, 90, 110)),
        ThemeToken::SystemInfo | ThemeToken::Status => Style::default().fg(Color::Rgb(36, 70, 120)),
        ThemeToken::SystemError => Style::default()
            .fg(Color::Rgb(176, 0, 32))
            .add_modifier(Modifier::BOLD),
        ThemeToken::InputBlock => Style::default()
            .bg(Color::Rgb(246, 248, 250))
            .fg(Color::Rgb(36, 41, 47)),
    }
}

fn high_contrast_preset_style(token: ThemeToken) -> Style {
    match token {
        ThemeToken::PythonPrompt => Style::default()
            .fg(Color::Rgb(0, 255, 127))
            .add_modifier(Modifier::BOLD),
        ThemeToken::AssistantPrompt => Style::default()
            .fg(Color::Rgb(255, 215, 0))
            .add_modifier(Modifier::BOLD),
        ThemeToken::UserInputPython | ThemeToken::UserInputAssistant => {
            Style::default().fg(Color::Rgb(255, 255, 255))
        }
        ThemeToken::PythonValue => Style::default().fg(Color::Rgb(0, 255, 127)),
        ThemeToken::PythonStdout => Style::default().fg(Color::Rgb(135, 206, 250)),
        ThemeToken::PythonStderr => Style::default().fg(Color::Rgb(255, 140, 0)),
        ThemeToken::PythonTraceback => Style::default()
            .fg(Color::Rgb(255, 64, 64))
            .add_modifier(Modifier::BOLD),
        ThemeToken::AssistantText => Style::default().fg(Color::Rgb(255, 215, 0)),
        ThemeToken::AssistantWaiting => Style::default()
            .fg(Color::Rgb(255, 255, 0))
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ThemeToken::AssistantProgressRequest => Style::default()
            .fg(Color::Rgb(220, 220, 220))
            .add_modifier(Modifier::ITALIC),
        ThemeToken::AssistantProgressResult => Style::default().fg(Color::Rgb(220, 220, 220)),
        ThemeToken::SystemInfo | ThemeToken::Status => {
            Style::default().fg(Color::Rgb(173, 216, 230))
        }
        ThemeToken::SystemError => Style::default()
            .fg(Color::Rgb(255, 64, 64))
            .add_modifier(Modifier::BOLD),
        ThemeToken::InputBlock => Style::default()
            .bg(Color::Rgb(0, 0, 0))
            .fg(Color::Rgb(255, 255, 255)),
    }
}

fn disabled_style(token: ThemeToken) -> Style {
    match token {
        ThemeToken::PythonPrompt | ThemeToken::AssistantPrompt => {
            Style::default().add_modifier(Modifier::BOLD)
        }
        _ => Style::default(),
    }
}

fn merge_style(base: Style, override_style: &StyleOverride) -> Style {
    let mut merged = base;

    if let Some(fg) = override_style.fg {
        merged = merged.fg(color_from_hex(fg));
    }

    if let Some(bg) = override_style.bg {
        merged = merged.bg(color_from_hex(bg));
    }

    if let Some(modifiers) = &override_style.modifiers {
        merged = merged
            .remove_modifier(Modifier::all())
            .add_modifier(modifiers_to_modifier(modifiers));
    }

    merged
}

fn color_from_hex(color: HexColor) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

fn modifiers_to_modifier(modifiers: &[ThemeModifier]) -> Modifier {
    modifiers
        .iter()
        .copied()
        .fold(Modifier::empty(), |acc, modifier| {
            acc | modifier_to_ratatui(modifier)
        })
}

fn modifier_to_ratatui(modifier: ThemeModifier) -> Modifier {
    match modifier {
        ThemeModifier::Bold => Modifier::BOLD,
        ThemeModifier::Dim => Modifier::DIM,
        ThemeModifier::Italic => Modifier::ITALIC,
        ThemeModifier::Underlined => Modifier::UNDERLINED,
        ThemeModifier::SlowBlink => Modifier::SLOW_BLINK,
        ThemeModifier::RapidBlink => Modifier::RAPID_BLINK,
        ThemeModifier::Reversed => Modifier::REVERSED,
        ThemeModifier::Hidden => Modifier::HIDDEN,
        ThemeModifier::CrossedOut => Modifier::CROSSED_OUT,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::Theme;
    use crate::config::{HexColor, StyleOverride, ThemeConfig, ThemePreset, ThemeToken};

    #[test]
    fn theme_new_matches_default_preset() {
        let theme = Theme::new(true);
        assert_eq!(
            theme.style(ThemeToken::PythonPrompt),
            Theme::from_config(true, &ThemeConfig::default()).style(ThemeToken::PythonPrompt)
        );
    }

    #[test]
    fn disabled_theme_only_keeps_prompt_bold() {
        let theme = Theme::new(false);
        assert!(
            theme
                .style(ThemeToken::PythonPrompt)
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD)
        );
        assert_eq!(
            theme.style(ThemeToken::SystemInfo),
            ratatui::style::Style::default()
        );
    }

    #[test]
    fn partial_override_preserves_unset_fields() {
        let mut config = ThemeConfig {
            preset: ThemePreset::Default,
            styles: HashMap::new(),
        };
        config.styles.insert(
            ThemeToken::PythonPrompt,
            StyleOverride {
                fg: Some(HexColor { r: 1, g: 2, b: 3 }),
                bg: None,
                modifiers: None,
            },
        );

        let theme = Theme::from_config(true, &config);
        let style = theme.style(ThemeToken::PythonPrompt);
        assert_eq!(style.fg, Some(ratatui::style::Color::Rgb(1, 2, 3)));
        assert_eq!(
            style.add_modifier,
            ratatui::style::Modifier::BOLD,
            "preset bold should be preserved"
        );
    }
}
