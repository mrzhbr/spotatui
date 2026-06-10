use crate::core::app::{App, SettingValue, SettingsCategory};
use crate::tui::ui::util::selectable_list_scroll_offset;
use ratatui::{
  layout::{Alignment, Constraint, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs},
  Frame,
};
use std::borrow::Cow;

const UNSAVED_PROMPT_WIDTH: u16 = 58;
const UNSAVED_PROMPT_HEIGHT: u16 = 9;

pub fn draw_settings(f: &mut Frame<'_>, app: &App) {
  let [tabs_area, list_area, help_area] = f.area().layout(
    &Layout::vertical([
      Constraint::Length(3), // Category tabs
      Constraint::Min(1),    // Settings list
      Constraint::Length(4), // Help bar + terminal capability line
    ])
    .margin(2),
  );

  draw_category_tabs(f, app, tabs_area);
  draw_settings_list(f, app, list_area);
  draw_settings_help(f, app, help_area);

  if app.settings_unsaved_prompt_visible {
    draw_unsaved_changes_prompt(f, app);
  }
}

fn draw_category_tabs(f: &mut Frame<'_>, app: &App, area: Rect) {
  let selected = app.settings_category.index();
  let titles = SettingsCategory::all()
    .iter()
    .map(|cat| Line::from(cat.name()));

  let tabs = Tabs::new(titles)
    .select(selected)
    .block(
      Block::default()
        .borders(Borders::ALL)
        .title("Settings (←/→ to switch tabs)"),
    )
    .highlight_style(
      Style::default()
        .fg(app.user_config.theme.selected)
        .add_modifier(Modifier::BOLD),
    )
    .style(app.user_config.theme.base_style());

  f.render_widget(tabs, area);
}

fn draw_settings_list(f: &mut Frame<'_>, app: &App, area: Rect) {
  let visible_rows = area.height.saturating_sub(2) as usize;
  let offset = selectable_list_scroll_offset(app.settings_selected_index, visible_rows);
  let selected_visible_index = app.settings_selected_index.checked_sub(offset);

  let visible_items = app
    .settings_items
    .iter()
    .enumerate()
    .skip(offset)
    .take(visible_rows.saturating_add(1))
    .map(|(i, setting)| {
      let is_selected = i == app.settings_selected_index;
      let is_editing = is_selected && app.settings_edit_mode;

      // Format the value display
      let value_str: Cow<'_, str> = if is_editing {
        match &setting.value {
          SettingValue::Bool(v) => {
            // Show toggle state (shouldn't reach here with new logic, but just in case)
            Cow::Borrowed(if *v {
              "[●] On  [ ] Off"
            } else {
              "[ ] On  [●] Off"
            })
          }
          _ => {
            // Show edit buffer with cursor
            Cow::Owned(format!("{}▏", app.settings_edit_buffer))
          }
        }
      } else {
        match &setting.value {
          SettingValue::Bool(v) => {
            // Show toggle indicator - pressing Enter will toggle
            Cow::Borrowed(if *v { "[●] On" } else { "[○] Off" })
          }
          SettingValue::Number(v) => Cow::Owned(v.to_string()),
          SettingValue::String(v) => Cow::Owned(format!("\"{}\"", v)),
          SettingValue::Key(v) => {
            if setting.id == "keys.open_settings" {
              let effective = app.effective_open_settings_key();
              let configured = app.user_config.keys.open_settings;
              if effective != configured {
                Cow::Owned(format!("[{}] (effective: {})", v, effective))
              } else {
                Cow::Owned(format!("[{}]", v))
              }
            } else {
              Cow::Owned(format!("[{}]", v))
            }
          }
          SettingValue::Color(v) => Cow::Owned(format!("■ {}", v)),
          SettingValue::Preset(v) => Cow::Owned(format!("◆ {} ◆", v)), // Show preset name with arrows hint
          SettingValue::Cycle(v, _) => Cow::Owned(format!("◆ {} ◆", v)),
        }
      };

      // Build the line with name and value
      let name_style = if is_selected {
        Style::default()
          .fg(app.user_config.theme.selected)
          .add_modifier(Modifier::BOLD)
      } else {
        Style::default().fg(app.user_config.theme.text)
      };

      let value_style = if is_editing {
        Style::default()
          .fg(app.user_config.theme.hint)
          .add_modifier(Modifier::BOLD)
      } else if is_selected {
        Style::default().fg(app.user_config.theme.selected)
      } else {
        Style::default().fg(app.user_config.theme.inactive)
      };

      let line = Line::from(vec![
        Span::styled(setting.name.as_str(), name_style),
        Span::styled(": ", name_style),
        Span::styled(value_str, value_style),
      ]);

      ListItem::new(line)
    });

  let title = format!(
    "{} Settings ({} items)",
    app.settings_category.name(),
    app.settings_items.len()
  );

  let mut state = ListState::default();
  state.select(selected_visible_index);

  let list = List::new(visible_items)
    .block(
      Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(app.user_config.theme.base_style())
        .border_style(Style::default().fg(app.user_config.theme.inactive)),
    )
    .highlight_style(
      Style::default()
        .fg(app.user_config.theme.selected)
        .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol(
      Line::from("▶ ").style(
        Style::default()
          .fg(app.user_config.theme.selected)
          .add_modifier(Modifier::BOLD),
      ),
    );

  f.render_stateful_widget(list, area, &mut state);
}

