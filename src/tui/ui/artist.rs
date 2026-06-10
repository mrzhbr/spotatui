use crate::core::app::{App, ArtistBlock};
use ratatui::{
  layout::{Constraint, Layout, Rect},
  Frame,
};
use rspotify::model::PlayableItem;
use rspotify::prelude::Id;

use super::util::{
  append_artist_string, draw_selectable_list_with, get_artist_highlight_state,
  SelectableListOptions,
};

pub fn draw_artist_albums(f: &mut Frame<'_>, app: &App, layout_chunk: Rect) {
  let [tracks_area, albums_area, related_artists_area] =
    layout_chunk.layout(&Layout::horizontal([
      Constraint::Percentage(33),
      Constraint::Percentage(33),
      Constraint::Percentage(33),
    ]));

  if let Some(artist) = &app.artist {
    let currently_playing_id = app
      .current_playback_context
      .as_ref()
      .and_then(|context| context.item.as_ref())
      .and_then(|item| match item {
        PlayableItem::Track(track) => track.id.as_ref().map(|id| id.id()),
        PlayableItem::Episode(episode) => Some(episode.id.id()),
        _ => None,
      });

    let top_tracks_title = format!("{} - Top Tracks", &artist.artist_name);
    draw_selectable_list_with(
      f,
      app,
      tracks_area,
      SelectableListOptions {
        title: &top_tracks_title,
        item_count: artist.top_tracks.len(),
        highlight_state: get_artist_highlight_state(app, ArtistBlock::TopTracks),
        selected_index: Some(artist.selected_top_track_index),
      },
      |index| {
        let Some(top_track) = artist.top_tracks.get(index) else {
          return String::new();
        };

        let mut name = String::new();
        if top_track
          .id
          .as_ref()
          .is_some_and(|id| currently_playing_id == Some(id.id()))
        {
          name.push_str("▶ ");
        }
        name.push_str(&top_track.name);
        name
      },
    );

    draw_selectable_list_with(
      f,
      app,
      albums_area,
      SelectableListOptions {
        title: "Albums",
        item_count: artist.albums.items.len(),
        highlight_state: get_artist_highlight_state(app, ArtistBlock::Albums),
        selected_index: Some(artist.selected_album_index),
      },
      |index| {
        let Some(item) = artist.albums.items.get(index) else {
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

    draw_selectable_list_with(
      f,
      app,
      related_artists_area,
      SelectableListOptions {
        title: "Related artists",
        item_count: artist.related_artists.len(),
        highlight_state: get_artist_highlight_state(app, ArtistBlock::RelatedArtists),
        selected_index: Some(artist.selected_related_artist_index),
      },
      |index| {
        let Some(item) = artist.related_artists.get(index) else {
          return String::new();
        };

        let mut artist_name = String::new();
        if app.followed_artist_ids_set.contains(item.id.id()) {
          artist_name.push_str(&app.user_config.behavior.liked_icon);
          artist_name.push(' ');
        }
        artist_name.push_str(&item.name);
        artist_name
      },
    );
  };
}
