use crate::core::app::{
  ActiveBlock, AlbumTableContext, App, EpisodeTableContext, RecommendationsContext,
};
use ratatui::{
  layout::{Constraint, Rect},
  style::{Modifier, Style},
  text::Span,
  widgets::{Block, Borders, Row, Table},
  Frame,
};
use rspotify::model::show::ResumePoint;
use rspotify::model::{track::SimplifiedTrack, PlayableItem};
use rspotify::prelude::Id;

use super::util::{
  append_artist_string, create_artist_string, get_color, get_percentage_width, millis_to_minutes,
};

fn current_playing_item_id(app: &App) -> Option<&str> {
  app
    .current_playback_context
    .as_ref()
    .and_then(|ctx| ctx.item.as_ref())
    .and_then(|item| match item {
      PlayableItem::Track(track) => track.id.as_ref().map(|id| id.id()),
      PlayableItem::Episode(episode) => Some(episode.id.id()),
      _ => None,
    })
}

fn liked_icon_cell(app: &App) -> String {
  let liked_icon = app.user_config.behavior.liked_icon.as_str();
  let mut cell = String::with_capacity(liked_icon.len() + 1);
  cell.push_str(liked_icon);
  cell.push(' ');
  cell
}

fn prefix_currently_playing(label: &mut String) {
  label.reserve("▶ ".len());
  label.insert_str(0, "▶ ");
}

pub struct TableHeader<'a, const N: usize> {
  pub items: [TableHeaderItem<'a>; N],
}

#[derive(Default)]
pub struct TableHeaderItem<'a> {
  pub text: &'a str,
  pub width: u16,
}

pub fn draw_artist_table(f: &mut Frame<'_>, app: &App, layout_chunk: Rect) {
  let header = TableHeader {
    items: [TableHeaderItem {
      text: "Artist",
      width: get_percentage_width(layout_chunk.width, 1.0),
    }],
  };

  let current_route = app.get_current_route();
  let highlight_state = (
    current_route.active_block == ActiveBlock::Artists,
    current_route.hovered_block == ActiveBlock::Artists,
  );

  let selected_index = app.artists_list_index;
  let selected_style = get_color(highlight_state, app.user_config.theme)
    .add_modifier(Modifier::BOLD | Modifier::REVERSED);
  let padding = 5;
  let visible_rows = layout_chunk
    .height
    .checked_sub(padding)
    .map(|height| height as usize)
    .unwrap_or(0);
  let offset = table_scroll_offset(selected_index, visible_rows);
  let selected_visible_index = selected_index.checked_sub(offset);

  let rows = app
    .library
    .saved_artists
    .get_results(None)
    .into_iter()
    .flat_map(|saved_artists| saved_artists.items.iter())
    .skip(offset)
    .take(visible_rows.saturating_add(1))
    .enumerate()
    .map(|(i, item)| {
      let style = if Some(i) == selected_visible_index {
        selected_style
      } else {
        app.user_config.theme.base_style()
      };
      Row::new([item.name.to_owned()]).style(style)
    });

  let table = Table::new(
    rows,
    header.items.iter().map(|h| Constraint::Length(h.width)),
  )
  .header(
    Row::new(header.items.iter().map(|h| h.text))
      .style(Style::default().fg(app.user_config.theme.header)),
  )
  .block(
    Block::default()
      .borders(Borders::ALL)
      .style(app.user_config.theme.base_style())
      .title(Span::styled(
        "Artists",
        get_color(highlight_state, app.user_config.theme),
      ))
      .border_style(get_color(highlight_state, app.user_config.theme)),
  )
  .style(app.user_config.theme.base_style());
  f.render_widget(table, layout_chunk);
}

