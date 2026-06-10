use crate::core::app::{ActiveBlock, App, ArtistBlock, SearchResultBlock};
use crate::core::user_config::Theme;
use ratatui::{
  layout::Rect,
  style::{Modifier, Style},
  text::{Line, Span},
  widgets::{Block, BorderType, Borders, List, ListItem, ListState},
  Frame,
};
use rspotify::model::artist::SimplifiedArtist;
use std::{fmt::Write as _, time::Duration};

pub const SMALL_TERMINAL_WIDTH: u16 = 150;
pub const SMALL_TERMINAL_HEIGHT: u16 = 45;

pub fn get_search_results_highlight_state(
  app: &App,
  block_to_match: SearchResultBlock,
) -> (bool, bool) {
  let current_route = app.get_current_route();
  (
    app.search_results.selected_block == block_to_match,
    current_route.hovered_block == ActiveBlock::SearchResultBlock
      && app.search_results.hovered_block == block_to_match,
  )
}

pub fn get_artist_highlight_state(app: &App, block_to_match: ArtistBlock) -> (bool, bool) {
  let current_route = app.get_current_route();
  if let Some(artist) = &app.artist {
    let is_hovered = artist.artist_selected_block == block_to_match;
    let is_selected = current_route.hovered_block == ActiveBlock::ArtistBlock
      && artist.artist_hovered_block == block_to_match;
    (is_hovered, is_selected)
  } else {
    (false, false)
  }
}

pub fn get_color((is_active, is_hovered): (bool, bool), theme: Theme) -> Style {
  match (is_active, is_hovered) {
    (true, _) => Style::default().fg(theme.selected).bg(theme.background),
    (false, true) => Style::default().fg(theme.hovered).bg(theme.background),
    _ => Style::default().fg(theme.inactive).bg(theme.background),
  }
}

pub fn draw_selectable_list<S>(
  f: &mut Frame<'_>,
  app: &App,
  layout_chunk: Rect,
  title: &str,
  items: &[S],
  highlight_state: (bool, bool),
  selected_index: Option<usize>,
) where
  S: std::convert::AsRef<str>,
{
  draw_selectable_list_with(
    f,
    app,
    layout_chunk,
    SelectableListOptions {
      title,
      item_count: items.len(),
      highlight_state,
      selected_index,
    },
    |index| items[index].as_ref().to_owned(),
  );
}

pub struct SelectableListOptions<'a> {
  pub title: &'a str,
  pub item_count: usize,
  pub highlight_state: (bool, bool),
  pub selected_index: Option<usize>,
}

pub fn draw_selectable_list_with<F>(
  f: &mut Frame<'_>,
  app: &App,
  layout_chunk: Rect,
  options: SelectableListOptions<'_>,
  item_text: F,
) where
  F: Fn(usize) -> String,
{
  let SelectableListOptions {
    title,
    item_count,
    highlight_state,
    selected_index,
  } = options;
  let visible_rows = layout_chunk.height.saturating_sub(2) as usize;
  let selected_index = clamped_selected_index(selected_index, item_count);
  let offset = selected_index
    .map(|index| selectable_list_scroll_offset(index, visible_rows))
    .unwrap_or(0);
  let selected_visible_index = selected_index.and_then(|index| index.checked_sub(offset));

  let mut state = ListState::default();
  state.select(selected_visible_index);

  let visible_item_range =
    offset..item_count.min(offset.saturating_add(visible_rows.saturating_add(1)));

  let block = Block::default()
    .borders(Borders::ALL)
    .border_type(BorderType::Rounded)
    .title(Span::styled(
      title,
      get_color(highlight_state, app.user_config.theme),
    ))
    .border_style(get_color(highlight_state, app.user_config.theme));

  let list = List::new(visible_item_range.map(|index| ListItem::new(Span::raw(item_text(index)))))
    .block(block)
    .style(app.user_config.theme.base_style())
    .highlight_style(
      get_color(highlight_state, app.user_config.theme)
        .add_modifier(Modifier::BOLD | Modifier::REVERSED),
    )
    .highlight_symbol(Line::from("▶ ").style(get_color(highlight_state, app.user_config.theme)));
  f.render_stateful_widget(list, layout_chunk, &mut state);
}

fn clamped_selected_index(selected_index: Option<usize>, item_count: usize) -> Option<usize> {
  if item_count == 0 {
    None
  } else {
    selected_index.map(|index| index.min(item_count - 1))
  }
}

pub fn selectable_list_scroll_offset(selected_index: usize, visible_rows: usize) -> usize {
  if visible_rows == 0 {
    return 0;
  }

  selected_index.saturating_sub(visible_rows.saturating_sub(1))
}

pub fn append_artist_string(display: &mut String, artists: &[SimplifiedArtist]) {
  for (index, artist) in artists.iter().enumerate() {
    if index > 0 {
      display.push_str(", ");
    }
    display.push_str(&artist.name);
  }
}

pub fn create_artist_string(artists: &[SimplifiedArtist]) -> String {
  let mut display = String::new();
  append_artist_string(&mut display, artists);
  display
}

pub fn artist_line<'a>(artists: &'a [SimplifiedArtist], style: Style) -> Line<'a> {
  match artists {
    [] => Line::default(),
    [artist] => Line::from(Span::styled(artist.name.as_str(), style)),
    _ => {
      let mut spans = Vec::with_capacity(artists.len() * 2 - 1);
      for (index, artist) in artists.iter().enumerate() {
        if index > 0 {
          spans.push(Span::styled(", ", style));
        }
        spans.push(Span::styled(artist.name.as_str(), style));
      }
      Line::from(spans)
    }
  }
}

