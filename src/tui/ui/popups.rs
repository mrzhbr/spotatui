use crate::core::app::{ActiveBlock, App, DialogContext};
use ratatui::{
  layout::{Alignment, Constraint, Direction, Layout, Rect},
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Row, Table, Wrap},
  Frame,
};
use rspotify::model::PlayableItem;
use rspotify::prelude::Id;

use super::help::{get_help_docs, HelpDocRow};
use super::util::{append_artist_string, selectable_list_scroll_offset};

pub fn draw_help_menu(f: &mut Frame<'_>, app: &App) {
  let [area] = f
    .area()
    .layout(&Layout::vertical([Constraint::Percentage(100)]).margin(2));

  // Create a one-column table to avoid flickering due to non-determinism when
  // resolving constraints on widths of table columns.
  // Calculate column widths based on available terminal width
  let total_width = area.width as usize;
  let col1_width = (total_width as f32 * 0.40) as usize;
  let col2_width = (total_width as f32 * 0.30) as usize;
  let col3_width = total_width.saturating_sub(col1_width + col2_width + 2);

  let format_row = |r: &HelpDocRow| -> [String; 1] {
    [format!(
      "{:<w1$}  {:<w2$}  {:<w3$}",
      truncate_help_cell(r[0].as_ref(), col1_width),
      truncate_help_cell(r[1].as_ref(), col2_width),
      truncate_help_cell(r[2].as_ref(), col3_width),
      w1 = col1_width,
      w2 = col2_width,
      w3 = col3_width,
    )]
  };

  let help_menu_style = app.user_config.theme.base_style();
  let header = [
    std::borrow::Cow::Borrowed("Description"),
    std::borrow::Cow::Borrowed("Event"),
    std::borrow::Cow::Borrowed("Context"),
  ];
  let header = format_row(&header);

  let visible_rows = area.height.saturating_sub(3) as usize;
  let rows = get_help_docs(app)
    .into_iter()
    .skip(app.help_menu_offset as usize)
    .take(visible_rows.saturating_add(1))
    .map(|row| format_row(&row))
    .map(|item| Row::new(item).style(help_menu_style));

  let help_menu = Table::new(rows, &[Constraint::Percentage(100)])
    .header(Row::new(header))
    .block(
      Block::default()
        .borders(Borders::ALL)
        .style(help_menu_style)
        .title(Span::styled(
          "Help (press <Esc> to go back)",
          help_menu_style,
        ))
        .border_style(help_menu_style),
    )
    .style(help_menu_style);
  f.render_widget(help_menu, area);
}

fn truncate_help_cell(s: &str, max_chars: usize) -> String {
  if max_chars == 0 {
    return String::new();
  }

  let mut truncate_at = 0;
  for (char_index, (byte_index, _)) in s.char_indices().enumerate() {
    if char_index == max_chars.saturating_sub(1) {
      truncate_at = byte_index;
    }
    if char_index == max_chars {
      let mut truncated = String::with_capacity(truncate_at + "…".len());
      truncated.push_str(&s[..truncate_at]);
      truncated.push('…');
      return truncated;
    }
  }

  s.to_string()
}

fn queue_item_line(item: &PlayableItem) -> String {
  match item {
    PlayableItem::Track(track) => {
      let mut label = String::with_capacity(track.name.len() + 3 + track.artists.len() * 16);
      label.push_str(&track.name);
      label.push_str(" - ");
      append_artist_string(&mut label, &track.artists);
      label
    }
    PlayableItem::Episode(episode) => {
      let mut label = String::with_capacity(episode.name.len() + 3 + episode.show.name.len());
      label.push_str(&episode.name);
      label.push_str(" - ");
      label.push_str(&episode.show.name);
      label
    }
    _ => String::from("Unknown item"),
  }
}