pub fn draw_podcast_table(f: &mut Frame<'_>, app: &App, layout_chunk: Rect) {
  let header = TableHeader {
    items: [
      TableHeaderItem {
        text: "Name",
        width: get_percentage_width(layout_chunk.width, 2.0 / 5.0),
      },
      TableHeaderItem {
        text: "Publisher(s)",
        width: get_percentage_width(layout_chunk.width, 2.0 / 5.0),
      },
    ],
  };

  let current_route = app.get_current_route();

  let highlight_state = (
    current_route.active_block == ActiveBlock::Podcasts,
    current_route.hovered_block == ActiveBlock::Podcasts,
  );

  if let Some(saved_shows) = app.library.saved_shows.get_results(None) {
    let selected_index = app.shows_list_index;
    let selected_style = get_color(highlight_state, app.user_config.theme)
      .add_modifier(Modifier::BOLD | Modifier::REVERSED);
    let padding = 5;
    let visible_rows = layout_chunk
      .height
      .checked_sub(padding)
      .map(|height| height as usize)
      .unwrap_or(0);
    let offset = table_scroll_offset(selected_index, visible_rows);
    let selected_visible_index = selected_index.checked_sub(offset);

    let rows = saved_shows
      .items
      .iter()
      .skip(offset)
      .take(visible_rows.saturating_add(1))
      .enumerate()
      .map(|(i, show_page)| {
        let style = if Some(i) == selected_visible_index {
          selected_style
        } else {
          app.user_config.theme.base_style()
        };
        #[allow(deprecated)]
        let publisher = show_page.show.publisher.to_owned();
        Row::new([show_page.show.name.to_owned(), publisher]).style(style)
      });

    let table = Table::new(
      rows,
      header.items.iter().map(|h| Constraint::Length(h.width)),
    )
    .header(
      Row::new(header.items.iter().map(|h| h.text))
        .style(Style::default().fg(app.user_config.theme.header)),
    )
    .block(
      Block::default()
        .borders(Borders::ALL)
        .style(app.user_config.theme.base_style())
        .title(Span::styled(
          "Podcasts",
          get_color(highlight_state, app.user_config.theme),
        ))
        .border_style(get_color(highlight_state, app.user_config.theme)),
    )
    .style(app.user_config.theme.base_style());
    f.render_widget(table, layout_chunk);
  };
}

pub fn draw_album_table(f: &mut Frame<'_>, app: &App, layout_chunk: Rect) {
  let header = TableHeader {
    items: [
      TableHeaderItem { text: "", width: 2 },
      TableHeaderItem {
        text: "#",
        width: 3,
      },
      TableHeaderItem {
        text: "Title",
        width: get_percentage_width(layout_chunk.width, 2.0 / 5.0) - 5,
      },
      TableHeaderItem {
        text: "Artist",
        width: get_percentage_width(layout_chunk.width, 2.0 / 5.0),
      },
      TableHeaderItem {
        text: "Length",
        width: get_percentage_width(layout_chunk.width, 1.0 / 5.0),
      },
    ],
  };

  let current_route = app.get_current_route();
  let highlight_state = (
    current_route.active_block == ActiveBlock::AlbumTracks,
    current_route.hovered_block == ActiveBlock::AlbumTracks,
  );

  match &app.album_table_context {
    AlbumTableContext::Simplified => {
      if let Some(selected_album_simplified) = app.selected_album_simplified.as_ref() {
        let mut title = selected_album_simplified.album.name.clone();
        title.push_str(" by ");
        append_artist_string(&mut title, &selected_album_simplified.album.artists);
        draw_album_track_window(
          f,
          app,
          layout_chunk,
          AlbumTrackWindow {
            header: &header,
            title: &title,
            tracks: &selected_album_simplified.tracks.items,
            selected_index: selected_album_simplified.selected_index,
            highlight_state,
          },
        );
      }
    }
    AlbumTableContext::Full => {
      if let Some(selected_album) = app.selected_album_full.as_ref() {
        let mut title = selected_album.album.name.clone();
        title.push_str(" by ");
        append_artist_string(&mut title, &selected_album.album.artists);
        draw_album_track_window(
          f,
          app,
          layout_chunk,
          AlbumTrackWindow {
            header: &header,
            title: &title,
            tracks: &selected_album.album.tracks.items,
            selected_index: app.saved_album_tracks_index,
            highlight_state,
          },
        );
      }
    }
  };
}

struct AlbumTrackWindow<'a, const N: usize> {
  header: &'a TableHeader<'a, N>,
  title: &'a str,
  tracks: &'a [SimplifiedTrack],
  selected_index: usize,
  highlight_state: (bool, bool),
}

