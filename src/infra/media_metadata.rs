#![cfg_attr(
  not(all(feature = "macos-media", target_os = "macos")),
  allow(dead_code)
)]

use crate::core::app::App;
use crate::tui::ui::util::create_artist_string;
use rspotify::model::{FullEpisode, FullTrack, PlayableItem};

#[derive(Clone, Debug, PartialEq)]
pub struct PlaybackMetadata {
  pub title: String,
  pub artists: Vec<String>,
  pub album: String,
  pub image_url: Option<String>,
  pub duration_ms: u32,
}

pub fn current_playback_metadata(app: &App) -> Option<PlaybackMetadata> {
  let context = app.current_playback_context.as_ref();
  if app.is_streaming_active {
    if let Some(native_info) = app.native_track_info.as_ref() {
      return Some(PlaybackMetadata {
        title: native_info.name.clone(),
        artists: vec![native_info.artists_display.clone()],
        album: native_info.album.clone(),
        image_url: image_url_from_context_item(context.and_then(|ctx| ctx.item.as_ref())),
        duration_ms: native_info.duration_ms,
      });
    }
  }

  metadata_from_context_item(context.and_then(|ctx| ctx.item.as_ref()))
}

fn metadata_from_track(track: &FullTrack) -> PlaybackMetadata {
  PlaybackMetadata {
    title: track.name.clone(),
    artists: vec![create_artist_string(&track.artists)],
    album: track.album.name.clone(),
    image_url: track.album.images.first().map(|image| image.url.clone()),
    duration_ms: track.duration.num_milliseconds() as u32,
  }
}

fn metadata_from_episode(episode: &FullEpisode) -> PlaybackMetadata {
  PlaybackMetadata {
    title: episode.name.clone(),
    artists: vec![episode.show.name.clone()],
    album: String::new(),
    image_url: episode.images.first().map(|image| image.url.clone()),
    duration_ms: episode.duration.num_milliseconds() as u32,
  }
}

fn metadata_from_context_item(item: Option<&PlayableItem>) -> Option<PlaybackMetadata> {
  match item? {
    PlayableItem::Track(track) => Some(metadata_from_track(track)),
    PlayableItem::Episode(episode) => Some(metadata_from_episode(episode)),
    PlayableItem::Unknown(_) => None,
  }
}

