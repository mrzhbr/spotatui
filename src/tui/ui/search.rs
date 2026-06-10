use crate::core::app::{ActiveBlock, App, InputContext, SearchResultBlock};
use ratatui::{
  layout::{Constraint, Layout, Rect},
  style::Style,
  text::{Span, Text},
  widgets::{Block, BorderType, Borders, Paragraph, Wrap},
  Frame,
};

use rspotify::model::PlayableItem;
use rspotify::prelude::Id;

use super::util::{
  append_artist_string, draw_selectable_list_with, get_color, get_search_results_highlight_state,
  SelectableListOptions, SMALL_TERMINAL_WIDTH,
};

const COMPACT_TOP_ROW_THRESHOLD: u16 = 60;
const COMPACT_HELP_WIDTH: u16 = 6;
const COMPACT_SETTINGS_WIDTH: u16 = 10;

pub fn draw_input_and_help_box(f: &mut Frame<'_>, app: &App, layout_chunk: Rect) {
  let compact_top_row = layout_chunk.width < COMPACT_TOP_ROW_THRESHOLD;

  // Check for the width and change the constraints accordingly
  let constraints = if compact_top_row {
    [
      Constraint::Min(1),
      Constraint::Length(COMPACT_HELP_WIDTH),
      Constraint::Length(COMPACT_SETTINGS_WIDTH),
    ]
  } else if app.size.width >= SMALL_TERMINAL_WIDTH
    && !app.user_config.behavior.enforce_wide_search_bar
  {
    [
      Constraint::Percentage(65),
      Constraint::Percentage(18),
      Constraint::Percentage(17),
    ]
  } else {
    [
      Constraint::Percentage(80),
      Constraint::Percentage(8),
      Constraint::Percentage(12),
    ]
  };

  let [input_area, help_area, settings_area] =
    layout_chunk.layout(&Layout::horizontal(constraints));

  let current_route = app.get_current_route();

  let highlight_state = (
    current_route.active_block == ActiveBlock::Input,
    current_route.hovered_block == ActiveBlock::Input,
  );

  let show_loading = app.is_loading && app.user_config.behavior.show_loading_indicator;
  let border_type = if show_loading {
    BorderType::Double
  } else {
    BorderType::Rounded
  };

  let input_string: String = app.input.iter().collect();
  let lines = Text::from(input_string);
  // Compute horizontal scroll so the cursor stays visible within the input box.
  // inner width = total width - 2 (for left and right borders)
  let inner_width = input_area.width.saturating_sub(2);
  let scroll_offset = if inner_width > 0 && app.input_cursor_position >= inner_width {
    app.input_cursor_position - inner_width + 1
  } else {
    0
  };
  app.input_scroll_offset.set(scroll_offset);

  let input_title = match app.input_context {
    InputContext::PlaylistTrackSearch => "Search Playlist",
    InputContext::GlobalSearch => "Search",
  };

  let input = Paragraph::new(lines).scroll((0, scroll_offset)).block(
    Block::default()
      .borders(Borders::ALL)
      .border_type(border_type)
      .title(Span::styled(
        input_title,
        get_color(highlight_state, app.user_config.theme),
      ))
      .style(app.user_config.theme.base_style())
      .border_style(get_color(highlight_state, app.user_config.theme)),
  );
  f.render_widget(input, input_area);

  let help_content = if show_loading {
    (app.user_config.theme.hint, "...")
  } else if compact_top_row {
    (app.user_config.theme.inactive, "?")
  } else {
    (app.user_config.theme.inactive, "Type ?")
  };

  let block = Block::default()
    .title(Span::styled("Help", Style::default().fg(help_content.0)))
    .borders(Borders::ALL)
    .border_type(BorderType::Rounded)
    .border_style(Style::default().fg(help_content.0));

  let lines = Text::from(help_content.1);
  let help = Paragraph::new(lines).block(block).style(
    Style::default()
      .fg(help_content.0)
      .bg(app.user_config.theme.background),
  );
  f.render_widget(help, help_area);

  let settings_keybind_string = app.effective_open_settings_key().to_string();
  let settings_keybind = settings_keybind_string.trim_matches(|c| c == '<' || c == '>');
  let settings_hint = if compact_top_row {
    Text::from(settings_keybind)
  } else {
    Text::from(format!("Type {settings_keybind}"))
  };
  let settings_color = app.user_config.theme.inactive;
  let settings_block = Block::default()
    .title(Span::styled(
      "Settings",
      Style::default().fg(settings_color),
    ))
    .borders(Borders::ALL)
    .border_type(BorderType::Rounded)
    .border_style(Style::default().fg(settings_color));

  let settings = Paragraph::new(settings_hint).block(settings_block).style(
    Style::default()
      .fg(settings_color)
      .bg(app.user_config.theme.background),
  );
  f.render_widget(settings, settings_area);
}