fn draw_album_track_window<const N: usize>(
  f: &mut Frame<'_>,
  app: &App,
  layout_chunk: Rect,
  window: AlbumTrackWindow<'_, N>,
) {
  let AlbumTrackWindow {
    header,
    title,
    tracks,
    selected_index,
    highlight_state,
  } = window;
  let selected_style = get_color(highlight_state, app.user_config.theme)
    .add_modifier(Modifier::BOLD | Modifier::REVERSED);
  let track_playing_id = current_playing_item_id(app);
  let padding = 5;
  let visible_rows = layout_chunk
    .height
    .checked_sub(padding)
    .map(|height| height as usize)
    .unwrap_or(0);
  let offset = table_scroll_offset(selected_index, visible_rows);
  let selected_visible_index = selected_index.checked_sub(offset);

  let rows = tracks
    .iter()
    .skip(offset)
    .take(visible_rows.saturating_add(1))
    .enumerate()
    .map(|(i, item)| {
      let item_id = item.id.as_ref().map(|id| id.id());
      let mut title = item.name.to_owned();
      let mut style = app.user_config.theme.base_style();

      if item_id.is_some_and(|id| track_playing_id == Some(id)) {
        prefix_currently_playing(&mut title);
        style = Style::default()
          .fg(app.user_config.theme.active)
          .add_modifier(Modifier::BOLD);
      }

      let liked = if item_id.is_some_and(|id| app.liked_song_ids_set.contains(id)) {
        liked_icon_cell(app)
      } else {
        String::new()
      };

      if Some(i) == selected_visible_index {
        style = selected_style;
      }

      Row::new([
        liked,
        item.track_number.to_string(),
        title,
        create_artist_string(&item.artists),
        millis_to_minutes(item.duration.num_milliseconds() as u128),
      ])
      .style(style)
    });

  let table = Table::new(
    rows,
    header.items.iter().map(|h| Constraint::Length(h.width)),
  )
  .header(
    Row::new(header.items.iter().map(|h| h.text))
      .style(Style::default().fg(app.user_config.theme.header)),
  )
  .block(
    Block::default()
      .borders(Borders::ALL)
      .style(app.user_config.theme.base_style())
      .title(Span::styled(
        title,
        get_color(highlight_state, app.user_config.theme),
      ))
      .border_style(get_color(highlight_state, app.user_config.theme)),
  )
  .style(app.user_config.theme.base_style());
  f.render_widget(table, layout_chunk);
}

pub fn draw_recommendations_table(f: &mut Frame<'_>, app: &App, layout_chunk: Rect) {
  let current_route = app.get_current_route();
  let highlight_state = (
    current_route.active_block == ActiveBlock::TrackTable,
    current_route.hovered_block == ActiveBlock::TrackTable,
  );

  let recommendations_ui = match &app.recommendations_context {
    Some(RecommendationsContext::Song) => format!(
      "Recommendations based on Song \'{}\'",
      &app.recommendations_seed
    ),
    Some(RecommendationsContext::Artist) => format!(
      "Recommendations based on Artist \'{}\'",
      &app.recommendations_seed
    ),
    None => "Recommendations".to_string(),
  };

  draw_track_table_window(f, app, layout_chunk, &recommendations_ui, highlight_state);
}

pub fn draw_song_table(f: &mut Frame<'_>, app: &App, layout_chunk: Rect) {
  let current_route = app.get_current_route();
  let highlight_state = (
    current_route.active_block == ActiveBlock::TrackTable,
    current_route.hovered_block == ActiveBlock::TrackTable,
  );

  let title = if app.is_playlist_track_table_context() {
    if let Some(query) = app.pending_playlist_track_search.as_ref() {
      format!("Songs (searching: {query}...)")
    } else {
      app
        .active_playlist_track_filter
        .as_ref()
        .map(|query| format!("Songs (filtered: {query})"))
        .unwrap_or_else(|| "Songs".to_string())
    }
  } else {
    "Songs".to_string()
  };

  draw_track_table_window(f, app, layout_chunk, &title, highlight_state);
}

fn track_table_header(layout_width: u16) -> TableHeader<'static, 5> {
  TableHeader {
    items: [
      TableHeaderItem { text: "", width: 2 },
      TableHeaderItem {
        text: "Title",
        width: get_percentage_width(layout_width, 0.3),
      },
      TableHeaderItem {
        text: "Artist",
        width: get_percentage_width(layout_width, 0.3),
      },
      TableHeaderItem {
        text: "Album",
        width: get_percentage_width(layout_width, 0.3),
      },
      TableHeaderItem {
        text: "Length",
        width: get_percentage_width(layout_width, 0.1),
      },
    ],
  }
}