fn append_terminal_input_caps(out: &mut String, app: &App) {
  let enhancement = if app.terminal_input_caps.keyboard_enhancement_enabled {
    "enh:on"
  } else if app.terminal_input_caps.keyboard_enhancement_supported {
    "enh:available"
  } else {
    "enh:off"
  };

  let ctrl_comma = match app.terminal_input_caps.ctrl_punct_reliable {
    crate::core::app::CapabilityState::Yes => "ctrl+,=ok",
    crate::core::app::CapabilityState::No => "ctrl+,=degraded",
    crate::core::app::CapabilityState::Unknown => "ctrl+,=unknown",
  };

  out.push_str("Terminal Input: ");
  out.push_str(enhancement);
  out.push_str(" | ");
  out.push_str(ctrl_comma);
}

fn draw_settings_help(f: &mut Frame<'_>, app: &App, area: Rect) {
  let controls_line: Cow<'_, str> = if app.settings_edit_mode {
    Cow::Borrowed(match app.settings_items.get(app.settings_selected_index) {
      Some(setting) => match &setting.value {
        SettingValue::Bool(_) => "Space/Enter: Toggle | ←/→: Toggle | Esc: Cancel",
        SettingValue::Number(_) => {
          "↑/↓: Increment/Decrement | Type numbers | Enter: Confirm | Esc: Cancel"
        }
        SettingValue::Key(_) => "Press any key to set binding | Esc: Cancel",
        SettingValue::Preset(_) | SettingValue::Cycle(_, _) => {
          "Enter/→: Next | ←: Previous | Esc: Cancel"
        }
        _ => "Type to edit | Enter: Confirm | Esc: Cancel",
      },
      None => "",
    })
  } else {
    Cow::Owned(format!(
      "↑/↓: Select | ←/→: Switch Tab | Enter: Toggle/Edit | Mouse: Click/Scroll | {}: Save | Esc/q: Exit",
      app.effective_save_settings_key()
    ))
  };
  let mut help_text = controls_line.into_owned();
  help_text.push('\n');
  append_terminal_input_caps(&mut help_text, app);

  let help = Paragraph::new(help_text)
    .style(
      Style::default()
        .fg(app.user_config.theme.hint)
        .bg(app.user_config.theme.background),
    )
    .block(
      Block::default()
        .borders(Borders::ALL)
        .title("Controls")
        .style(app.user_config.theme.base_style())
        .border_style(Style::default().fg(app.user_config.theme.inactive)),
    );

  f.render_widget(help, area);
}

fn draw_unsaved_changes_prompt(f: &mut Frame<'_>, app: &App) {
  let bounds = f.area();
  let width = std::cmp::min(bounds.width.saturating_sub(4), UNSAVED_PROMPT_WIDTH);
  if width == 0 {
    return;
  }

  let height = UNSAVED_PROMPT_HEIGHT.min(bounds.height.saturating_sub(2).max(1));
  let left = bounds.x + bounds.width.saturating_sub(width) / 2;
  let top = bounds.y + bounds.height.saturating_sub(height) / 2;
  let rect = Rect::new(left, top, width, height);

  f.render_widget(Clear, rect);

  let block = Block::default()
    .title(" Unsaved Settings ")
    .borders(Borders::ALL)
    .style(app.user_config.theme.base_style())
    .border_style(Style::default().fg(app.user_config.theme.active));
  f.render_widget(block, rect);

  let [message_area, buttons_area, hint_area] = rect.layout(
    &Layout::vertical([
      Constraint::Min(2),
      Constraint::Length(3),
      Constraint::Length(1),
    ])
    .margin(1),
  );

  let message = Paragraph::new("You have unsaved changes. Save before leaving settings?")
    .style(app.user_config.theme.base_style())
    .alignment(Alignment::Center);
  f.render_widget(message, message_area);

  let [yes_area, no_area] = buttons_area.layout(
    &Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]).horizontal_margin(3),
  );

  let yes_selected = app.settings_unsaved_prompt_save_selected;
  let yes = Paragraph::new("[ Yes ]")
    .alignment(Alignment::Center)
    .style(Style::default().fg(if yes_selected {
      app.user_config.theme.hovered
    } else {
      app.user_config.theme.inactive
    }));
  f.render_widget(yes, yes_area);

  let no = Paragraph::new("[ No ]")
    .alignment(Alignment::Center)
    .style(Style::default().fg(if yes_selected {
      app.user_config.theme.inactive
    } else {
      app.user_config.theme.hovered
    }));
  f.render_widget(no, no_area);

  let hint = Paragraph::new("Y: Yes | N: No | Enter: Select | Esc: Cancel")
    .alignment(Alignment::Center)
    .style(Style::default().fg(app.user_config.theme.inactive));
  f.render_widget(hint, hint_area);
}