pub fn draw_queue(f: &mut Frame<'_>, app: &App) {
  let [area] = f
    .area()
    .layout(&Layout::vertical([Constraint::Percentage(100)]).margin(2));

  let style = app.user_config.theme.base_style();
  let len = match &app.queue {
    None => 1,
    Some(q) => {
      let item_count = usize::from(q.currently_playing.is_some()) + q.queue.len();
      item_count.max(1)
    }
  };
  let selected_index = app.queue_selected_index.min(len.saturating_sub(1));
  let visible_rows = area.height.saturating_sub(2) as usize;
  let offset = selectable_list_scroll_offset(selected_index, visible_rows);
  let visible_item_range = offset..len.min(offset.saturating_add(visible_rows.saturating_add(1)));

  let mut state = ListState::default();
  state.select(selected_index.checked_sub(offset));
  let list = List::new(visible_item_range.map(|row_index| match &app.queue {
    None => ListItem::new(Span::raw("Loading...")).style(style),
    Some(q) => {
      if row_index == 0 {
        if let Some(ref now) = q.currently_playing {
          return ListItem::new(Line::from(vec![
            Span::styled("Now playing: ", style.add_modifier(Modifier::BOLD)),
            Span::raw(queue_item_line(now)),
          ]))
          .style(style);
        }
      }

      let queue_index = row_index.saturating_sub(usize::from(q.currently_playing.is_some()));
      q.queue
        .get(queue_index)
        .map(|item| ListItem::new(queue_item_line(item)).style(style))
        .unwrap_or_else(|| ListItem::new(Span::raw("No queue (no active device?)")).style(style))
    }
  }))
  .block(
    Block::default()
      .borders(Borders::ALL)
      .style(style)
      .title(Span::styled("Queue (press Esc to go back)", style))
      .border_style(style),
  )
  .style(style)
  .highlight_style(
    Style::default()
      .fg(app.user_config.theme.active)
      .bg(app.user_config.theme.inactive)
      .add_modifier(Modifier::BOLD),
  )
  .highlight_symbol(Line::from("▶ ").style(Style::default().fg(app.user_config.theme.active)));
  f.render_stateful_widget(list, area, &mut state);
}

pub fn draw_error_screen(f: &mut Frame<'_>, app: &App) {
  let chunks = Layout::default()
    .direction(Direction::Vertical)
    .constraints([Constraint::Percentage(100)])
    .margin(5)
    .split(f.area());

  let playing_text = vec![
    Line::from(vec![
      Span::raw("Api response: "),
      Span::styled(
        &app.api_error,
        Style::default().fg(app.user_config.theme.error_text),
      ),
    ]),
    Line::from(Span::styled(
      "If you are trying to play a track, please check that",
      Style::default().fg(app.user_config.theme.text),
    )),
    Line::from(Span::styled(
      " 1. You have a Spotify Premium Account",
      Style::default().fg(app.user_config.theme.text),
    )),
    Line::from(Span::styled(
      " 2. Your playback device is active and selected - press `d` to go to device selection menu",
      Style::default().fg(app.user_config.theme.text),
    )),
    Line::from(Span::styled(
      " 3. If you're using spotifyd as a playback device, your device name must not contain spaces",
      Style::default().fg(app.user_config.theme.text),
    )),
    Line::from(Span::styled("Hint: a playback device must be either an official spotify client or a light weight alternative such as spotifyd",
        Style::default().fg(app.user_config.theme.hint)
        ),
    ),
    Line::from(
      Span::styled(
          "\nPress <Esc> to return",
          Style::default().fg(app.user_config.theme.inactive),
      ),
    )
  ];

  let playing_paragraph = Paragraph::new(playing_text)
    .wrap(Wrap { trim: true })
    .style(app.user_config.theme.base_style())
    .block(
      Block::default()
        .borders(Borders::ALL)
        .style(app.user_config.theme.base_style())
        .title(Span::styled(
          "Error",
          Style::default().fg(app.user_config.theme.error_border),
        ))
        .border_style(Style::default().fg(app.user_config.theme.error_border)),
    );
  f.render_widget(playing_paragraph, chunks[0]);
}