fn draw_track_table_window(
  f: &mut Frame<'_>,
  app: &App,
  layout_chunk: Rect,
  title: &str,
  highlight_state: (bool, bool),
) {
  let header = track_table_header(layout_chunk.width);
  let selected_style = get_color(highlight_state, app.user_config.theme)
    .add_modifier(Modifier::BOLD | Modifier::REVERSED);

  let track_playing_id = current_playing_item_id(app);

  let padding = 5;
  let visible_rows = layout_chunk
    .height
    .checked_sub(padding)
    .map(|height| height as usize)
    .unwrap_or(0);
  let offset = table_scroll_offset(app.track_table.selected_index, visible_rows);
  let selected_visible_index = app.track_table.selected_index.checked_sub(offset);

  let rows = app
    .track_table
    .tracks
    .iter()
    .skip(offset)
    .take(visible_rows.saturating_add(1))
    .enumerate()
    .map(|(i, item)| {
      let item_id = item.id.as_ref().map(|id| id.id());
      let mut title = item.name.to_owned();
      let mut style = app.user_config.theme.base_style();

      if item_id.is_some_and(|id| track_playing_id == Some(id)) {
        prefix_currently_playing(&mut title);
        style = Style::default()
          .fg(app.user_config.theme.active)
          .add_modifier(Modifier::BOLD);
      }

      let liked = if item_id.is_some_and(|id| app.liked_song_ids_set.contains(id)) {
        liked_icon_cell(app)
      } else {
        String::new()
      };

      if Some(i) == selected_visible_index {
        style = selected_style;
      }

      Row::new([
        liked,
        title,
        create_artist_string(&item.artists),
        item.album.name.to_owned(),
        millis_to_minutes(item.duration.num_milliseconds() as u128),
      ])
      .style(style)
    });

  let table = Table::new(
    rows,
    header.items.iter().map(|h| Constraint::Length(h.width)),
  )
  .header(
    Row::new(header.items.iter().map(|h| h.text))
      .style(Style::default().fg(app.user_config.theme.header)),
  )
  .block(
    Block::default()
      .borders(Borders::ALL)
      .style(app.user_config.theme.base_style())
      .title(Span::styled(
        title,
        get_color(highlight_state, app.user_config.theme),
      ))
      .border_style(get_color(highlight_state, app.user_config.theme)),
  )
  .style(app.user_config.theme.base_style());
  f.render_widget(table, layout_chunk);
}

pub fn draw_album_list(f: &mut Frame<'_>, app: &App, layout_chunk: Rect) {
  let header = TableHeader {
    items: [
      TableHeaderItem {
        text: "Name",
        width: get_percentage_width(layout_chunk.width, 2.0 / 5.0),
      },
      TableHeaderItem {
        text: "Artists",
        width: get_percentage_width(layout_chunk.width, 2.0 / 5.0),
      },
      TableHeaderItem {
        text: "Release Date",
        width: get_percentage_width(layout_chunk.width, 1.0 / 5.0),
      },
    ],
  };

  let current_route = app.get_current_route();

  let highlight_state = (
    current_route.active_block == ActiveBlock::AlbumList,
    current_route.hovered_block == ActiveBlock::AlbumList,
  );

  let selected_index = app.album_list_index;

  if let Some(saved_albums) = app.library.saved_albums.get_results(None) {
    let selected_style = get_color(highlight_state, app.user_config.theme)
      .add_modifier(Modifier::BOLD | Modifier::REVERSED);
    let padding = 5;
    let visible_rows = layout_chunk
      .height
      .checked_sub(padding)
      .map(|height| height as usize)
      .unwrap_or(0);
    let offset = table_scroll_offset(selected_index, visible_rows);
    let selected_visible_index = selected_index.checked_sub(offset);

    let rows = saved_albums
      .items
      .iter()
      .skip(offset)
      .take(visible_rows.saturating_add(1))
      .enumerate()
      .map(|(i, album_page)| {
        let style = if Some(i) == selected_visible_index {
          selected_style
        } else {
          app.user_config.theme.base_style()
        };

        let liked_icon = app.user_config.behavior.liked_icon.as_str();
        let mut album_name =
          String::with_capacity(liked_icon.len() + 1 + album_page.album.name.len());
        album_name.push_str(liked_icon);
        album_name.push(' ');
        album_name.push_str(&album_page.album.name);

        Row::new([
          album_name,
          create_artist_string(&album_page.album.artists),
          album_page.album.release_date.to_owned(),
        ])
        .style(style)
      });

    let table = Table::new(
      rows,
      header.items.iter().map(|h| Constraint::Length(h.width)),
    )
    .header(
      Row::new(header.items.iter().map(|h| h.text))
        .style(Style::default().fg(app.user_config.theme.header)),
    )
    .block(
      Block::default()
        .borders(Borders::ALL)
        .style(app.user_config.theme.base_style())
        .title(Span::styled(
          "Saved Albums",
          get_color(highlight_state, app.user_config.theme),
        ))
        .border_style(get_color(highlight_state, app.user_config.theme)),
    )
    .style(app.user_config.theme.base_style());
    f.render_widget(table, layout_chunk);
  };
}

