use super::requests::is_rate_limited_error;
use super::Network;
use crate::core::app::{ActiveBlock, App, DiscoverTimeRange, RouteId};
use crate::core::playback_target::{parse_sonos_persisted_id, PlaybackTargetRef};
use anyhow::anyhow;

use rand::seq::SliceRandom;
use rspotify::model::{
  artist::FullArtist,
  device::DevicePayload,
  page::{CursorBasedPage, Page},
  playing::PlayHistory,
  track::FullTrack,
  user::PrivateUser,
};
use rspotify::prelude::*;
use serde::Deserialize;
use std::time::{Duration, Instant};

#[derive(Deserialize)]
struct ArtistTopTracksResponse {
  tracks: Vec<FullTrack>,
}

fn should_discover_sonos_for_devices(
  selected_sonos_room_uuid: Option<&str>,
  persisted_device_id: Option<&str>,
  explicit_device_picker: bool,
) -> bool {
  explicit_device_picker
    || selected_sonos_room_uuid.is_some()
    || persisted_device_id.is_some_and(|device_id| parse_sonos_persisted_id(device_id).is_some())
}

fn refreshed_device_selection_index(app: &App, target_count: usize) -> Option<usize> {
  if target_count == 0 {
    return None;
  }

  if let Some(selected_uuid) = app.selected_sonos_room_uuid.as_deref() {
    if let Some(index) = (0..target_count).position(|index| {
      matches!(
        app.playback_target_at(index),
        Some(PlaybackTargetRef::Sonos { room, .. }) if room.uuid == selected_uuid
      )
    }) {
      return Some(index);
    }
  } else if let Some(index) = (0..target_count).position(|index| {
    matches!(
      app.playback_target_at(index),
      Some(PlaybackTargetRef::Spotify {
        is_active: true,
        ..
      })
    )
  }) {
    return Some(index);
  }

  Some(
    app
      .selected_device_index
      .unwrap_or(0)
      .min(target_count.saturating_sub(1)),
  )
}

pub trait UserNetwork {
  async fn get_user(&mut self);
  async fn get_devices(&mut self);
  async fn get_user_top_tracks(&mut self, time_range: DiscoverTimeRange);
  async fn get_top_artists_mix(&mut self);
  #[allow(dead_code)]
  async fn get_recently_played(&mut self);
}

impl UserNetwork for Network {
  async fn get_user(&mut self) {
    match self.spotify_get_typed::<PrivateUser>("me", &[]).await {
      Ok(user) => {
        let mut app = self.app.lock().await;
        app.user = Some(user);
      }
      Err(e) => {
        let err = anyhow!(e);
        if is_rate_limited_error(&err) {
          let mut app = self.app.lock().await;
          app.status_message = Some(
            "Spotify rate limit hit while loading profile. Retrying automatically.".to_string(),
          );
          app.status_message_expires_at = Some(Instant::now() + Duration::from_secs(6));
          return;
        }
        self.handle_error(err).await;
      }
    }
  }

  async fn get_devices(&mut self) {
    {
      let mut app = self.app.lock().await;
      app.push_navigation_stack(RouteId::SelectedDevice, ActiveBlock::SelectDevice);
    }

    let selected_sonos_room_uuid = {
      let app = self.app.lock().await;
      app.selected_sonos_room_uuid.clone()
    };
    let should_discover_sonos = should_discover_sonos_for_devices(
      selected_sonos_room_uuid.as_deref(),
      self.client_config.device_id.as_deref(),
      true,
    );

    let spotify_devices = self
      .spotify_get_typed::<DevicePayload>("me/player/devices", &[])
      .await;
    let sonos_rooms = if should_discover_sonos {
      Some(crate::infra::sonos::discover_rooms().await)
    } else {
      None
    };

    let mut app = self.app.lock().await;

    if let Ok(result) = spotify_devices {
      app.devices = Some(result);
    }

    if let Some(sonos_rooms) = sonos_rooms {
      match sonos_rooms {
        Ok(rooms) => {
          if rooms.is_empty() {
            app.set_status_message(
              "No Sonos rooms found via SSDP; showing Spotify devices only".to_string(),
              6,
            );
          } else {
            app.sonos_rooms = rooms;
          }
        }
        Err(e) => {
          app.set_status_message(format!("No Sonos rooms found via SSDP: {e}"), 6);
        }
      }
    }

    let target_count = app.playback_target_count();
    if target_count == 0 {
      app.set_status_message(
        "No Spotify devices found. Make sure a device is active in Spotify.",
        6,
      );
    }

    app.selected_device_index = refreshed_device_selection_index(&app, target_count);
  }

  async fn get_user_top_tracks(&mut self, time_range: DiscoverTimeRange) {
    let range_str = match time_range {
      DiscoverTimeRange::Short => "short_term",
      DiscoverTimeRange::Medium => "medium_term",
      DiscoverTimeRange::Long => "long_term",
    };

    // Set loading state
    {
      let mut app = self.app.lock().await;
      app.discover_loading = true;
    }

    match self
      .spotify_get_typed::<Page<FullTrack>>(
        "me/top/tracks",
        &[
          ("time_range", range_str.to_string()),
          ("limit", "50".to_string()),
        ],
      )
      .await
    {
      Ok(page) => {
        let mut app = self.app.lock().await;
        app.discover_top_tracks = page.items;
        app.discover_loading = false;
      }
      Err(e) => {
        let mut app = self.app.lock().await;
        app.discover_loading = false;
        app.handle_error(anyhow!(e));
      }
    }
  }