pub fn draw_dialog(f: &mut Frame<'_>, app: &App) {
  let dialog_context = match app.get_current_route().active_block {
    ActiveBlock::Dialog(context) => context,
    _ => return,
  };

  match dialog_context {
    DialogContext::PlaylistWindow | DialogContext::PlaylistSearch => {
      if let Some(playlist) = app.dialog.as_ref() {
        let text = vec![
          Line::from(Span::raw("Are you sure you want to delete the playlist: ")),
          Line::from(Span::styled(
            playlist.as_str(),
            Style::default().add_modifier(Modifier::BOLD),
          )),
          Line::from(Span::raw("?")),
        ];
        draw_confirmation_dialog(f, app, "Confirm", text, 45);
      }
    }
    DialogContext::RemoveTrackFromPlaylistConfirm => {
      if let Some(pending_remove) = app.pending_playlist_track_removal.as_ref() {
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let text = vec![
          Line::from(Span::raw("Remove this track from playlist?")),
          Line::from(vec![
            Span::styled("Track: ", bold),
            Span::styled(pending_remove.track_name.as_str(), bold),
          ]),
          Line::from(vec![
            Span::styled("Playlist: ", bold),
            Span::styled(pending_remove.playlist_name.as_str(), bold),
          ]),
        ];
        draw_confirmation_dialog(f, app, "Remove Track", text, 60);
      }
    }
    DialogContext::PersistKeybindingFallback => {
      if let Some(persist) = app.pending_keybinding_persist.as_ref() {
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let text = vec![
          Line::from(Span::raw("Ctrl+, is not reported by this terminal stack.")),
          Line::from(Span::raw("Use fallback shortcut for Open Settings?")),
          Line::from(vec![
            Span::styled("Save as: ", bold),
            Span::styled(persist.open_settings_key.to_string(), bold),
          ]),
        ];
        draw_confirmation_dialog(f, app, "Save Shortcut Fallback", text, 66);
      }
    }
    DialogContext::AddTrackToPlaylistPicker => {
      draw_add_track_to_playlist_picker_dialog(f, app);
    }
  }
}

fn centered_modal_rect(bounds: Rect, requested_width: u16, requested_height: u16) -> Rect {
  let width = requested_width.min(bounds.width.saturating_sub(2).max(1));
  let height = requested_height.min(bounds.height.saturating_sub(2).max(1));
  let left = bounds.x + bounds.width.saturating_sub(width) / 2;
  let top = bounds.y + bounds.height.saturating_sub(height) / 3;
  Rect::new(left, top, width, height)
}

fn draw_confirmation_dialog(
  f: &mut Frame<'_>,
  app: &App,
  title: &str,
  text: Vec<Line<'_>>,
  requested_width: u16,
) {
  let rect = centered_modal_rect(f.area(), requested_width, 10);
  f.render_widget(Clear, rect);

  let block = Block::default()
    .title(Span::styled(
      title,
      Style::default()
        .fg(app.user_config.theme.header)
        .add_modifier(Modifier::BOLD),
    ))
    .borders(Borders::ALL)
    .style(app.user_config.theme.base_style())
    .border_style(Style::default().fg(app.user_config.theme.inactive));
  f.render_widget(block, rect);

  let vchunks = Layout::default()
    .direction(Direction::Vertical)
    .margin(1)
    .constraints([Constraint::Min(3), Constraint::Length(3)])
    .split(rect);

  let text = Paragraph::new(text)
    .wrap(Wrap { trim: true })
    .style(app.user_config.theme.base_style())
    .alignment(Alignment::Center);
  f.render_widget(text, vchunks[0]);

  let hchunks = Layout::default()
    .direction(Direction::Horizontal)
    .horizontal_margin(3)
    .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
    .split(vchunks[1]);

  let ok = Paragraph::new(Span::raw("Ok"))
    .style(Style::default().fg(if app.confirm {
      app.user_config.theme.hovered
    } else {
      app.user_config.theme.inactive
    }))
    .alignment(Alignment::Center);
  f.render_widget(ok, hchunks[0]);

  let cancel = Paragraph::new(Span::raw("Cancel"))
    .style(Style::default().fg(if app.confirm {
      app.user_config.theme.inactive
    } else {
      app.user_config.theme.hovered
    }))
    .alignment(Alignment::Center);
  f.render_widget(cancel, hchunks[1]);
}