pub fn draw_show_episodes(f: &mut Frame<'_>, app: &App, layout_chunk: Rect) {
  let header = TableHeader {
    items: [
      TableHeaderItem {
        // Column to mark an episode as fully played
        text: "",
        width: 2,
      },
      TableHeaderItem {
        text: "Date",
        width: get_percentage_width(layout_chunk.width, 0.5 / 5.0) - 2,
      },
      TableHeaderItem {
        text: "Name",
        width: get_percentage_width(layout_chunk.width, 3.5 / 5.0),
      },
      TableHeaderItem {
        text: "Duration",
        width: get_percentage_width(layout_chunk.width, 1.0 / 5.0),
      },
    ],
  };

  let current_route = app.get_current_route();

  let highlight_state = (
    current_route.active_block == ActiveBlock::EpisodeTable,
    current_route.hovered_block == ActiveBlock::EpisodeTable,
  );

  if let Some(episodes) = app.library.show_episodes.get_results(None) {
    #[allow(deprecated)]
    let title = match &app.episode_table_context {
      EpisodeTableContext::Simplified => match &app.selected_show_simplified {
        Some(selected_show) => {
          format!(
            "{} by {}",
            selected_show.show.name.to_owned(),
            selected_show.show.publisher
          )
        }
        None => "Episodes".to_owned(),
      },
      EpisodeTableContext::Full => match &app.selected_show_full {
        Some(selected_show) => {
          format!(
            "{} by {}",
            selected_show.show.name.to_owned(),
            selected_show.show.publisher
          )
        }
        None => "Episodes".to_owned(),
      },
    };

    let selected_index = app.episode_list_index;
    let selected_style = get_color(highlight_state, app.user_config.theme)
      .add_modifier(Modifier::BOLD | Modifier::REVERSED);
    let track_playing_id = current_playing_item_id(app);
    let padding = 5;
    let visible_rows = layout_chunk
      .height
      .checked_sub(padding)
      .map(|height| height as usize)
      .unwrap_or(0);
    let offset = table_scroll_offset(selected_index, visible_rows);
    let selected_visible_index = selected_index.checked_sub(offset);

    let rows = episodes
      .items
      .iter()
      .skip(offset)
      .take(visible_rows.saturating_add(1))
      .enumerate()
      .map(|(i, episode)| {
        let (played_str, time_str) = match episode.resume_point {
          Some(ResumePoint {
            fully_played,
            resume_position,
          }) => (
            if fully_played {
              " ✔".to_owned()
            } else {
              "".to_owned()
            },
            format!(
              "{} / {}",
              millis_to_minutes(resume_position.num_milliseconds() as u128),
              millis_to_minutes(episode.duration.num_milliseconds() as u128)
            ),
          ),
          None => (
            "".to_owned(),
            millis_to_minutes(episode.duration.num_milliseconds() as u128),
          ),
        };

        let mut name = episode.name.to_owned();
        let mut style = app.user_config.theme.base_style();
        if track_playing_id == Some(episode.id.id()) {
          prefix_currently_playing(&mut name);
          style = Style::default()
            .fg(app.user_config.theme.active)
            .add_modifier(Modifier::BOLD);
        }
        if Some(i) == selected_visible_index {
          style = selected_style;
        }

        Row::new([played_str, episode.release_date.to_owned(), name, time_str]).style(style)
      });

    let table = Table::new(
      rows,
      header.items.iter().map(|h| Constraint::Length(h.width)),
    )
    .header(
      Row::new(header.items.iter().map(|h| h.text))
        .style(Style::default().fg(app.user_config.theme.header)),
    )
    .block(
      Block::default()
        .borders(Borders::ALL)
        .style(app.user_config.theme.base_style())
        .title(Span::styled(
          title,
          get_color(highlight_state, app.user_config.theme),
        ))
        .border_style(get_color(highlight_state, app.user_config.theme)),
    )
    .style(app.user_config.theme.base_style());
    f.render_widget(table, layout_chunk);
  };
}