fn image_url_from_context_item(item: Option<&PlayableItem>) -> Option<String> {
  match item? {
    PlayableItem::Track(track) => track.album.images.first().map(|image| image.url.clone()),
    PlayableItem::Episode(episode) => episode.images.first().map(|image| image.url.clone()),
    PlayableItem::Unknown(_) => None,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::app::NativeTrackInfo;
  use chrono::{Duration, Utc};
  use rspotify::model::{
    context::{Actions, CurrentPlaybackContext},
    idtypes::{EpisodeId, ShowId},
    show::{FullEpisode, SimplifiedShow},
    track::FullTrack,
    CurrentlyPlayingType, Device, DeviceType, Image, PlayableItem, RepeatState, SimplifiedAlbum,
    SimplifiedArtist, Type,
  };
  use std::{collections::HashMap, sync::mpsc::channel, time::SystemTime};

  fn app() -> App {
    let (tx, _rx) = channel();
    App::new(
      tx,
      crate::core::user_config::UserConfig::new(),
      SystemTime::now(),
    )
  }

  #[allow(deprecated)]
  fn playback_context(item: PlayableItem, is_playing: bool) -> CurrentPlaybackContext {
    CurrentPlaybackContext {
      device: Device {
        id: Some("device-1".to_string()),
        is_active: true,
        is_private_session: false,
        is_restricted: false,
        name: "Desk Speaker".to_string(),
        _type: DeviceType::Computer,
        volume_percent: Some(42),
      },
      repeat_state: RepeatState::Off,
      shuffle_state: true,
      context: None,
      timestamp: Utc::now(),
      progress: None,
      is_playing,
      item: Some(item),
      currently_playing_type: CurrentlyPlayingType::Track,
      actions: Actions::default(),
    }
  }

  #[allow(deprecated)]
  fn track() -> FullTrack {
    FullTrack {
      album: SimplifiedAlbum {
        name: "Album".to_string(),
        images: vec![Image {
          height: Some(640),
          url: "https://example.com/cover.jpg".to_string(),
          width: Some(640),
        }],
        ..Default::default()
      },
      artists: vec![SimplifiedArtist {
        name: "Artist".to_string(),
        ..Default::default()
      }],
      available_markets: Vec::new(),
      disc_number: 1,
      duration: Duration::milliseconds(181_000),
      explicit: false,
      external_ids: HashMap::new(),
      external_urls: HashMap::new(),
      href: None,
      id: None,
      is_local: false,
      is_playable: Some(true),
      linked_from: None,
      restrictions: None,
      name: "Track".to_string(),
      popularity: 50,
      preview_url: None,
      track_number: 1,
      r#type: Type::Track,
    }
  }

  #[allow(deprecated)]
  fn episode() -> FullEpisode {
    FullEpisode {
      audio_preview_url: None,
      description: "Description".to_string(),
      duration: Duration::milliseconds(2_400_000),
      explicit: false,
      external_urls: HashMap::new(),
      href: "https://example.com/episode".to_string(),
      id: EpisodeId::from_id("0zTOsY4qQhZQ6JcZx7aG4P")
        .unwrap()
        .into_static(),
      images: vec![Image {
        height: Some(640),
        url: "https://example.com/episode.jpg".to_string(),
        width: Some(640),
      }],
      is_externally_hosted: false,
      is_playable: true,
      language: "en".to_string(),
      languages: vec!["en".to_string()],
      name: "Episode".to_string(),
      release_date: "2024-01-01".to_string(),
      release_date_precision: rspotify::model::DatePrecision::Day,
      resume_point: None,
      show: SimplifiedShow {
        available_markets: Vec::new(),
        copyrights: Vec::new(),
        description: "Show description".to_string(),
        explicit: false,
        external_urls: HashMap::new(),
        href: "https://example.com/show".to_string(),
        id: ShowId::from_id("6mD5pBAZpHeQOdT0bFvB1V")
          .unwrap()
          .into_static(),
        images: Vec::new(),
        is_externally_hosted: None,
        languages: vec!["en".to_string()],
        media_type: "audio".to_string(),
        name: "Show".to_string(),
        publisher: "Publisher".to_string(),
      },
      r#type: Type::Episode,
    }
  }

  #[test]
  fn extracts_native_track_info() {
    let mut app = app();
    app.is_streaming_active = true;
    app.native_track_info = Some(NativeTrackInfo {
      name: "Native Track".to_string(),
      artists_display: "Native Artist".to_string(),
      album: "Native Album".to_string(),
      duration_ms: 123_000,
    });

    let metadata = current_playback_metadata(&app).unwrap();

    assert_eq!(metadata.title, "Native Track");
    assert_eq!(metadata.artists, vec!["Native Artist"]);
    assert_eq!(metadata.album, "Native Album");
    assert_eq!(metadata.duration_ms, 123_000);
  }

  #[test]
  fn extracts_metadata_without_building_snapshot_identity() {
    let mut app = app();
    app.current_playback_context = Some(playback_context(PlayableItem::Track(track()), true));

    let metadata = current_playback_metadata(&app).unwrap();

    assert_eq!(metadata.title, "Track");
    assert_eq!(metadata.artists, vec!["Artist"]);
    assert_eq!(metadata.album, "Album");
    assert_eq!(
      metadata.image_url.as_deref(),
      Some("https://example.com/cover.jpg")
    );
    assert_eq!(metadata.duration_ms, 181_000);
  }

  #[test]
  fn ignores_stale_native_play_state_for_api_metadata() {
    let mut app = app();
    app.native_is_playing = Some(false);
    app.current_playback_context = Some(playback_context(PlayableItem::Track(track()), true));

    let metadata = current_playback_metadata(&app).unwrap();

    assert_eq!(metadata.title, "Track");
  }

  #[test]
  fn ignores_stale_native_metadata_when_streaming_is_inactive() {
    let mut app = app();
    app.native_is_playing = Some(false);
    app.native_track_info = Some(NativeTrackInfo {
      name: "Native Track".to_string(),
      artists_display: "Native Artist".to_string(),
      album: "Native Album".to_string(),
      duration_ms: 123_000,
    });
    app.current_playback_context = Some(playback_context(PlayableItem::Track(track()), true));

    let metadata = current_playback_metadata(&app).unwrap();

    assert_eq!(metadata.title, "Track");
  }

  #[test]
  fn extracts_spotify_track() {
    let mut app = app();
    app.current_playback_context = Some(playback_context(PlayableItem::Track(track()), true));

    let metadata = current_playback_metadata(&app).unwrap();

    assert_eq!(metadata.title, "Track");
    assert_eq!(metadata.artists, vec!["Artist"]);
    assert_eq!(metadata.album, "Album");
    assert_eq!(
      metadata.image_url.as_deref(),
      Some("https://example.com/cover.jpg")
    );
    assert_eq!(metadata.duration_ms, 181_000);
  }

  #[test]
  fn extracts_spotify_episode() {
    let mut app = app();
    app.current_playback_context = Some(playback_context(PlayableItem::Episode(episode()), false));

    let metadata = current_playback_metadata(&app).unwrap();

    assert_eq!(metadata.title, "Episode");
    assert_eq!(metadata.artists, vec!["Show"]);
    assert_eq!(metadata.album, "");
    assert_eq!(
      metadata.image_url.as_deref(),
      Some("https://example.com/episode.jpg")
    );
    assert_eq!(metadata.duration_ms, 2_400_000);
  }

  #[test]
  fn empty_playback_has_no_metadata() {
    let app = app();

    assert_eq!(current_playback_metadata(&app), None);
  }
}