fn draw_add_track_to_playlist_picker_dialog(f: &mut Frame<'_>, app: &App) {
  let rect = centered_modal_rect(f.area(), 70, 20);
  f.render_widget(Clear, rect);

  let block = Block::default()
    .title(Span::styled(
      "Add Track To Playlist",
      Style::default()
        .fg(app.user_config.theme.header)
        .add_modifier(Modifier::BOLD),
    ))
    .borders(Borders::ALL)
    .style(app.user_config.theme.base_style())
    .border_style(Style::default().fg(app.user_config.theme.inactive));
  f.render_widget(block, rect);

  let vchunks = Layout::default()
    .direction(Direction::Vertical)
    .margin(1)
    .constraints([
      Constraint::Length(2),
      Constraint::Min(3),
      Constraint::Length(1),
    ])
    .split(rect);

  let track_name = app
    .pending_playlist_track_add
    .as_ref()
    .map(|p| p.track_name.as_str())
    .unwrap_or("Selected track");

  let header = Paragraph::new(Line::from(Span::raw(format!(
    "Choose a playlist for: {}",
    track_name
  ))))
  .wrap(Wrap { trim: true })
  .style(app.user_config.theme.base_style());
  f.render_widget(header, vchunks[0]);

  let mut list_state = ListState::default();
  let editable_count = app
    .all_playlists
    .iter()
    .filter(|playlist| app.playlist_is_editable(playlist))
    .count();

  if editable_count == 0 {
    let empty_text = Paragraph::new("No editable playlists available")
      .style(Style::default().fg(app.user_config.theme.inactive))
      .alignment(Alignment::Center);
    f.render_widget(empty_text, vchunks[1]);
  } else {
    let is_own_playlist = |playlist: &rspotify::model::SimplifiedPlaylist| -> bool {
      app
        .user
        .as_ref()
        .is_some_and(|user| user.id.id() == playlist.owner.id.id())
    };
    let selected = app.playlist_picker_selected_index.min(editable_count - 1);
    let visible_rows = vchunks[1].height as usize;
    let offset = selectable_list_scroll_offset(selected, visible_rows);
    let visible_items = app
      .all_playlists
      .iter()
      .filter(|playlist| app.playlist_is_editable(playlist))
      .skip(offset)
      .take(visible_rows.saturating_add(1))
      .map(|playlist| {
        let label = if is_own_playlist(playlist) {
          playlist.name.clone()
        } else {
          let owner = playlist
            .owner
            .display_name
            .as_deref()
            .unwrap_or_else(|| playlist.owner.id.id());
          format!("{} - {} (collab)", playlist.name, owner)
        };
        ListItem::new(Span::raw(label))
      });
    list_state.select(selected.checked_sub(offset));

    let list = List::new(visible_items)
      .style(app.user_config.theme.base_style())
      .highlight_style(Style::default().fg(app.user_config.theme.hovered))
      .highlight_symbol("▶ ");

    f.render_stateful_widget(list, vchunks[1], &mut list_state);
  }

  let footer = Paragraph::new("Enter add | q cancel | j/k or arrows move | H/M/L jump")
    .style(Style::default().fg(app.user_config.theme.inactive))
    .alignment(Alignment::Center);
  f.render_widget(footer, vchunks[2]);
}

pub fn draw_exit_prompt(f: &mut Frame<'_>, app: &App) {
  let width = std::cmp::min(f.area().width.saturating_sub(4), 56);
  let height = 8;
  let rect = f
    .area()
    .centered(Constraint::Length(width), Constraint::Length(height));

  f.render_widget(Clear, rect);

  let text = vec![
    Line::from(Span::styled(
      "Exit spotatui?",
      Style::default().add_modifier(Modifier::BOLD),
    )),
    Line::from(""),
    Line::from("Press Y for Yes or N for No"),
    Line::from(Span::styled(
      "[ENTER = Yes, ESC = No]",
      Style::default().fg(app.user_config.theme.inactive),
    )),
  ];

  let paragraph = Paragraph::new(text)
    .style(app.user_config.theme.base_style())
    .alignment(Alignment::Center)
    .block(
      Block::default()
        .borders(Borders::ALL)
        .style(app.user_config.theme.base_style())
        .border_style(Style::default().fg(app.user_config.theme.active))
        .title(" Confirm Exit "),
    );

  f.render_widget(paragraph, rect);
}