pub fn draw_recently_played_table(f: &mut Frame<'_>, app: &App, layout_chunk: Rect) {
  let header = TableHeader {
    items: [
      TableHeaderItem { text: "", width: 2 },
      TableHeaderItem {
        text: "Title",
        // We need to subtract the fixed value of the previous column
        width: get_percentage_width(layout_chunk.width, 2.0 / 5.0) - 2,
      },
      TableHeaderItem {
        text: "Artist",
        width: get_percentage_width(layout_chunk.width, 2.0 / 5.0),
      },
      TableHeaderItem {
        text: "Length",
        width: get_percentage_width(layout_chunk.width, 1.0 / 5.0),
      },
    ],
  };

  if let Some(recently_played) = &app.recently_played.result {
    let current_route = app.get_current_route();

    let highlight_state = (
      current_route.active_block == ActiveBlock::RecentlyPlayed,
      current_route.hovered_block == ActiveBlock::RecentlyPlayed,
    );

    let selected_index = app.recently_played.index;
    let selected_style = get_color(highlight_state, app.user_config.theme)
      .add_modifier(Modifier::BOLD | Modifier::REVERSED);
    let track_playing_id = current_playing_item_id(app);
    let padding = 5;
    let visible_rows = layout_chunk
      .height
      .checked_sub(padding)
      .map(|height| height as usize)
      .unwrap_or(0);
    let offset = table_scroll_offset(selected_index, visible_rows);
    let selected_visible_index = selected_index.checked_sub(offset);

    let rows = recently_played
      .items
      .iter()
      .skip(offset)
      .take(visible_rows.saturating_add(1))
      .enumerate()
      .map(|(i, item)| {
        let item_id = item.track.id.as_ref().map(|id| id.id());
        let mut title = item.track.name.to_owned();
        let mut style = app.user_config.theme.base_style();

        if item_id.is_some_and(|id| track_playing_id == Some(id)) {
          prefix_currently_playing(&mut title);
          style = Style::default()
            .fg(app.user_config.theme.active)
            .add_modifier(Modifier::BOLD);
        }

        let liked = if item_id.is_some_and(|id| app.liked_song_ids_set.contains(id)) {
          liked_icon_cell(app)
        } else {
          String::new()
        };

        if Some(i) == selected_visible_index {
          style = selected_style;
        }

        Row::new([
          liked,
          title,
          create_artist_string(&item.track.artists),
          millis_to_minutes(item.track.duration.num_milliseconds() as u128),
        ])
        .style(style)
      });

    let table = Table::new(
      rows,
      header.items.iter().map(|h| Constraint::Length(h.width)),
    )
    .header(
      Row::new(header.items.iter().map(|h| h.text))
        .style(Style::default().fg(app.user_config.theme.header)),
    )
    .block(
      Block::default()
        .borders(Borders::ALL)
        .style(app.user_config.theme.base_style())
        .title(Span::styled(
          "Recently Played Tracks",
          get_color(highlight_state, app.user_config.theme),
        ))
        .border_style(get_color(highlight_state, app.user_config.theme)),
    )
    .style(app.user_config.theme.base_style());
    f.render_widget(table, layout_chunk);
  };
}

pub fn table_scroll_offset(selected_index: usize, visible_rows: usize) -> usize {
  if visible_rows == 0 {
    return 0;
  }

  selected_index.saturating_sub(visible_rows.saturating_sub(1))
}