pub fn draw_search_results(f: &mut Frame<'_>, app: &App, layout_chunk: Rect) {
  let [song_artist_area, albums_playlist_area, podcasts_area] =
    layout_chunk.layout(&Layout::vertical([
      Constraint::Percentage(35),
      Constraint::Percentage(35),
      Constraint::Percentage(25),
    ]));

  {
    let [songs_area, artists_area] = song_artist_area.layout(&Layout::horizontal([
      Constraint::Percentage(50),
      Constraint::Percentage(50),
    ]));

    let currently_playing_id = app
      .current_playback_context
      .as_ref()
      .and_then(|context| context.item.as_ref())
      .and_then(|item| match item {
        PlayableItem::Track(track) => track.id.as_ref().map(|id| id.id()),
        PlayableItem::Episode(episode) => Some(episode.id.id()),
        _ => None,
      });

    let song_count = app
      .search_results
      .tracks
      .as_ref()
      .map(|tracks| tracks.items.len())
      .unwrap_or(0);
    draw_selectable_list_with(
      f,
      app,
      songs_area,
      SelectableListOptions {
        title: "Songs",
        item_count: song_count,
        highlight_state: get_search_results_highlight_state(app, SearchResultBlock::SongSearch),
        selected_index: app.search_results.selected_tracks_index,
      },
      |index| {
        let Some(item) = app
          .search_results
          .tracks
          .as_ref()
          .and_then(|tracks| tracks.items.get(index))
        else {
          return String::new();
        };

        let item_id = item.id.as_ref().map(|id| id.id());
        let mut song_name = String::new();
        if item_id.is_some_and(|id| currently_playing_id == Some(id)) {
          song_name.push_str("▶ ");
        }
        if item_id.is_some_and(|id| app.liked_song_ids_set.contains(id)) {
          song_name.push_str(&app.user_config.behavior.liked_icon);
          song_name.push(' ');
        }
        song_name.push_str(&item.name);
        song_name.push_str(" - ");
        append_artist_string(&mut song_name, &item.artists);
        song_name
      },
    );

    let artist_count = app
      .search_results
      .artists
      .as_ref()
      .map(|artists| artists.items.len())
      .unwrap_or(0);
    draw_selectable_list_with(
      f,
      app,
      artists_area,
      SelectableListOptions {
        title: "Artists",
        item_count: artist_count,
        highlight_state: get_search_results_highlight_state(app, SearchResultBlock::ArtistSearch),
        selected_index: app.search_results.selected_artists_index,
      },
      |index| {
        let Some(item) = app
          .search_results
          .artists
          .as_ref()
          .and_then(|artists| artists.items.get(index))
        else {
          return String::new();
        };

        let mut artist = String::new();
        if app.followed_artist_ids_set.contains(item.id.id()) {
          artist.push_str(&app.user_config.behavior.liked_icon);
          artist.push(' ');
        }
        artist.push_str(&item.name);
        artist
      },
    );
  }

  {
    let [albums_area, playlist_area] = albums_playlist_area.layout(&Layout::horizontal([
      Constraint::Percentage(50),
      Constraint::Percentage(50),
    ]));

    let album_count = app
      .search_results
      .albums
      .as_ref()
      .map(|albums| albums.items.len())
      .unwrap_or(0);
    draw_selectable_list_with(
      f,
      app,
      albums_area,
      SelectableListOptions {
        title: "Albums",
        item_count: album_count,
        highlight_state: get_search_results_highlight_state(app, SearchResultBlock::AlbumSearch),
        selected_index: app.search_results.selected_album_index,
      },
      |index| {
        let Some(item) = app
          .search_results
          .albums
          .as_ref()
          .and_then(|albums| albums.items.get(index))
        else {
          return String::new();
        };

        let mut album_artist = String::new();
        if item
          .id
          .as_ref()
          .is_some_and(|album_id| app.saved_album_ids_set.contains(album_id.id()))
        {
          album_artist.push_str(&app.user_config.behavior.liked_icon);
          album_artist.push(' ');
        }
        album_artist.push_str(&item.name);
        album_artist.push_str(" - ");
        append_artist_string(&mut album_artist, &item.artists);
        album_artist.push_str(" (");
        album_artist.push_str(item.album_type.as_deref().unwrap_or("unknown"));
        album_artist.push(')');
        album_artist
      },
    );

    let playlist_count = app
      .search_results
      .playlists
      .as_ref()
      .map(|playlists| playlists.items.len())
      .unwrap_or(0);

    if playlist_count == 0 {
      let warning_text = "Cannot display Spotify created playlists. Try a more specific search to find user-created playlists.";
      let warning_paragraph = Paragraph::new(warning_text)
        .wrap(Wrap { trim: true })
        .style(Style::default().fg(app.user_config.theme.hint))
        .block(
          Block::default()
            .title(Span::styled(
              "Playlists",
              get_color(
                get_search_results_highlight_state(app, SearchResultBlock::PlaylistSearch),
                app.user_config.theme,
              ),
            ))
            .borders(Borders::ALL)
            .border_style(get_color(
              get_search_results_highlight_state(app, SearchResultBlock::PlaylistSearch),
              app.user_config.theme,
            )),
        );
      f.render_widget(warning_paragraph, playlist_area);
    } else {
      draw_selectable_list_with(
        f,
        app,
        playlist_area,
        SelectableListOptions {
          title: "Playlists",
          item_count: playlist_count,
          highlight_state: get_search_results_highlight_state(
            app,
            SearchResultBlock::PlaylistSearch,
          ),
          selected_index: app.search_results.selected_playlists_index,
        },
        |index| {
          app
            .search_results
            .playlists
            .as_ref()
            .and_then(|playlists| playlists.items.get(index))
            .map(|item| item.name.clone())
            .unwrap_or_default()
        },
      );
    }
  }

  {
    let podcast_count = app
      .search_results
      .shows
      .as_ref()
      .map(|podcasts| podcasts.items.len())
      .unwrap_or(0);
    draw_selectable_list_with(
      f,
      app,
      podcasts_area,
      SelectableListOptions {
        title: "Podcasts",
        item_count: podcast_count,
        highlight_state: get_search_results_highlight_state(app, SearchResultBlock::ShowSearch),
        selected_index: app.search_results.selected_shows_index,
      },
      |index| {
        let Some(item) = app
          .search_results
          .shows
          .as_ref()
          .and_then(|podcasts| podcasts.items.get(index))
        else {
          return String::new();
        };

        let mut show_name = String::new();
        if app.saved_show_ids_set.contains(item.id.id()) {
          show_name.push_str(&app.user_config.behavior.liked_icon);
          show_name.push(' ');
        }
        show_name.push_str(&item.name);
        show_name.push_str(" - ");
        #[allow(deprecated)]
        show_name.push_str(&item.publisher);
        show_name
      },
    );
  }
}