  async fn get_top_artists_mix(&mut self) {
    // Set loading state
    {
      let mut app = self.app.lock().await;
      app.discover_loading = true;
    }

    // 1. Get top artists
    let artists_res = self
      .spotify_get_typed::<Page<FullArtist>>(
        "me/top/artists",
        &[("limit", "5".to_string())], // Get top 5 artists
      )
      .await;

    let artists = match artists_res {
      Ok(page) => page.items,
      Err(e) => {
        let mut app = self.app.lock().await;
        app.discover_loading = false;
        app.handle_error(anyhow!(e));
        return;
      }
    };

    let mut all_tracks = Vec::new();

    // 2. Get top tracks for each artist
    for artist in artists {
      let path = format!("artists/{}/top-tracks", artist.id.id());
      if let Ok(res) = self
        .spotify_get_typed::<ArtistTopTracksResponse>(&path, &[])
        .await
      {
        all_tracks.extend(res.tracks);
      }
    }

    // 3. Shuffle
    {
      let mut rng = rand::thread_rng();
      all_tracks.shuffle(&mut rng);
    }

    // 4. Update state
    let mut app = self.app.lock().await;
    app.discover_artists_mix = all_tracks;
    app.discover_loading = false;
  }

  async fn get_recently_played(&mut self) {
    let limit = self.large_search_limit;
    match self
      .spotify_get_typed::<CursorBasedPage<PlayHistory>>(
        "me/player/recently-played",
        &[("limit", limit.to_string())],
      )
      .await
    {
      Ok(recently_played) => {
        let mut app = self.app.lock().await;
        app.recently_played.result = Some(recently_played);
        app.push_navigation_stack(RouteId::RecentlyPlayed, ActiveBlock::RecentlyPlayed);
      }
      Err(e) => {
        self.handle_error(anyhow!(e)).await;
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::playback_target::{sonos_persisted_id, SonosRoom};
  use crate::core::user_config::UserConfig;
  use rspotify::model::{Device, DeviceType};
  use std::{sync::mpsc::channel, time::SystemTime};

  fn app() -> App {
    let (tx, _rx) = channel();
    App::new(tx, UserConfig::new(), SystemTime::now())
  }

  fn spotify_device(id: Option<&str>, name: &str, is_active: bool) -> Device {
    Device {
      id: id.map(ToString::to_string),
      is_active,
      is_private_session: false,
      is_restricted: false,
      name: name.to_string(),
      _type: DeviceType::Computer,
      volume_percent: Some(50),
    }
  }

  #[test]
  fn sonos_discovery_is_skipped_without_selected_or_persisted_sonos_target() {
    assert!(!should_discover_sonos_for_devices(None, None, false));
    assert!(!should_discover_sonos_for_devices(
      None,
      Some("spotify-device-id"),
      false
    ));
  }

  #[test]
  fn sonos_discovery_runs_for_explicit_device_picker() {
    assert!(should_discover_sonos_for_devices(None, None, true));
  }

  #[test]
  fn sonos_discovery_runs_when_sonos_room_is_selected() {
    assert!(should_discover_sonos_for_devices(
      Some("RINCON_SELECTED"),
      None,
      false
    ));
  }

  #[test]
  fn sonos_discovery_runs_when_sonos_device_is_persisted() {
    let persisted_id = sonos_persisted_id("RINCON_PERSISTED");

    assert!(should_discover_sonos_for_devices(
      None,
      Some(&persisted_id),
      false
    ));
  }

  #[test]
  fn refreshed_device_selection_prefers_selected_sonos_room() {
    let mut app = app();
    app.devices = Some(DevicePayload {
      devices: vec![spotify_device(Some("spotify-1"), "Desktop", true)],
    });
    app.sonos_rooms = vec![
      SonosRoom {
        uuid: "RINCON_OTHER".to_string(),
        name: "Kitchen".to_string(),
        location: "http://192.168.1.10:1400/xml/device_description.xml".to_string(),
      },
      SonosRoom {
        uuid: "RINCON_SELECTED".to_string(),
        name: "Living Room".to_string(),
        location: "http://192.168.1.20:1400/xml/device_description.xml".to_string(),
      },
    ];
    app.selected_sonos_room_uuid = Some("RINCON_SELECTED".to_string());

    assert_eq!(
      refreshed_device_selection_index(&app, app.playback_target_count()),
      Some(2)
    );
  }

  #[test]
  fn refreshed_device_selection_prefers_active_spotify_device_without_sonos_selection() {
    let mut app = app();
    app.devices = Some(DevicePayload {
      devices: vec![
        spotify_device(Some("spotify-1"), "Desktop", false),
        spotify_device(None, "Nameless", true),
        spotify_device(Some("spotify-2"), "Phone", true),
      ],
    });
    app.sonos_rooms.push(SonosRoom {
      uuid: "RINCON_123".to_string(),
      name: "Living Room".to_string(),
      location: "http://192.168.1.20:1400/xml/device_description.xml".to_string(),
    });

    assert_eq!(
      refreshed_device_selection_index(&app, app.playback_target_count()),
      Some(1)
    );
  }

  #[test]
  fn refreshed_device_selection_clamps_previous_index_when_preferred_target_is_missing() {
    let mut app = app();
    app.selected_device_index = Some(9);
    app.selected_sonos_room_uuid = Some("RINCON_MISSING".to_string());
    app.devices = Some(DevicePayload {
      devices: vec![spotify_device(Some("spotify-1"), "Desktop", false)],
    });

    assert_eq!(
      refreshed_device_selection_index(&app, app.playback_target_count()),
      Some(0)
    );
  }
}
