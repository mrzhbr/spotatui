use crate::core::app::{ActiveBlock, App, LIBRARY_OPTIONS};
use crate::core::layout::library_constraints;
use ratatui::{
  layout::{Constraint, Layout, Rect},
  Frame,
};

use super::{
  search::draw_input_and_help_box,
  util::{
    draw_selectable_list, draw_selectable_list_with, SelectableListOptions, SMALL_TERMINAL_WIDTH,
  },
};

pub fn draw_library_block(f: &mut Frame<'_>, app: &App, layout_chunk: Rect) {
  let current_route = app.get_current_route();
  let highlight_state = (
    current_route.active_block == ActiveBlock::Library,
    current_route.hovered_block == ActiveBlock::Library,
  );
  draw_selectable_list(
    f,
    app,
    layout_chunk,
    "Library",
    &LIBRARY_OPTIONS,
    highlight_state,
    Some(app.library.selected_index),
  );
}

pub fn draw_playlist_block(f: &mut Frame<'_>, app: &App, layout_chunk: Rect) {
  let playlist_count = if app.playlist_folder_items.is_empty() {
    app.playlists.as_ref().map(|p| p.items.len()).unwrap_or(0)
  } else {
    app.get_playlist_display_count()
  };
  let item_count = playlist_count.saturating_add(1);

  let current_route = app.get_current_route();

  let highlight_state = (
    current_route.active_block == ActiveBlock::MyPlaylists,
    current_route.hovered_block == ActiveBlock::MyPlaylists,
  );

  draw_selectable_list_with(
    f,
    app,
    layout_chunk,
    SelectableListOptions {
      title: "Playlists",
      item_count,
      highlight_state,
      selected_index: app.selected_playlist_index,
    },
    |index| playlist_display_text(app, index, playlist_count),
  );
}

fn playlist_display_text(app: &App, index: usize, playlist_count: usize) -> String {
  if index == playlist_count {
    return "+ Add Playlist".to_string();
  }

  if app.playlist_folder_items.is_empty() {
    return app
      .playlists
      .as_ref()
      .and_then(|playlists| playlists.items.get(index))
      .map(|playlist| playlist.name.clone())
      .unwrap_or_else(|| "Unknown".to_string());
  }

  match app.get_playlist_display_item_at(index) {
    Some(crate::core::app::PlaylistFolderItem::Folder(folder)) => {
      if folder.name.starts_with('\u{2190}') {
        folder.name.clone()
      } else {
        format!("\u{1F4C1} {}", folder.name)
      }
    }
    Some(crate::core::app::PlaylistFolderItem::Playlist { index, .. }) => app
      .all_playlists
      .get(*index)
      .map(|playlist| playlist.name.clone())
      .unwrap_or_else(|| "Unknown".to_string()),
    None => "Unknown".to_string(),
  }
}

pub fn draw_user_block(f: &mut Frame<'_>, app: &App, layout_chunk: Rect) {
  // Check for width to make a responsive layout
  if app.size.width >= SMALL_TERMINAL_WIDTH && !app.user_config.behavior.enforce_wide_search_bar {
    let lib_constraints = library_constraints(&app.user_config.behavior);
    let [input_area, library_area, playlist_area] = layout_chunk.layout(&Layout::vertical([
      Constraint::Length(3),
      lib_constraints[0],
      lib_constraints[1],
    ]));

    // Search input and help
    draw_input_and_help_box(f, app, input_area);
    draw_library_block(f, app, library_area);
    draw_playlist_block(f, app, playlist_area);
  } else {
    let [library_area, playlist_area] = layout_chunk.layout(&Layout::vertical(
      library_constraints(&app.user_config.behavior),
    ));

    // Search input and help
    draw_library_block(f, app, library_area);
    draw_playlist_block(f, app, playlist_area);
  }
}