fn append_millis_to_minutes(display: &mut String, millis: u128) {
  let minutes = millis / 60000;
  let seconds = (millis % 60000) / 1000;
  write!(display, "{minutes}:{seconds:02}").expect("writing to String cannot fail");
}

pub fn millis_to_minutes(millis: u128) -> String {
  let mut display = String::new();
  append_millis_to_minutes(&mut display, millis);
  display
}

pub fn display_track_progress(progress: u128, track_duration: Duration) -> String {
  let duration_ms = track_duration.as_millis();
  let remaining = duration_ms.saturating_sub(progress);
  let mut display = String::new();
  append_millis_to_minutes(&mut display, progress);
  display.push('/');
  append_millis_to_minutes(&mut display, duration_ms);
  display.push_str(" (-");
  append_millis_to_minutes(&mut display, remaining);
  display.push(')');
  display
}

pub fn display_track_progress_unknown_duration(progress: u128) -> String {
  let mut display = String::new();
  append_millis_to_minutes(&mut display, progress);
  display.push_str("/--:--");
  display
}

// `percentage` param needs to be between 0 and 1
pub fn get_percentage_width(width: u16, percentage: f32) -> u16 {
  let padding = 3;
  let width = width - padding;
  (f32::from(width) * percentage) as u16
}

// Ensure track progress percentage is between 0 and 100 inclusive
pub fn get_track_progress_percentage(song_progress_ms: u128, track_duration: Duration) -> u16 {
  let min_perc = 0_f64;
  let track_progress = std::cmp::min(song_progress_ms, track_duration.as_millis());
  let track_perc = (track_progress as f64 / track_duration.as_millis() as f64) * 100_f64;
  min_perc.max(track_perc) as u16
}

// Make better use of space on small terminals
pub fn get_main_layout_margin(app: &App) -> u16 {
  if app.size.height > SMALL_TERMINAL_HEIGHT {
    1
  } else {
    0
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn selectable_list_scroll_offset_keeps_selected_row_visible() {
    assert_eq!(selectable_list_scroll_offset(0, 0), 0);
    assert_eq!(selectable_list_scroll_offset(0, 5), 0);
    assert_eq!(selectable_list_scroll_offset(4, 5), 0);
    assert_eq!(selectable_list_scroll_offset(5, 5), 1);
    assert_eq!(selectable_list_scroll_offset(20, 5), 16);
  }

  #[test]
  fn clamped_selected_index_prevents_stale_out_of_bounds_selection() {
    assert_eq!(clamped_selected_index(None, 3), None);
    assert_eq!(clamped_selected_index(Some(0), 0), None);
    assert_eq!(clamped_selected_index(Some(1), 3), Some(1));
    assert_eq!(clamped_selected_index(Some(9), 3), Some(2));
  }

  #[test]
  fn create_artist_string_joins_without_trailing_separator() {
    assert_eq!(create_artist_string(&[]), "");
    assert_eq!(
      create_artist_string(&[SimplifiedArtist {
        name: "One".to_string(),
        ..Default::default()
      }]),
      "One"
    );
    assert_eq!(
      create_artist_string(&[
        SimplifiedArtist {
          name: "One".to_string(),
          ..Default::default()
        },
        SimplifiedArtist {
          name: "Two".to_string(),
          ..Default::default()
        },
      ]),
      "One, Two"
    );
  }

  #[test]
  fn artist_line_joins_borrowed_artist_spans() {
    let artists = [
      SimplifiedArtist {
        name: "One".to_string(),
        ..Default::default()
      },
      SimplifiedArtist {
        name: "Two".to_string(),
        ..Default::default()
      },
    ];

    let line = artist_line(&artists, Style::default());

    assert_eq!(line.spans.len(), 3);
    assert_eq!(line.spans[0].content.as_ref(), "One");
    assert_eq!(line.spans[1].content.as_ref(), ", ");
    assert_eq!(line.spans[2].content.as_ref(), "Two");
  }

  #[test]
  fn millis_to_minutes_test() {
    assert_eq!(millis_to_minutes(0), "0:00");
    assert_eq!(millis_to_minutes(1000), "0:01");
    assert_eq!(millis_to_minutes(1500), "0:01");
    assert_eq!(millis_to_minutes(1900), "0:01");
    assert_eq!(millis_to_minutes(60 * 1000), "1:00");
    assert_eq!(millis_to_minutes(60 * 1500), "1:30");
  }

  #[test]
  fn display_track_progress_test() {
    let two_minutes = Duration::from_millis(2 * 60 * 1000);
    assert_eq!(display_track_progress(0, two_minutes), "0:00/2:00 (-2:00)");
    assert_eq!(
      display_track_progress(Duration::from_millis(60 * 1000).as_millis(), two_minutes),
      "1:00/2:00 (-1:00)"
    );
    assert_eq!(
      display_track_progress_unknown_duration(65_000),
      "1:05/--:--"
    );
  }

  #[test]
  fn get_track_progress_percentage_test() {
    let track_length = Duration::from_millis(60 * 1000);
    assert_eq!(get_track_progress_percentage(0, track_length), 0);
    assert_eq!(
      get_track_progress_percentage((60 * 1000) / 2, track_length),
      50
    );

    // If progress is somehow higher than total duration, 100 should be max
    assert_eq!(
      get_track_progress_percentage(60 * 1000 * 2, track_length),
      100
    );
  }
}