/// Draw the sort menu popup overlay
pub fn draw_sort_menu(f: &mut Frame<'_>, app: &App) {
  if !app.sort_menu_visible {
    return;
  }

  let context = match app.sort_context {
    Some(ctx) => ctx,
    None => return,
  };

  let available_fields = context.available_fields();
  let current_sort = match context {
    crate::core::sort::SortContext::PlaylistTracks => &app.playlist_sort,
    crate::core::sort::SortContext::SavedAlbums => &app.album_sort,
    crate::core::sort::SortContext::SavedArtists => &app.artist_sort,
    crate::core::sort::SortContext::RecentlyPlayed => &app.playlist_sort,
  };

  let width = std::cmp::min(f.area().width.saturating_sub(4), 35);
  let height = (available_fields.len() + 4) as u16; // +4 for borders/padding
  let rect = f
    .area()
    .centered(Constraint::Length(width), Constraint::Length(height));

  f.render_widget(Clear, rect);

  // Build list items
  let items = available_fields.iter().enumerate().map(|(i, field)| {
    let display_name = field.display_name();
    let shortcut = field.shortcut();
    let indicator = (*field == current_sort.field).then(|| current_sort.order.indicator());
    let mut text = String::with_capacity(
      display_name.len()
        + shortcut.map(|_| " (_)".len()).unwrap_or(0)
        + indicator.map(|indicator| 1 + indicator.len()).unwrap_or(0),
    );
    text.push_str(display_name);
    if let Some(shortcut) = shortcut {
      text.push_str(" (");
      text.push(shortcut);
      text.push(')');
    }
    if let Some(indicator) = indicator {
      text.push(' ');
      text.push_str(indicator);
    }

    let style = if i == app.sort_menu_selected {
      Style::default()
        .fg(app.user_config.theme.active)
        .add_modifier(Modifier::BOLD)
    } else if *field == current_sort.field {
      Style::default().fg(app.user_config.theme.hovered)
    } else {
      Style::default().fg(app.user_config.theme.text)
    };

    ListItem::new(text).style(style)
  });

  let title = match context {
    crate::core::sort::SortContext::PlaylistTracks => "Sort Tracks",
    crate::core::sort::SortContext::SavedAlbums => "Sort Albums",
    crate::core::sort::SortContext::SavedArtists => "Sort Artists",
    crate::core::sort::SortContext::RecentlyPlayed => "Sort",
  };

  let list = List::new(items)
    .block(
      Block::default()
        .borders(Borders::ALL)
        .style(app.user_config.theme.base_style())
        .border_style(Style::default().fg(app.user_config.theme.active))
        .title(Span::styled(
          title,
          Style::default()
            .fg(app.user_config.theme.active)
            .add_modifier(Modifier::BOLD),
        )),
    )
    .highlight_style(
      Style::default()
        .fg(app.user_config.theme.active)
        .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol(Line::from("▶ ").style(Style::default().fg(app.user_config.theme.active)));

  let mut state = ListState::default();
  state.select(Some(app.sort_menu_selected));

  f.render_stateful_widget(list, rect, &mut state);
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn truncate_help_cell_keeps_short_text_unchanged() {
    assert_eq!(truncate_help_cell("abc", 0), "");
    assert_eq!(truncate_help_cell("abc", 3), "abc");
    assert_eq!(truncate_help_cell("abc", 4), "abc");
  }

  #[test]
  fn truncate_help_cell_shortens_long_text_with_ellipsis() {
    assert_eq!(truncate_help_cell("abcdef", 1), "…");
    assert_eq!(truncate_help_cell("abcdef", 4), "abc…");
  }

  #[test]
  fn truncate_help_cell_does_not_split_unicode_codepoints() {
    assert_eq!(truncate_help_cell("åß∂ƒ", 3), "åß…");
  }
}
