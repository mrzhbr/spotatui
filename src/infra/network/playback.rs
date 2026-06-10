use super::{IoEvent, Network};
#[cfg(feature = "streaming")]
use crate::core::app::NativePlaybackOrigin;
use crate::core::playback_target::{
  parse_sonos_persisted_id, sonos_persisted_id, SonosNowPlaying, SonosRoom,
};
use crate::tui::ui::util::create_artist_string;
use anyhow::anyhow;
use chrono::TimeDelta;
#[cfg(feature = "streaming")]
use log::info;
use reqwest::Method;
#[cfg(feature = "streaming")]
use rspotify::model::device::{Device, DevicePayload};
use rspotify::model::{
  context::CurrentUserQueue,
  enums::RepeatState,
  idtypes::{PlayContextId, PlayableId},
  PlayableItem,
};
use rspotify::prelude::*;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

#[cfg(feature = "streaming")]
use librespot_connect::{LoadRequest, LoadRequestOptions, PlayingTrack};
#[cfg(feature = "streaming")]
use std::sync::Arc;

const MAX_API_PLAYBACK_URIS: usize = 100;

pub trait PlaybackNetwork {
  async fn get_current_playback(&mut self);
  async fn start_playback(
    &mut self,
    context_id: Option<PlayContextId<'static>>,
    uris: Option<Vec<PlayableId<'static>>>,
    offset: Option<usize>,
  );
  async fn pause_playback(&mut self);
  async fn next_track(&mut self);
  async fn previous_track(&mut self);
  async fn force_previous_track(&mut self);
  async fn seek(&mut self, position_ms: u32);
  async fn shuffle(&mut self, shuffle_state: bool);
  async fn repeat(&mut self, repeat_state: RepeatState);
  async fn change_volume(&mut self, volume: u8);
  async fn transfert_playback_to_device(&mut self, device_id: String, persist_device_id: bool);
  async fn transfer_playback_to_sonos_room(&mut self, room_uuid: String, persist_device_id: bool);
  #[cfg(feature = "streaming")]
  async fn auto_select_streaming_device(&mut self, device_name: String, persist_device_id: bool);
  async fn ensure_playback_continues(&mut self, previous_track_id: String);
  #[allow(dead_code)]
  async fn add_item_to_queue(&mut self, item: PlayableId<'static>);
  async fn get_queue(&mut self);
}

async fn sonos_room_by_uuid(network: &Network, room_uuid: &str) -> Option<SonosRoom> {
  let app = network.app.lock().await;
  app
    .sonos_rooms
    .iter()
    .find(|room| room.uuid == room_uuid)
    .cloned()
}

async fn refresh_sonos_rooms(network: &Network) -> anyhow::Result<Vec<SonosRoom>> {
  let rooms = crate::infra::sonos::discover_rooms().await?;
  let mut app = network.app.lock().await;
  if !rooms.is_empty() {
    app.sonos_rooms = rooms.clone();
  }
  Ok(rooms)
}

enum SelectedSonosRoom {
  None,
  Missing,
  Room(SonosRoom),
}

async fn selected_sonos_room(network: &Network) -> SelectedSonosRoom {
  let selected_uuid = {
    let app = network.app.lock().await;
    app.selected_sonos_room_uuid.clone()
  };

  let Some(selected_uuid) = selected_uuid else {
    return SelectedSonosRoom::None;
  };

  if let Some(room) = sonos_room_by_uuid(network, &selected_uuid).await {
    return SelectedSonosRoom::Room(room);
  }

  match refresh_sonos_rooms(network).await {
    Ok(rooms) => {
      if let Some(room) = rooms.into_iter().find(|room| room.uuid == selected_uuid) {
        SelectedSonosRoom::Room(room)
      } else {
        let mut app = network.app.lock().await;
        app.sonos_is_playing = Some(false);
        app.sonos_now_playing = None;
        app.is_volume_change_in_flight = false;
        app.pending_volume = None;
        app.last_dispatched_volume = None;
        app.set_status_message(
          "Selected Sonos room unavailable. Check that it is powered on and on this network.",
          6,
        );
        SelectedSonosRoom::Missing
      }
    }
    Err(e) => {
      let mut app = network.app.lock().await;
      app.sonos_is_playing = Some(false);
      app.sonos_now_playing = None;
      app.is_volume_change_in_flight = false;
      app.pending_volume = None;
      app.last_dispatched_volume = None;
      app.set_status_message(format!("Could not discover selected Sonos room: {e}"), 6);
      SelectedSonosRoom::Missing
    }
  }
}

async fn handle_sonos_error(network: &Network, err: anyhow::Error) {
  let mut app = network.app.lock().await;
  let message = err.to_string();
  let lower_message = message.to_lowercase();

  if message.contains("UPnP error 800")
    || lower_message.contains("account")
    || lower_message.contains("auth")
    || lower_message.contains("token")
  {
    app.set_status_message(
      "Sonos could not play Spotify. Link Spotify in the Sonos app first, then try again.",
      7,
    );
  } else if message.contains("UPnP error 701") || message.contains("UPnP error 711") {
    app.set_status_message("Sonos cannot perform that transport action right now.", 5);
  } else if lower_message.contains("unsupported spotify")
    || lower_message.contains("cannot start a new spotify item")
  {
    app.set_status_message(message, 6);
  } else {
    app.set_status_message(format!("Sonos: {message}"), 6);
  }
}

fn sonos_now_playing_from_snapshot(
  room_uuid: String,
  snapshot: crate::infra::sonos::transport::SonosPlaybackSnapshot,
) -> SonosNowPlaying {
  SonosNowPlaying {
    room_uuid,
    title: snapshot.title,
    artist: snapshot.artist,
    album: snapshot.album,
    track_uri: snapshot.track_uri,
    duration_ms: snapshot.duration_ms,
    position_ms: snapshot.position_ms,
    is_playing: snapshot.is_playing,
    volume_percent: snapshot.volume_percent,
    fetched_at: Instant::now(),
  }
}

fn trim_api_playback_uris(
  track_uris: Vec<PlayableId<'static>>,
  offset: Option<usize>,
) -> (Vec<PlayableId<'static>>, Option<usize>) {
  if track_uris.len() <= MAX_API_PLAYBACK_URIS {
    return (track_uris, offset);
  }

  let selected_index = offset.unwrap_or(0).min(track_uris.len().saturating_sub(1));
  let preferred_history = MAX_API_PLAYBACK_URIS / 5;
  let mut start = selected_index.saturating_sub(preferred_history);
  let end = (start + MAX_API_PLAYBACK_URIS).min(track_uris.len());

  if end - start < MAX_API_PLAYBACK_URIS {
    start = end.saturating_sub(MAX_API_PLAYBACK_URIS);
  }

  // Spotify rejects oversized URI payloads, so URI-list playback is capped
  // to a window that still contains the selected track.
  let trimmed_uris = track_uris[start..end]
    .iter()
    .map(PlayableId::clone_static)
    .collect::<Vec<_>>();

  (trimmed_uris, Some(selected_index - start))
}

fn api_playback_offset_json(
  context_uris: Option<&[PlayableId<'static>]>,
  offset: Option<usize>,
) -> Option<Value> {
  if let Some(first_uri) = context_uris.and_then(|uris| uris.first()) {
    return Some(json!({ "uri": first_uri.uri() }));
  }

  offset.map(|index| json!({ "position": index }))
}

fn api_playback_body(
  context_id: Option<&PlayContextId<'static>>,
  uris: Option<&[PlayableId<'static>]>,
  offset: Option<usize>,
) -> Option<Value> {
  match (context_id, uris) {
    (Some(context), track_uris) => {
      let mut body = json!({ "context_uri": context.uri() });
      if let Some(offset) = api_playback_offset_json(track_uris, offset) {
        body["offset"] = offset;
      }
      Some(body)
    }
    (None, Some(track_uris)) => {
      let mut body = json!({
        "uris": track_uris.iter().map(|uri| uri.uri()).collect::<Vec<_>>()
      });
      if let Some(offset) = api_playback_offset_json(None, offset) {
        body["offset"] = offset;
      }
      Some(body)
    }
    (None, None) => None,
  }
}

fn playable_item_id(item: &PlayableItem) -> Option<&str> {
  match item {
    PlayableItem::Track(track) => track.id.as_ref().map(|id| id.id()),
    PlayableItem::Episode(episode) => Some(episode.id.id()),
    PlayableItem::Unknown(_) => None,
  }
}

fn playable_item_name(item: &PlayableItem) -> Option<&str> {
  match item {
    PlayableItem::Track(track) => Some(&track.name),
    PlayableItem::Episode(episode) => Some(&episode.name),
    PlayableItem::Unknown(_) => None,
  }
}

#[cfg(feature = "streaming")]
#[derive(Debug, PartialEq, Eq)]
enum NativePlaybackRoute {
  ContextApi { device_id: String },
  NativeLoad,
}

fn api_confirms_native_info_is_current(
  native_name: &str,
  item: &PlayableItem,
  last_track_id: Option<&str>,
) -> bool {
  if playable_item_name(item) == Some(native_name) {
    return true;
  }

  playable_item_id(item).is_some_and(|api_id| Some(api_id) == last_track_id)
}

#[cfg(feature = "streaming")]
#[derive(Clone, Copy, Debug)]
struct StaleApiItemContext {
  native_info_present: bool,
  api_item_present: bool,
  api_confirms_native_info: bool,
  native_track_id_present: bool,
  api_item_matches_native_track: bool,
  native_streaming_was_active: bool,
  native_activation_pending: bool,
  api_device_is_native: bool,
}

#[cfg(feature = "streaming")]
fn stale_api_item_should_preserve_native_context(context: StaleApiItemContext) -> bool {
  context.api_item_present
    && !context.api_confirms_native_info
    && (context.native_info_present
      || (context.native_track_id_present && !context.api_item_matches_native_track))
    && (context.native_streaming_was_active
      || context.native_activation_pending
      || context.api_device_is_native)
}

#[cfg(feature = "streaming")]
fn native_activation_status_message(device_name: &str) -> String {
  let prefix = "Activating native Spotify device: ";
  let mut message = String::with_capacity(prefix.len() + device_name.len());
  message.push_str(prefix);
  message.push_str(device_name);
  message
}

#[cfg(feature = "streaming")]
fn mark_native_activation_started(
  app: &mut crate::core::app::App,
  session_device_id: String,
  activation_time: Instant,
) {
  app.is_streaming_active = true;
  app.native_activation_pending = true;
  // Librespot exposes the Connect session device id before Spotify's Web API
  // necessarily lists it. Store it immediately so local routing and stale
  // playback contexts can still identify the native target.
  app.native_device_id = Some(session_device_id);
  app.last_device_activation = Some(activation_time);
  app.instant_since_last_current_playback_poll = activation_time - Duration::from_secs(6);
}

#[cfg(feature = "streaming")]
fn mark_native_activation_requested(app: &mut crate::core::app::App, activation_time: Instant) {
  app.is_streaming_active = true;
  app.last_device_activation = Some(activation_time);
  app.instant_since_last_current_playback_poll = activation_time - Duration::from_secs(6);
}

#[cfg(feature = "streaming")]
fn mark_native_activation_confirmed(app: &mut crate::core::app::App, confirmed_device_id: String) {
  app.native_device_id = Some(confirmed_device_id);
  app.native_activation_pending = false;
}

#[cfg(feature = "streaming")]
fn mark_direct_native_transfer_started(
  app: &mut crate::core::app::App,
  session_device_id: String,
  activation_time: Instant,
) {
  mark_native_activation_started(app, session_device_id, activation_time);
  app.native_playback_origin = None;
  // Drop stale previous-device/Sonos state so playback routing follows the
  // native intent until the next Spotify poll repopulates real state.
  app.current_playback_context = None;
  app.selected_sonos_room_uuid = None;
  app.sonos_is_playing = None;
  app.sonos_now_playing = None;
  app.is_volume_change_in_flight = false;
  app.pending_volume = None;
  app.last_dispatched_volume = None;
}

#[cfg(feature = "streaming")]
fn native_device_confirmation<'a>(
  payload: &'a DevicePayload,
  device_name: &str,
  session_device_id: &str,
) -> Option<&'a Device> {
  payload
    .devices
    .iter()
    .find(|device| device.id.as_deref() == Some(session_device_id))
    .or_else(|| {
      payload
        .devices
        .iter()
        .find(|device| device.name.eq_ignore_ascii_case(device_name) && device.is_active)
    })
    .or_else(|| {
      payload
        .devices
        .iter()
        .find(|device| device.name.eq_ignore_ascii_case(device_name))
    })
}

/// Get the currently active streaming player, if any.
/// Note: This logic is duplicated in `main.rs` as `active_streaming_player()`.
/// Both are identical; the difference is input type (Network vs. App Arc).
/// A future refactor could consolidate to a shared location like `src/core/app.rs`.
#[cfg(feature = "streaming")]
async fn current_streaming_player(
  network: &Network,
) -> Option<Arc<crate::infra::player::StreamingPlayer>> {
  let app = network.app.lock().await;
  app.streaming_player.clone()
}

#[cfg(feature = "streaming")]
fn device_names_for_log(payload: &DevicePayload) -> String {
  let mut names = String::new();
  for device in &payload.devices {
    if !names.is_empty() {
      names.push_str(", ");
    }
    names.push_str(&device.name);
  }
  names
}

#[cfg(feature = "streaming")]
fn native_device_names_match(current_name: &str, native_name: &str) -> bool {
  current_name == native_name
    || current_name.eq_ignore_ascii_case(native_name)
    || current_name.to_lowercase() == native_name.to_lowercase()
}

#[cfg(feature = "streaming")]
async fn is_native_streaming_active_for_playback(network: &Network) -> bool {
  let app = network.app.lock().await;
  let streaming_player = app.streaming_player.clone();
  let player_connected = streaming_player.as_ref().is_some_and(|p| p.is_connected());

  if !player_connected {
    return false;
  }

  // If no context yet (e.g., at startup), use the app state flag which is
  // set when the native streaming device is activated/selected.
  let Some(ref ctx) = app.current_playback_context else {
    return app.is_streaming_active;
  };

  // First, check if the current playback device matches the native streaming device ID
  if let (Some(current_id), Some(native_id)) =
    (ctx.device.id.as_ref(), app.native_device_id.as_ref())
  {
    if current_id == native_id {
      return true;
    }
  }

  // Fallback: strict name match (case-insensitive)
  if streaming_player
    .as_ref()
    .is_some_and(|p| native_device_names_match(&ctx.device.name, p.device_name()))
  {
    return true;
  }

  // The user explicitly selected the native device very recently; honor that
  // intent even when the API context hasn't caught up yet (the brief pre-poll
  // window). `is_streaming_active` is re-derived from real Spotify state on the
  // next poll, so this cannot reintroduce the #254 device hijack. (#282)
  if app.is_streaming_active
    && app
      .last_device_activation
      .is_some_and(|instant| instant.elapsed() < Duration::from_secs(5))
  {
    return true;
  }

  // No match - not the active device
  false
}

#[cfg(feature = "streaming")]
async fn requested_native_playback_origin(
  network: &Network,
  context_id: &Option<PlayContextId<'static>>,
  uris: &Option<Vec<PlayableId<'static>>>,
) -> NativePlaybackOrigin {
  if context_id.is_some() {
    return NativePlaybackOrigin::Context;
  }

  if uris.is_some() {
    return NativePlaybackOrigin::RawList;
  }

  let app = network.app.lock().await;
  if let Some(origin) = app.native_playback_origin {
    return origin;
  }

  if app
    .current_playback_context
    .as_ref()
    .and_then(|ctx| ctx.context.as_ref())
    .is_some()
  {
    NativePlaybackOrigin::Context
  } else {
    NativePlaybackOrigin::RawList
  }
}

#[cfg(feature = "streaming")]
async fn resolve_native_playback_route(
  network: &Network,
  context_id: &Option<PlayContextId<'static>>,
) -> NativePlaybackRoute {
  if context_id.is_none() {
    return NativePlaybackRoute::NativeLoad;
  }

  let app = network.app.lock().await;
  match app.native_device_id.clone() {
    Some(device_id) => NativePlaybackRoute::ContextApi { device_id },
    None => NativePlaybackRoute::NativeLoad,
  }
}

impl PlaybackNetwork for Network {
  async fn get_current_playback(&mut self) {
    match selected_sonos_room(self).await {
      SelectedSonosRoom::Room(room) => {
        let result = match crate::infra::sonos::SonosTransport::new() {
          Ok(transport) => transport.now_playing(&room).await,
          Err(e) => Err(e),
        };

        match result {
          Ok(snapshot) => {
            let now_playing = sonos_now_playing_from_snapshot(room.uuid.clone(), snapshot);
            let mut app = self.app.lock().await;
            app.instant_since_last_current_playback_poll = Instant::now();
            app.sonos_volume = now_playing.volume_percent.or(app.sonos_volume);
            app.sonos_is_playing = Some(now_playing.is_playing);
            app.song_progress_ms = now_playing.position_ms as u128;
            app.sonos_now_playing = Some(now_playing);
            app.current_playback_context = None;
            app.is_fetching_current_playback = false;
            return;
          }
          Err(e) => {
            let mut app = self.app.lock().await;
            app.instant_since_last_current_playback_poll = Instant::now();
            app.sonos_is_playing = Some(false);
            app.sonos_now_playing = None;
            app.sonos_volume = None;
            app.is_volume_change_in_flight = false;
            app.pending_volume = None;
            app.last_dispatched_volume = None;
            app.is_fetching_current_playback = false;
            drop(app);
            handle_sonos_error(self, e).await;
            return;
          }
        }
      }
      SelectedSonosRoom::Missing => {
        let mut app = self.app.lock().await;
        app.instant_since_last_current_playback_poll = Instant::now();
        app.is_fetching_current_playback = false;
        return;
      }
      SelectedSonosRoom::None => {}
    }

    // When using native streaming, the Spotify API returns stale server-side state
    // that doesn't reflect recent local changes (volume, shuffle, repeat, play/pause).
    // We need to preserve these local states and restore them after getting the API response.
    #[cfg(feature = "streaming")]
    let streaming_player = current_streaming_player(self).await;
    #[cfg(feature = "streaming")]
    // Check if native streaming is active by examining the pre-fetched player
    // (avoids redundant lock call from is_native_streaming_active)
    let local_state: Option<(Option<u8>, bool, rspotify::model::RepeatState, Option<bool>)> =
      if streaming_player.as_ref().is_some_and(|p| p.is_connected()) {
        let app = self.app.lock().await;
        if let Some(ref ctx) = app.current_playback_context {
          let volume = streaming_player.as_ref().map(|p| p.get_volume());
          Some((
            volume,
            ctx.shuffle_state,
            ctx.repeat_state,
            app.native_is_playing,
          ))
        } else {
          None
        }
      } else {
        None
      };

    let context = self
      .spotify_get_typed::<Option<rspotify::model::CurrentPlaybackContext>>(
        "me/player",
        &[("additional_types", "episode,track".to_string())],
      )
      .await;

    let mut app = self.app.lock().await;

    match context {
      #[allow(unused_mut)]
      Ok(Some(mut c)) => {
        app.instant_since_last_current_playback_poll = Instant::now();

        // Detect whether the native spotatui streaming device is the active Spotify device.
        #[cfg(feature = "streaming")]
        let is_native_device = streaming_player.as_ref().is_some_and(|p| {
          if let (Some(current_id), Some(native_id)) =
            (c.device.id.as_ref(), app.native_device_id.as_ref())
          {
            return current_id == native_id;
          }
          native_device_names_match(&c.device.name, p.device_name())
        });

        #[cfg(feature = "streaming")]
        if is_native_device && app.native_device_id.is_none() {
          if let Some(id) = c.device.id.clone() {
            app.native_device_id = Some(id);
          }
        }

        #[cfg(feature = "streaming")]
        let native_streaming_was_active = app.is_streaming_active;
        #[cfg(feature = "streaming")]
        let native_activation_was_pending = app.native_activation_pending;
        let native_track_id_before_api = app.last_track_id.as_deref();
        #[cfg(feature = "streaming")]
        let native_track_id_present = native_track_id_before_api.is_some();
        #[cfg(feature = "streaming")]
        let api_item_matches_native_track = c
          .item
          .as_ref()
          .and_then(playable_item_id)
          .is_some_and(|api_id| Some(api_id) == native_track_id_before_api);
        let api_item_confirms_native_info = app
          .native_track_info
          .as_ref()
          .zip(c.item.as_ref())
          .is_some_and(|(native_info, item)| {
            api_confirms_native_info_is_current(&native_info.name, item, native_track_id_before_api)
          });
        #[cfg(feature = "streaming")]
        let stale_api_item_for_native =
          stale_api_item_should_preserve_native_context(StaleApiItemContext {
            native_info_present: app.native_track_info.is_some(),
            api_item_present: c.item.is_some(),
            api_confirms_native_info: api_item_confirms_native_info,
            native_track_id_present,
            api_item_matches_native_track,
            native_streaming_was_active,
            native_activation_pending: native_activation_was_pending,
            api_device_is_native: is_native_device,
          });
        #[cfg(not(feature = "streaming"))]
        let stale_api_item_for_native =
          app.native_track_info.is_some() && c.item.is_some() && !api_item_confirms_native_info;

        // Process track info before storing context (avoids cloning)
        if !stale_api_item_for_native {
          if let Some(ref item) = c.item {
            match item {
              PlayableItem::Track(track) => {
                if let Some(ref track_id) = track.id {
                  let track_id_str = track_id.id();

                  // Check if this is a new track
                  if app.last_track_id.as_deref() != Some(track_id_str) {
                    // Trigger lyrics fetch
                    let duration_secs = track.duration.num_seconds() as f64;
                    app.dispatch(IoEvent::GetLyrics(
                      track.name.clone(),
                      create_artist_string(&track.artists),
                      duration_secs,
                    ));

                    app.dispatch(IoEvent::CurrentUserSavedTracksContains(vec![track_id
                      .clone()
                      .into_static()]));
                    app.last_track_id = Some(track_id_str.to_owned());
                  }
                };
              }
              PlayableItem::Episode(_episode) => { /*should map this to following the podcast show*/
              }
              _ => {}
            }
          };
        }

        // Preserve local streaming states (API returns stale server-side state)
        #[cfg(feature = "streaming")]
        if is_native_device {
          if let Some((volume, shuffle, repeat, native_is_playing)) = local_state {
            if let Some(vol) = volume {
              c.device.volume_percent = Some(vol.into());
            }
            c.shuffle_state = shuffle;
            c.repeat_state = repeat;
            // Preserve play/pause state from native player events when available.
            if let Some(is_playing) = native_is_playing {
              c.is_playing = is_playing;
            }
          }
        }

        // Check if Spotify finally caught up to the user's volume change.
        // If the API now returns what the user asked for, we can clear pending_volume
        // and let the API take over again. If not, this response is stale — ignore it.
        if let Some(pending) = app.pending_volume {
          let api_vol = c.device.volume_percent.unwrap_or(0) as u8;
          if api_vol == pending {
            app.pending_volume = None;
            app.last_dispatched_volume = None;
          } else {
            // API hasn't caught up yet — keep showing the user's intended value
            if let Some(ctx) = app.current_playback_context.as_ref() {
              c.device.volume_percent = ctx.device.volume_percent;
            }
          }
        }

        // On first load with native streaming AND native device is active,
        // override API shuffle with saved preference.
        #[cfg(feature = "streaming")]
        if local_state.is_none() && is_native_device {
          c.shuffle_state = app.user_config.behavior.shuffle_enabled;
          // Proactively set native shuffle on first load to keep backend in sync
          if let Some(ref player) = streaming_player {
            let _ = player.set_shuffle(app.user_config.behavior.shuffle_enabled);
          }
        }

        if !stale_api_item_for_native {
          // Get album/episode cover art
          #[cfg(feature = "cover-art")]
          if app
            .user_config
            .do_draw_cover_art(app.cover_art.full_image_support())
          {
            if let Some(playable) = &c.item {
              let image = match playable {
                PlayableItem::Track(t) => t.album.images.first(),
                PlayableItem::Episode(e) => e.images.first(),
                _ => None,
              };

              if let Some(image) = image {
                if let anyhow::Result::Err(err) = app.cover_art.refresh(image).await {
                  drop(app);
                  self.handle_error(err).await;
                  return;
                }
              }
            }
          }

          app.current_playback_context = Some(c);
        }

        // Update is_streaming_active based on whether the current device matches native streaming
        #[cfg(feature = "streaming")]
        {
          if stale_api_item_for_native {
            app.is_streaming_active = true;
            app.native_activation_pending = false;
          } else {
            app.is_streaming_active = is_native_device;
          }

          if is_native_device {
            app.native_activation_pending = false;
          }
        }

        // Keep native metadata authoritative while the native player is active.
        // Spotify's playback endpoint can lag behind librespot by several seconds
        // and report a different item; TrackChanged/Stopped events own this field.
        #[cfg(feature = "streaming")]
        if app.native_track_info.is_some()
          && !stale_api_item_for_native
          && (!is_native_device || api_item_confirms_native_info)
        {
          app.native_track_info = None;
        }
      }
      Ok(None) => {
        app.instant_since_last_current_playback_poll = Instant::now();
      }
      Err(e) => {
        app.is_fetching_current_playback = false;

        let err = anyhow!(e);
        let err_text = err.to_string();
        let err_text_lower = err_text.to_lowercase();

        if err_text.contains("429")
          || err_text.contains("Too Many Requests")
          || err_text.contains("Too many requests")
        {
          app.status_message = Some(
            "Spotify rate limit hit. Retrying automatically; please wait a few seconds."
              .to_string(),
          );
          app.status_message_expires_at = Some(Instant::now() + Duration::from_secs(6));
          app.instant_since_last_current_playback_poll = Instant::now();
          return;
        }

        if err_text_lower.contains("error sending request for url")
          || err_text.contains("connection reset")
          || err_text.contains("connection refused")
          || err_text.contains("timed out")
          || err_text.contains("temporary failure")
          || err_text.contains("dns")
        {
          app.status_message = Some(
            "Temporary Spotify network error while polling playback; retrying automatically."
              .to_string(),
          );
          app.status_message_expires_at = Some(Instant::now() + Duration::from_secs(5));
          app.instant_since_last_current_playback_poll = Instant::now();
          return;
        }

        if err_text.contains("504")
          || err_text.contains("503")
          || err_text.contains("502")
          || err_text.contains("Gateway Timeout")
          || err_text.contains("Service Unavailable")
          || err_text.contains("Bad Gateway")
        {
          app.status_message = Some(
            "Spotify server temporarily unavailable (5xx); retrying automatically.".to_string(),
          );
          app.status_message_expires_at = Some(Instant::now() + Duration::from_secs(10));
          app.instant_since_last_current_playback_poll = Instant::now();
          return;
        }

        // 404 = no active device/player; treat as idle, not an error
        if err_text.contains("404") || err_text.contains("Not Found") {
          app.current_playback_context = None;
          app.instant_since_last_current_playback_poll = Instant::now();
          app.is_fetching_current_playback = false;
          return;
        }

        app.handle_error(err);
        return;
      }
    }

    app.seek_ms.take();
    app.is_fetching_current_playback = false;
  }

  async fn start_playback(
    &mut self,
    context_id: Option<PlayContextId<'static>>,
    uris: Option<Vec<PlayableId<'static>>>,
    offset: Option<usize>,
  ) {
    let (uris, offset) = if context_id.is_none() {
      match uris {
        Some(track_uris) => {
          let (trimmed_uris, trimmed_offset) = trim_api_playback_uris(track_uris, offset);
          (Some(trimmed_uris), trimmed_offset)
        }
        None => (None, offset),
      }
    } else {
      (uris, offset)
    };

    let desired_shuffle_state = {
      let app = self.app.lock().await;
      app
        .current_playback_context
        .as_ref()
        .map(|ctx| ctx.shuffle_state)
        .unwrap_or(app.user_config.behavior.shuffle_enabled)
    };

    match selected_sonos_room(self).await {
      SelectedSonosRoom::Room(room) => {
        let transport = match crate::infra::sonos::SonosTransport::new() {
          Ok(transport) => transport,
          Err(e) => {
            handle_sonos_error(self, e).await;
            return;
          }
        };

        let result = if context_id.is_none() && uris.is_none() {
          transport.play(&room).await
        } else {
          match crate::infra::sonos::spotify::item_from_playback_request(
            context_id.as_ref(),
            uris.as_deref(),
            offset,
          ) {
            Ok(item) => transport.play_spotify_item(&room, &item).await,
            Err(e) => Err(e),
          }
        };

        match result {
          Ok(_) => {
            let mut app = self.app.lock().await;
            app.sonos_is_playing = Some(true);
            app.selected_sonos_room_uuid = Some(room.uuid.clone());
            app.sonos_volume = app
              .sonos_volume
              .or(Some(app.user_config.behavior.volume_percent));
            if context_id.is_some() || uris.is_some() {
              app.sonos_now_playing = None;
              app.song_progress_ms = 0;
            } else if let Some(now_playing) = &mut app.sonos_now_playing {
              now_playing.is_playing = true;
              now_playing.fetched_at = Instant::now();
            }
            app.current_playback_context = None;
            #[cfg(feature = "streaming")]
            {
              app.is_streaming_active = false;
              app.native_playback_origin = None;
            }
            app.set_status_message(format!("Playing on Sonos: {}", room.name), 4);
          }
          Err(e) => {
            let mut app = self.app.lock().await;
            app.sonos_is_playing = Some(false);
            drop(app);
            handle_sonos_error(self, e).await;
          }
        }
        return;
      }
      SelectedSonosRoom::Missing => return,
      SelectedSonosRoom::None => {}
    }

    // Check if we should use native streaming for playback
    #[cfg(feature = "streaming")]
    if is_native_streaming_active_for_playback(self).await {
      if let Some(player) = current_streaming_player(self).await {
        let requested_origin = requested_native_playback_origin(self, &context_id, &uris).await;
        let native_route = resolve_native_playback_route(self, &context_id).await;
        let activation_time = Instant::now();
        let should_transfer = {
          let app = self.app.lock().await;
          let activation_pending = app.native_activation_pending;
          let recent_activation = app
            .last_device_activation
            .is_some_and(|instant| instant.elapsed() < Duration::from_secs(5));
          if activation_pending {
            !recent_activation
          } else {
            !app.is_streaming_active && !recent_activation
          }
        };

        if should_transfer {
          let _ = player.transfer(None);
        }

        player.activate();
        {
          let mut app = self.app.lock().await;
          app.is_streaming_active = true;
          app.last_device_activation = Some(activation_time);
          app.native_activation_pending = false;
          app.native_playback_origin = Some(requested_origin);
        }

        // For resume playback (no context, no uris)
        if context_id.is_none() && uris.is_none() {
          info!("starting native resume playback via direct player route");
          player.play();
          // Update UI state immediately
          let mut app = self.app.lock().await;
          if let Some(ctx) = &mut app.current_playback_context {
            ctx.is_playing = true;
          }
          return;
        }

        if let (NativePlaybackRoute::ContextApi { device_id }, Some(context)) =
          (&native_route, context_id.clone())
        {
          info!(
            "starting native playback via Spotify context route on device {}",
            device_id
          );
          let body = api_playback_body(Some(&context), uris.as_deref(), offset);
          match self
            .spotify_api_request_json(
              Method::PUT,
              "me/player/play",
              &[("device_id", device_id.clone())],
              body,
            )
            .await
          {
            Ok(_) => {
              if let Err(e) = self
                .spotify_api_request_json(
                  Method::PUT,
                  "me/player/shuffle",
                  &[
                    ("state", desired_shuffle_state.to_string()),
                    ("device_id", device_id.clone()),
                  ],
                  None,
                )
                .await
              {
                let mut app = self.app.lock().await;
                app.handle_error(anyhow!(e));
              }

              let mut app = self.app.lock().await;
              app.instant_since_last_current_playback_poll =
                Instant::now() - Duration::from_secs(6);
              if let Some(ctx) = &mut app.current_playback_context {
                ctx.is_playing = true;
                ctx.shuffle_state = desired_shuffle_state;
              }
              app.user_config.behavior.shuffle_enabled = desired_shuffle_state;
              app.dispatch(IoEvent::GetCurrentPlayback);
              return;
            }
            Err(e) => {
              info!(
                "native context playback via Spotify API failed; falling back to direct native load: {}",
                e
              );
            }
          }
        }

        // For URI-based or context playback, use Spirc load directly.
        let mut options = LoadRequestOptions {
          start_playing: true,
          seek_to: 0,
          context_options: None,
          playing_track: None,
        };

        let request = match (context_id, uris) {
          (Some(context), Some(track_uris)) => {
            if let Some(first_uri) = track_uris.first() {
              options.playing_track = Some(PlayingTrack::Uri(first_uri.uri()));
            } else if let Some(i) = offset.and_then(|i| u32::try_from(i).ok()) {
              options.playing_track = Some(PlayingTrack::Index(i));
            }
            LoadRequest::from_context_uri(context.uri(), options)
          }
          (Some(context), None) => {
            if let Some(i) = offset.and_then(|i| u32::try_from(i).ok()) {
              options.playing_track = Some(PlayingTrack::Index(i));
            }
            LoadRequest::from_context_uri(context.uri(), options)
          }
          (None, Some(track_uris)) => {
            if let Some(i) = offset.and_then(|i| u32::try_from(i).ok()) {
              options.playing_track = Some(PlayingTrack::Index(i));
            }
            let uris = track_uris.into_iter().map(|u| u.uri()).collect::<Vec<_>>();
            LoadRequest::from_tracks(uris, options)
          }
          (None, None) => {
            player.play();
            let mut app = self.app.lock().await;
            if let Some(ctx) = &mut app.current_playback_context {
              ctx.is_playing = true;
            }
            return;
          }
        };

        info!("starting native playback via direct load route");
        if let Err(e) = player.load(request) {
          let mut app = self.app.lock().await;
          app.handle_error(anyhow!("Failed to start native playback: {}", e));
        } else {
          let _ = player.set_shuffle(desired_shuffle_state);
          // Optimistic UI update
          let mut app = self.app.lock().await;
          if let Some(ctx) = &mut app.current_playback_context {
            ctx.is_playing = true;
            ctx.shuffle_state = desired_shuffle_state;
          }
          app.user_config.behavior.shuffle_enabled = desired_shuffle_state;
        }
        return;
      }
    }

    let body = api_playback_body(context_id.as_ref(), uris.as_deref(), offset);
    let result = self
      .spotify_api_request_json(Method::PUT, "me/player/play", &[], body)
      .await;

    match result {
      Ok(_) => {
        if let Err(e) = self
          .spotify_api_request_json(
            Method::PUT,
            "me/player/shuffle",
            &[("state", desired_shuffle_state.to_string())],
            None,
          )
          .await
        {
          let mut app = self.app.lock().await;
          app.handle_error(anyhow!(e));
        }

        let mut app = self.app.lock().await;
        if let Some(ctx) = &mut app.current_playback_context {
          ctx.is_playing = true;
          ctx.shuffle_state = desired_shuffle_state;
        }
        app.user_config.behavior.shuffle_enabled = desired_shuffle_state;
      }
      Err(e) => {
        let mut app = self.app.lock().await;
        app.handle_error(anyhow!(e));
      }
    }
  }

  async fn pause_playback(&mut self) {
    match selected_sonos_room(self).await {
      SelectedSonosRoom::Room(room) => {
        match crate::infra::sonos::SonosTransport::new() {
          Ok(transport) => match transport.pause(&room).await {
            Ok(_) => {
              let mut app = self.app.lock().await;
              app.sonos_is_playing = Some(false);
              if let Some(now_playing) = &mut app.sonos_now_playing {
                now_playing.is_playing = false;
              }
              if let Some(ctx) = &mut app.current_playback_context {
                ctx.is_playing = false;
              }
            }
            Err(e) => handle_sonos_error(self, e).await,
          },
          Err(e) => handle_sonos_error(self, e).await,
        }
        return;
      }
      SelectedSonosRoom::Missing => return,
      SelectedSonosRoom::None => {}
    }

    // Check if using native streaming
    #[cfg(feature = "streaming")]
    if is_native_streaming_active_for_playback(self).await {
      if let Some(player) = current_streaming_player(self).await {
        player.pause();
        // Update UI state immediately
        let mut app = self.app.lock().await;
        if let Some(ctx) = &mut app.current_playback_context {
          ctx.is_playing = false;
        }
        return;
      }
    }

    match self
      .spotify_api_request_json(Method::PUT, "me/player/pause", &[], None)
      .await
    {
      Ok(_) => {
        let mut app = self.app.lock().await;
        if let Some(ctx) = &mut app.current_playback_context {
          ctx.is_playing = false;
        }
      }
      Err(e) => {
        let mut app = self.app.lock().await;
        app.handle_error(anyhow!(e));
      }
    }
  }

  async fn next_track(&mut self) {
    match selected_sonos_room(self).await {
      SelectedSonosRoom::Room(room) => {
        match crate::infra::sonos::SonosTransport::new() {
          Ok(transport) => match transport.next(&room).await {
            Ok(_) => {
              let mut app = self.app.lock().await;
              app.song_progress_ms = 0;
              app.sonos_is_playing = Some(true);
              app.sonos_now_playing = None;
            }
            Err(e) => handle_sonos_error(self, e).await,
          },
          Err(e) => handle_sonos_error(self, e).await,
        }
        return;
      }
      SelectedSonosRoom::Missing => return,
      SelectedSonosRoom::None => {}
    }

    #[cfg(feature = "streaming")]
    if is_native_streaming_active_for_playback(self).await {
      if let Some(player) = current_streaming_player(self).await {
        player.next();
        return;
      }
    }

    if let Err(e) = self
      .spotify_api_request_json(Method::POST, "me/player/next", &[], None)
      .await
    {
      let mut app = self.app.lock().await;
      app.handle_error(anyhow!(e));
    }
  }

  async fn previous_track(&mut self) {
    match selected_sonos_room(self).await {
      SelectedSonosRoom::Room(room) => {
        match crate::infra::sonos::SonosTransport::new() {
          Ok(transport) => match transport.previous(&room).await {
            Ok(_) => {
              let mut app = self.app.lock().await;
              app.song_progress_ms = 0;
              app.sonos_is_playing = Some(true);
              app.sonos_now_playing = None;
            }
            Err(e) => {
              let message = e.to_string();
              if message.contains("UPnP error 701") || message.contains("UPnP error 711") {
                if let Err(seek_err) = transport.seek(&room, 0).await {
                  handle_sonos_error(self, seek_err).await;
                }
              } else {
                handle_sonos_error(self, e).await;
              }
            }
          },
          Err(e) => handle_sonos_error(self, e).await,
        }
        return;
      }
      SelectedSonosRoom::Missing => return,
      SelectedSonosRoom::None => {}
    }

    #[cfg(feature = "streaming")]
    if is_native_streaming_active_for_playback(self).await {
      if let Some(player) = current_streaming_player(self).await {
        player.prev();
        return;
      }
    }

    if let Err(e) = self
      .spotify_api_request_json(Method::POST, "me/player/previous", &[], None)
      .await
    {
      let mut app = self.app.lock().await;
      app.handle_error(anyhow!(e));
    }
  }

  async fn force_previous_track(&mut self) {
    match selected_sonos_room(self).await {
      SelectedSonosRoom::Room(room) => {
        match crate::infra::sonos::SonosTransport::new() {
          Ok(transport) => {
            if let Err(e) = transport.previous(&room).await {
              handle_sonos_error(self, e).await;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if let Err(e) = transport.previous(&room).await {
              handle_sonos_error(self, e).await;
            }
            let mut app = self.app.lock().await;
            app.song_progress_ms = 0;
            app.sonos_is_playing = Some(true);
            app.sonos_now_playing = None;
          }
          Err(e) => handle_sonos_error(self, e).await,
        }
        return;
      }
      SelectedSonosRoom::Missing => return,
      SelectedSonosRoom::None => {}
    }

    #[cfg(feature = "streaming")]
    if is_native_streaming_active_for_playback(self).await {
      if let Some(player) = current_streaming_player(self).await {
        player.prev();
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        player.prev();
        return;
      }
    }

    // First previous_track restarts the current track (if past Spotify's ~3s
    // threshold). After a short delay the second call actually skips to the
    // previous track, since the position is now back at 0.
    if let Err(e) = self
      .spotify_api_request_json(Method::POST, "me/player/previous", &[], None)
      .await
    {
      let mut app = self.app.lock().await;
      app.handle_error(anyhow!(e));
      return;
    }

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    if let Err(e) = self
      .spotify_api_request_json(Method::POST, "me/player/previous", &[], None)
      .await
    {
      let mut app = self.app.lock().await;
      app.handle_error(anyhow!(e));
    }
  }

  async fn seek(&mut self, position_ms: u32) {
    match selected_sonos_room(self).await {
      SelectedSonosRoom::Room(room) => {
        match crate::infra::sonos::SonosTransport::new() {
          Ok(transport) => match transport.seek(&room, position_ms).await {
            Ok(_) => {
              let mut app = self.app.lock().await;
              app.song_progress_ms = position_ms as u128;
              if let Some(now_playing) = &mut app.sonos_now_playing {
                now_playing.position_ms = position_ms;
                now_playing.fetched_at = Instant::now();
              }
              app.seek_ms = None;
            }
            Err(e) => handle_sonos_error(self, e).await,
          },
          Err(e) => handle_sonos_error(self, e).await,
        }
        return;
      }
      SelectedSonosRoom::Missing => return,
      SelectedSonosRoom::None => {}
    }

    #[cfg(feature = "streaming")]
    if is_native_streaming_active_for_playback(self).await {
      if let Some(player) = current_streaming_player(self).await {
        player.seek(position_ms);
        return;
      }
    }

    if let Err(e) = self
      .spotify_api_request_json(
        Method::PUT,
        "me/player/seek",
        &[("position_ms", position_ms.to_string())],
        None,
      )
      .await
    {
      let mut app = self.app.lock().await;
      app.handle_error(anyhow!(e));
    }
  }

  async fn shuffle(&mut self, shuffle_state: bool) {
    match selected_sonos_room(self).await {
      SelectedSonosRoom::Room(_) => {
        let mut app = self.app.lock().await;
        if let Some(ctx) = &mut app.current_playback_context {
          ctx.shuffle_state = shuffle_state;
        }
        app.user_config.behavior.shuffle_enabled = shuffle_state;
        app.set_status_message(
          "Sonos shuffle changes are not supported by this integration",
          4,
        );
        return;
      }
      SelectedSonosRoom::Missing => return,
      SelectedSonosRoom::None => {}
    }

    #[cfg(feature = "streaming")]
    if is_native_streaming_active_for_playback(self).await {
      if let Some(player) = current_streaming_player(self).await {
        let _ = player.set_shuffle(shuffle_state);
        let mut app = self.app.lock().await;
        if let Some(ctx) = &mut app.current_playback_context {
          ctx.shuffle_state = shuffle_state;
        }
        return;
      }
    }

    match self
      .spotify_api_request_json(
        Method::PUT,
        "me/player/shuffle",
        &[("state", shuffle_state.to_string())],
        None,
      )
      .await
    {
      Ok(_) => {
        let mut app = self.app.lock().await;
        if let Some(ctx) = &mut app.current_playback_context {
          ctx.shuffle_state = shuffle_state;
        }
      }
      Err(e) => {
        let mut app = self.app.lock().await;
        app.handle_error(anyhow!(e));
      }
    }
  }

  async fn repeat(&mut self, repeat_state: RepeatState) {
    match selected_sonos_room(self).await {
      SelectedSonosRoom::Room(_) => {
        let mut app = self.app.lock().await;
        if let Some(ctx) = &mut app.current_playback_context {
          ctx.repeat_state = repeat_state;
        }
        app.set_status_message(
          "Sonos repeat changes are not supported by this integration",
          4,
        );
        return;
      }
      SelectedSonosRoom::Missing => return,
      SelectedSonosRoom::None => {}
    }

    #[cfg(feature = "streaming")]
    if is_native_streaming_active_for_playback(self).await {
      if let Some(player) = current_streaming_player(self).await {
        let _ = player.set_repeat(repeat_state);
        let mut app = self.app.lock().await;
        if let Some(ctx) = &mut app.current_playback_context {
          ctx.repeat_state = repeat_state;
        }
        return;
      }
    }

    let repeat_state_param: &'static str = repeat_state.into();
    match self
      .spotify_api_request_json(
        Method::PUT,
        "me/player/repeat",
        &[("state", repeat_state_param.to_string())],
        None,
      )
      .await
    {
      Ok(_) => {
        let mut app = self.app.lock().await;
        if let Some(ctx) = &mut app.current_playback_context {
          ctx.repeat_state = repeat_state;
        }
      }
      Err(e) => {
        let mut app = self.app.lock().await;
        app.handle_error(anyhow!(e));
      }
    }
  }

  /// Sends the volume change to Spotify, either through the native streaming
  /// player or the Web API depending on which device is active.
  ///
  /// On success we clear the in-flight flag but keep `pending_volume` around.
  /// It only gets cleared when `get_current_playback` comes back with a matching
  /// volume — that's our signal that Spotify actually caught up.
  ///
  /// On error we bail and clear everything so the UI falls back to whatever
  /// the API last reported.
  async fn change_volume(&mut self, volume: u8) {
    match selected_sonos_room(self).await {
      SelectedSonosRoom::Room(room) => {
        match crate::infra::sonos::SonosTransport::new() {
          Ok(transport) => match transport.set_volume(&room, volume).await {
            Ok(_) => {
              let mut app = self.app.lock().await;
              let queued_volume = app.pending_volume.filter(|pending| *pending != volume);
              let visible_volume = queued_volume.unwrap_or(volume);
              app.sonos_volume = Some(visible_volume);
              if let Some(now_playing) = &mut app.sonos_now_playing {
                now_playing.volume_percent = Some(visible_volume);
              }

              if let Some(next_volume) = queued_volume {
                app.is_volume_change_in_flight = true;
                app.pending_volume = Some(next_volume);
                app.last_dispatched_volume = Some(next_volume);
                app.dispatch(IoEvent::ChangeVolume(next_volume));
              } else {
                app.is_volume_change_in_flight = false;
                app.pending_volume = None;
                app.last_dispatched_volume = Some(volume);
              }
            }
            Err(e) => {
              let mut app = self.app.lock().await;
              app.sonos_volume = app
                .sonos_now_playing
                .as_ref()
                .and_then(|now_playing| now_playing.volume_percent);
              app.is_volume_change_in_flight = false;
              app.pending_volume = None;
              app.last_dispatched_volume = None;
              drop(app);
              handle_sonos_error(self, e).await;
            }
          },
          Err(e) => {
            let mut app = self.app.lock().await;
            app.sonos_volume = app
              .sonos_now_playing
              .as_ref()
              .and_then(|now_playing| now_playing.volume_percent);
            app.is_volume_change_in_flight = false;
            app.pending_volume = None;
            app.last_dispatched_volume = None;
            drop(app);
            handle_sonos_error(self, e).await;
          }
        }
        return;
      }
      SelectedSonosRoom::Missing => return,
      SelectedSonosRoom::None => {}
    }

    #[cfg(feature = "streaming")]
    if is_native_streaming_active_for_playback(self).await {
      if let Some(player) = current_streaming_player(self).await {
        player.set_volume(volume);
        let mut app = self.app.lock().await;
        if let Some(ctx) = &mut app.current_playback_context {
          ctx.device.volume_percent = Some(volume.into());
        }
        app.is_volume_change_in_flight = false;
        app.last_dispatched_volume = Some(volume);
        // Keep pending_volume set — cleared when API confirms the value matches
        return;
      }
    }

    match self
      .spotify_api_request_json(
        Method::PUT,
        "me/player/volume",
        &[("volume_percent", volume.to_string())],
        None,
      )
      .await
    {
      Ok(_) => {
        let mut app = self.app.lock().await;
        if let Some(ctx) = &mut app.current_playback_context {
          ctx.device.volume_percent = Some(volume.into());
        }
        app.is_volume_change_in_flight = false;
        app.last_dispatched_volume = Some(volume);
        // Keep pending_volume set — cleared when get_current_playback confirms
      }
      Err(e) => {
        let mut app = self.app.lock().await;
        app.is_volume_change_in_flight = false;
        app.pending_volume = None;
        app.last_dispatched_volume = None;
        app.handle_error(anyhow!(e));
      }
    }
  }

  async fn transfert_playback_to_device(&mut self, device_id: String, persist_device_id: bool) {
    if let Some(room_uuid) = parse_sonos_persisted_id(&device_id).map(ToOwned::to_owned) {
      self
        .transfer_playback_to_sonos_room(room_uuid, persist_device_id)
        .await;
      return;
    }

    let selected_sonos_uuid = {
      let app = self.app.lock().await;
      app.selected_sonos_room_uuid.clone()
    };
    if let Some(room_uuid) = selected_sonos_uuid {
      if let Some(room) = sonos_room_by_uuid(self, &room_uuid).await {
        if let Ok(transport) = crate::infra::sonos::SonosTransport::new() {
          let _ = transport.pause(&room).await;
        }
      }
    }

    #[cfg(feature = "streaming")]
    {
      let streaming_player = current_streaming_player(self).await;
      let is_native_transfer = if let Some(ref player) = streaming_player {
        let native_name = player.device_name();
        let app = self.app.lock().await;
        let matches_cached_device = app.devices.as_ref().is_some_and(|payload| {
          payload.devices.iter().any(|d| {
            d.id.as_ref() == Some(&device_id) && native_device_names_match(&d.name, native_name)
          })
        });
        matches_cached_device || app.native_device_id.as_ref() == Some(&device_id)
      } else {
        false
      };

      if is_native_transfer {
        if let Some(ref player) = streaming_player {
          let activation_time = Instant::now();
          let session_device_id = player.device_id();
          info!(
            "transferring to native streaming device selected_id={} session_device_id={} connected={} persist={}",
            device_id,
            session_device_id,
            player.is_connected(),
            persist_device_id
          );
          if let Err(error) = player.transfer(None) {
            info!("native streaming direct transfer failed: {}", error);
          }
          player.activate();
          let persist_result = if persist_device_id {
            self.client_config.set_device_id(device_id.clone()).err()
          } else {
            None
          };
          let mut app = self.app.lock().await;
          mark_direct_native_transfer_started(&mut app, session_device_id, activation_time);
          app.set_status_message(native_activation_status_message(player.device_name()), 4);
          if let Some(error) = persist_result {
            info!(
              "failed to persist native streaming device selection: {}",
              error
            );
            app.handle_error(anyhow!(error));
          }
          return;
        }
      }
    }

    if let Err(e) = self
      .spotify_api_request_json(
        Method::PUT,
        "me/player",
        &[],
        Some(json!({
          "device_ids": [device_id.clone()],
          "play": true
        })),
      )
      .await
    {
      let mut app = self.app.lock().await;
      app.handle_error(anyhow!(e));
    } else {
      let mut app = self.app.lock().await;
      if persist_device_id {
        // Update via client_config helper to save to file
        if let Err(e) = self.client_config.set_device_id(device_id) {
          app.handle_error(anyhow!(e));
        }
      }
      app.current_playback_context = None;
      app.selected_sonos_room_uuid = None;
      app.sonos_is_playing = None;
      app.sonos_now_playing = None;
      app.is_volume_change_in_flight = false;
      app.pending_volume = None;
      app.last_dispatched_volume = None;

      #[cfg(feature = "streaming")]
      {
        // If transferring away from native, update flag
        app.is_streaming_active = false;
        app.native_playback_origin = None;
      }
    }
  }

  async fn transfer_playback_to_sonos_room(&mut self, room_uuid: String, persist_device_id: bool) {
    let room = match sonos_room_by_uuid(self, &room_uuid).await {
      Some(room) => Some(room),
      None => match refresh_sonos_rooms(self).await {
        Ok(rooms) => rooms.into_iter().find(|room| room.uuid == room_uuid),
        Err(e) => {
          let mut app = self.app.lock().await;
          app.set_status_message(format!("Could not discover Sonos rooms: {e}"), 6);
          None
        }
      },
    };

    let Some(room) = room else {
      if persist_device_id {
        let persisted_id = sonos_persisted_id(&room_uuid);
        if let Err(e) = self.client_config.set_device_id(persisted_id) {
          let mut app = self.app.lock().await;
          app.handle_error(anyhow!(e));
          return;
        }
      }

      let mut app = self.app.lock().await;
      app.selected_sonos_room_uuid = Some(room_uuid);
      app.sonos_is_playing = Some(false);
      app.sonos_now_playing = None;
      app.current_playback_context = None;
      #[cfg(feature = "streaming")]
      {
        app.is_streaming_active = false;
        app.native_playback_origin = None;
        app.native_activation_pending = false;
      }
      app.set_status_message(
        "Saved Sonos room unavailable. Check that it is powered on and on this network.",
        6,
      );
      return;
    };

    let (sonos_now_playing, current_sonos_volume) = match crate::infra::sonos::SonosTransport::new()
    {
      Ok(transport) => match transport.now_playing(&room).await {
        Ok(snapshot) => {
          let now_playing = sonos_now_playing_from_snapshot(room.uuid.clone(), snapshot);
          let volume = now_playing.volume_percent;
          (Some(now_playing), volume)
        }
        Err(_) => (None, transport.volume(&room).await.ok()),
      },
      Err(_) => (None, None),
    };

    {
      let mut app = self.app.lock().await;
      app.selected_sonos_room_uuid = Some(room.uuid.clone());
      app.sonos_volume = current_sonos_volume.or(Some(app.user_config.behavior.volume_percent));
      app.is_volume_change_in_flight = false;
      app.pending_volume = None;
      app.last_dispatched_volume = None;
      app.sonos_is_playing = Some(
        sonos_now_playing
          .as_ref()
          .is_some_and(|now_playing| now_playing.is_playing),
      );
      if let Some(now_playing) = sonos_now_playing {
        app.song_progress_ms = now_playing.position_ms as u128;
        app.sonos_now_playing = Some(now_playing);
      } else {
        app.sonos_now_playing = None;
      }
      app.current_playback_context = None;
      #[cfg(feature = "streaming")]
      {
        app.is_streaming_active = false;
        app.native_playback_origin = None;
        app.native_activation_pending = false;
      }
    }

    if persist_device_id {
      let persisted_id = sonos_persisted_id(&room_uuid);
      if let Err(e) = self.client_config.set_device_id(persisted_id) {
        let mut app = self.app.lock().await;
        app.handle_error(anyhow!(e));
        return;
      }
    }

    let mut app = self.app.lock().await;
    app.set_status_message(format!("Selected Sonos room: {}", room.name), 4);
  }

  #[cfg(feature = "streaming")]
  async fn auto_select_streaming_device(&mut self, device_name: String, persist_device_id: bool) {
    tokio::time::sleep(Duration::from_millis(200)).await;

    if let Some(player) = current_streaming_player(self).await {
      let activation_time = Instant::now();
      let session_device_id = player.device_id();
      info!(
        "auto-selecting native streaming device name='{}' session_device_id={} connected={} persist={}",
        device_name,
        session_device_id,
        player.is_connected(),
        persist_device_id
      );
      let should_transfer = {
        let app = self.app.lock().await;
        let recent_activation = app
          .last_device_activation
          .is_some_and(|instant| instant.elapsed() < Duration::from_secs(5));
        !app.native_activation_pending && !app.is_streaming_active && !recent_activation
      };

      {
        let mut app = self.app.lock().await;
        mark_native_activation_started(&mut app, session_device_id.clone(), activation_time);
        app.set_status_message(native_activation_status_message(&device_name), 4);
      }

      if should_transfer {
        info!("native streaming auto-select: issuing Spirc transfer before activate");
        if let Err(error) = player.transfer(None) {
          info!("native streaming auto-select transfer failed: {}", error);
        }
      } else {
        info!("native streaming auto-select: recent/active activation present; skipping transfer");
      }
      player.activate();

      {
        let mut app = self.app.lock().await;
        mark_native_activation_requested(&mut app, activation_time);
      }

      const DEVICE_APPEARANCE_ATTEMPTS: usize = 8;
      for attempt in 0..DEVICE_APPEARANCE_ATTEMPTS {
        if attempt > 0 {
          tokio::time::sleep(Duration::from_millis(250)).await;
        }

        match self
          .spotify_get_typed::<DevicePayload>("me/player/devices", &[])
          .await
        {
          Ok(payload) => {
            if let Some(device) =
              native_device_confirmation(&payload, &device_name, &session_device_id)
            {
              if let Some(id) = &device.id {
                info!(
                  "native streaming device appeared in Spotify devices on attempt {}: id={} name='{}' active={}",
                  attempt + 1,
                  id,
                  device.name,
                  device.is_active
                );
                if persist_device_id {
                  let _ = self.client_config.set_device_id(id.clone());
                }
                let mut app = self.app.lock().await;
                mark_native_activation_confirmed(&mut app, id.clone());
                return;
              }
            }
            info!(
              "native streaming device '{}' not present in Spotify devices on attempt {}; seen devices: {}",
              device_name,
              attempt + 1,
              device_names_for_log(&payload)
            );
          }
          Err(error) => {
            info!(
              "failed to fetch Spotify devices while confirming native streaming auto-select on attempt {}: {}",
              attempt + 1,
              error
            );
            continue;
          }
        }
      }

      info!(
        "native streaming device '{}' was not confirmed by Spotify devices after {} attempts; keeping session_device_id={} as native target and activation pending",
        device_name,
        DEVICE_APPEARANCE_ATTEMPTS,
        session_device_id
      );
      let mut app = self.app.lock().await;
      app.set_status_message(
        format!(
          "Native Spotify device not confirmed yet; using local session id for {device_name}"
        ),
        8,
      );
    } else {
      info!(
        "native streaming auto-select requested for '{}' but no streaming player is available",
        device_name
      );
      let mut app = self.app.lock().await;
      app.set_status_message("Native streaming player is not available", 6);
    }
  }

  async fn ensure_playback_continues(&mut self, previous_track_id: String) {
    // Native streaming normally advances by itself, but librespot can report a
    // stopped EndOfTrack event after single-item direct loads. Keep the same
    // recovery heuristic enabled for native so playlist/saved-track playback
    // can advance instead of stopping after one song.

    // Check if we are paused/stopped
    let context = self
      .spotify_get_typed::<Option<rspotify::model::CurrentPlaybackContext>>("me/player", &[])
      .await;

    if let Ok(Some(ctx)) = context {
      if !ctx.is_playing {
        // If we are stopped but shouldn't be (e.g. track finished), try to skip to next
        // Use a heuristic: if the current item is the SAME as the previous one and we are at 0:00,
        // it might mean Spotify stopped. Or if we are just null.
        if let Some(item) = ctx.item {
          let current_id = match item {
            PlayableItem::Track(t) => t.id.map(|id| id.id().to_string()),
            PlayableItem::Episode(e) => Some(e.id.id().to_string()),
            _ => None,
          };

          if current_id == Some(previous_track_id)
            && ctx
              .progress
              .map(|d: TimeDelta| d.num_milliseconds())
              .unwrap_or(0)
              == 0
          {
            self.next_track().await;
          }
        }
      }
    }
  }

  async fn add_item_to_queue(&mut self, item: PlayableId<'static>) {
    match selected_sonos_room(self).await {
      SelectedSonosRoom::Room(room) => {
        let result = match crate::infra::sonos::SonosTransport::new() {
          Ok(transport) => {
            let uri = item.uri();
            match crate::infra::sonos::spotify::item_from_spotify_uri(&uri) {
              Ok(sonos_item) => transport
                .enqueue_spotify_item(&room, &sonos_item)
                .await
                .map(|_| ()),
              Err(e) => Err(e),
            }
          }
          Err(e) => Err(e),
        };

        match result {
          Ok(_) => {
            let mut app = self.app.lock().await;
            app.set_status_message("Added to Sonos queue", 3);
          }
          Err(e) => handle_sonos_error(self, e).await,
        }
        return;
      }
      SelectedSonosRoom::Missing => return,
      SelectedSonosRoom::None => {}
    }

    match self
      .spotify_api_request_json(
        Method::POST,
        "me/player/queue",
        &[("uri", item.uri())],
        None,
      )
      .await
    {
      Ok(_) => {
        let mut app = self.app.lock().await;
        app.status_message = Some("Added to queue".to_string());
        app.status_message_expires_at = Some(Instant::now() + Duration::from_secs(3));
      }
      Err(e) => {
        let mut app = self.app.lock().await;
        app.handle_error(anyhow!(e));
      }
    }
  }

  async fn get_queue(&mut self) {
    match selected_sonos_room(self).await {
      SelectedSonosRoom::Room(_) => {
        let mut app = self.app.lock().await;
        app.queue = None;
        app.set_status_message("Sonos queue viewing is not supported yet", 3);
        return;
      }
      SelectedSonosRoom::Missing => return,
      SelectedSonosRoom::None => {}
    }

    match self
      .spotify_get_typed::<CurrentUserQueue>("me/player/queue", &[])
      .await
    {
      Ok(q) => {
        let mut app = self.app.lock().await;
        app.queue = Some(q);
      }
      Err(e) => {
        let mut app = self.app.lock().await;
        app.queue = None;
        app.status_message = Some("Could not load queue (no active device?)".to_string());
        app.status_message_expires_at = Some(Instant::now() + Duration::from_secs(3));
        log::warn!("get_queue failed: {}", e);
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use rspotify::model::{
    artist::SimplifiedArtist, idtypes::TrackId, track::FullTrack, SimplifiedAlbum,
  };
  #[cfg(feature = "streaming")]
  use rspotify::model::{
    context::{Actions, CurrentPlaybackContext},
    device::Device,
    enums::{CurrentlyPlayingType, RepeatState},
    DeviceType,
  };
  use rspotify::prelude::Id;
  use std::collections::HashMap;

  fn playable_track(id: &str) -> PlayableId<'static> {
    PlayableId::Track(TrackId::from_id(id).unwrap().into_static())
  }

  #[allow(deprecated)]
  fn full_track(id: &str, name: &str) -> PlayableItem {
    PlayableItem::Track(FullTrack {
      album: SimplifiedAlbum {
        name: "Album".to_string(),
        ..Default::default()
      },
      artists: vec![SimplifiedArtist {
        name: "Artist".to_string(),
        ..Default::default()
      }],
      available_markets: Vec::new(),
      disc_number: 1,
      duration: TimeDelta::milliseconds(180_000),
      explicit: false,
      external_ids: HashMap::new(),
      external_urls: HashMap::new(),
      href: None,
      id: Some(TrackId::from_id(id).unwrap().into_static()),
      is_local: false,
      is_playable: Some(true),
      linked_from: None,
      restrictions: None,
      name: name.to_string(),
      popularity: 50,
      preview_url: None,
      track_number: 1,
      r#type: rspotify::model::Type::Track,
    })
  }

  #[cfg(feature = "streaming")]
  #[allow(deprecated)]
  fn spotify_device(id: &str, name: &str, is_active: bool) -> Device {
    Device {
      id: Some(id.to_string()),
      is_active,
      is_private_session: false,
      is_restricted: false,
      name: name.to_string(),
      _type: DeviceType::Computer,
      volume_percent: Some(50),
    }
  }

  #[cfg(feature = "streaming")]
  #[allow(deprecated)]
  fn stale_playback_context() -> CurrentPlaybackContext {
    CurrentPlaybackContext {
      device: spotify_device("old-device", "Old Device", true),
      repeat_state: RepeatState::Off,
      shuffle_state: false,
      context: None,
      timestamp: chrono::Utc::now(),
      progress: None,
      is_playing: true,
      item: Some(full_track("0000000000000000000001", "Old Track")),
      currently_playing_type: CurrentlyPlayingType::Track,
      actions: Actions::default(),
    }
  }

  #[test]
  fn trim_api_playback_uris_leaves_small_requests_unchanged() {
    let uris = vec![
      playable_track("0000000000000000000001"),
      playable_track("0000000000000000000002"),
    ];

    let (trimmed, offset) = trim_api_playback_uris(uris.clone(), Some(1));

    assert_eq!(trimmed, uris);
    assert_eq!(offset, Some(1));
  }

  #[test]
  fn trim_api_playback_uris_keeps_selected_track_inside_window() {
    let uris = (0..150)
      .map(|index| playable_track(&format!("{index:022}")))
      .collect::<Vec<_>>();

    let (trimmed, offset) = trim_api_playback_uris(uris.clone(), Some(60));

    assert_eq!(trimmed.len(), MAX_API_PLAYBACK_URIS);
    assert_eq!(offset, Some(20));
    assert_eq!(trimmed[offset.unwrap()].uri(), uris[60].uri());
  }

  #[test]
  fn trim_api_playback_uris_slides_window_near_end() {
    let uris = (0..150)
      .map(|index| playable_track(&format!("{index:022}")))
      .collect::<Vec<_>>();

    let (trimmed, offset) = trim_api_playback_uris(uris.clone(), Some(149));

    assert_eq!(trimmed.len(), MAX_API_PLAYBACK_URIS);
    assert_eq!(offset, Some(99));
    assert_eq!(trimmed[offset.unwrap()].uri(), uris[149].uri());
  }

  #[test]
  fn api_playback_offset_uses_track_uri_for_context_playback() {
    let uris = vec![
      playable_track("0000000000000000000001"),
      playable_track("0000000000000000000002"),
    ];

    let offset = api_playback_offset_json(Some(&uris), Some(1));

    assert_eq!(
      offset,
      Some(json!({ "uri": "spotify:track:0000000000000000000001" }))
    );
  }

  #[test]
  fn api_playback_offset_uses_position_for_uri_list_playback() {
    let offset = api_playback_offset_json(None, Some(1));

    assert_eq!(offset, Some(json!({ "position": 1 })));
  }

  #[test]
  fn api_playback_offset_falls_back_to_position_when_context_has_no_uri() {
    let offset = api_playback_offset_json(None, Some(3));

    assert_eq!(offset, Some(json!({ "position": 3 })));
  }

  #[test]
  fn api_confirms_native_info_when_names_match() {
    let item = full_track("0000000000000000000001", "Current Song");

    assert!(api_confirms_native_info_is_current(
      "Current Song",
      &item,
      Some("different-id")
    ));
  }

  #[test]
  fn api_confirms_native_info_when_current_id_matches_even_if_name_differs() {
    let item = full_track("0000000000000000000001", "Stranger Thing");

    assert!(api_confirms_native_info_is_current(
      "Greater Together",
      &item,
      Some("0000000000000000000001")
    ));
  }

  #[test]
  fn api_does_not_confirm_stale_api_item_for_different_native_track() {
    let item = full_track("0000000000000000000001", "Old API Song");

    assert!(!api_confirms_native_info_is_current(
      "New Native Song",
      &item,
      Some("0000000000000000000002")
    ));
  }

  #[cfg(feature = "streaming")]
  fn test_app() -> crate::core::app::App {
    let (tx, _rx) = std::sync::mpsc::channel();
    crate::core::app::App::new(
      tx,
      crate::core::user_config::UserConfig::new(),
      std::time::SystemTime::now(),
    )
  }

  #[cfg(feature = "streaming")]
  #[test]
  fn device_names_for_log_joins_without_trailing_separator() {
    let payload = DevicePayload {
      devices: vec![
        spotify_device("device-a", "spotatui", false),
        spotify_device("device-b", "Kitchen", true),
      ],
    };

    assert_eq!(device_names_for_log(&payload), "spotatui, Kitchen");
  }

  #[cfg(feature = "streaming")]
  #[test]
  fn native_device_names_match_without_allocating_for_exact_or_ascii_case() {
    assert!(native_device_names_match("spotatui", "spotatui"));
    assert!(native_device_names_match("Spotatui", "spotatui"));
    assert!(native_device_names_match("Spötatui", "spötatui"));
    assert!(!native_device_names_match("Other", "spotatui"));
  }

  #[cfg(feature = "streaming")]
  #[test]
  fn native_activation_status_message_names_device() {
    assert_eq!(
      native_activation_status_message("spotatui"),
      "Activating native Spotify device: spotatui"
    );
  }

  #[cfg(feature = "streaming")]
  #[test]
  fn native_activation_start_stores_session_device_id_while_pending() {
    let mut app = test_app();
    let activation_time = Instant::now();

    mark_native_activation_started(&mut app, "session-device-id".to_string(), activation_time);

    assert!(app.is_streaming_active);
    assert!(app.native_activation_pending);
    assert_eq!(app.native_device_id.as_deref(), Some("session-device-id"));
    assert_eq!(app.last_device_activation, Some(activation_time));
  }

  #[cfg(feature = "streaming")]
  #[test]
  fn native_activation_confirmation_replaces_session_id_and_clears_pending() {
    let mut app = test_app();
    let activation_time = Instant::now();
    mark_native_activation_started(&mut app, "session-device-id".to_string(), activation_time);

    mark_native_activation_confirmed(&mut app, "spotify-web-api-device-id".to_string());

    assert!(app.is_streaming_active);
    assert!(!app.native_activation_pending);
    assert_eq!(
      app.native_device_id.as_deref(),
      Some("spotify-web-api-device-id")
    );
  }

  #[cfg(feature = "streaming")]
  #[test]
  fn native_activation_request_keeps_pending_state_until_device_confirmation() {
    let mut app = test_app();
    let activation_time = Instant::now();
    mark_native_activation_started(&mut app, "session-device-id".to_string(), activation_time);

    mark_native_activation_requested(&mut app, activation_time);

    assert!(app.is_streaming_active);
    assert!(app.native_activation_pending);
    assert_eq!(app.native_device_id.as_deref(), Some("session-device-id"));
  }

  #[cfg(feature = "streaming")]
  #[test]
  fn direct_native_transfer_resets_stale_context_and_sonos_state() {
    let mut app = test_app();
    let activation_time = Instant::now();
    app.current_playback_context = Some(stale_playback_context());
    app.selected_sonos_room_uuid = Some("sonos-room".to_string());
    app.sonos_is_playing = Some(true);
    app.pending_volume = Some(42);
    app.last_dispatched_volume = Some(42);
    app.is_volume_change_in_flight = true;
    app.native_playback_origin = Some(NativePlaybackOrigin::Context);

    mark_direct_native_transfer_started(&mut app, "session-device-id".to_string(), activation_time);

    assert!(app.is_streaming_active);
    assert!(app.native_activation_pending);
    assert_eq!(app.native_device_id.as_deref(), Some("session-device-id"));
    assert_eq!(app.last_device_activation, Some(activation_time));
    assert!(app.current_playback_context.is_none());
    assert!(app.selected_sonos_room_uuid.is_none());
    assert!(app.sonos_is_playing.is_none());
    assert!(app.pending_volume.is_none());
    assert!(app.last_dispatched_volume.is_none());
    assert!(!app.is_volume_change_in_flight);
    assert!(app.native_playback_origin.is_none());
  }

  #[cfg(feature = "streaming")]
  #[test]
  fn native_device_confirmation_prefers_session_device_id_over_same_name() {
    let payload = DevicePayload {
      devices: vec![
        spotify_device("same-name-device", "spotatui", true),
        spotify_device("session-device-id", "renamed spotatui", false),
      ],
    };

    let confirmed = native_device_confirmation(&payload, "spotatui", "session-device-id");

    assert_eq!(
      confirmed.and_then(|device| device.id.as_deref()),
      Some("session-device-id")
    );
  }

  #[cfg(feature = "streaming")]
  #[test]
  fn native_device_confirmation_prefers_active_same_name_when_session_id_absent() {
    let payload = DevicePayload {
      devices: vec![
        spotify_device("stale-same-name", "spotatui", false),
        spotify_device("active-same-name", "SPOTATUI", true),
      ],
    };

    let confirmed = native_device_confirmation(&payload, "spotatui", "session-device-id");

    assert_eq!(
      confirmed.and_then(|device| device.id.as_deref()),
      Some("active-same-name")
    );
  }

  #[cfg(feature = "streaming")]
  #[test]
  fn stale_api_item_keeps_native_metadata_when_native_was_active() {
    assert!(stale_api_item_should_preserve_native_context(
      StaleApiItemContext {
        native_info_present: true,
        api_item_present: true,
        api_confirms_native_info: false,
        native_track_id_present: true,
        api_item_matches_native_track: false,
        native_streaming_was_active: true,
        native_activation_pending: false,
        api_device_is_native: false,
      },
    ));
  }

  #[cfg(feature = "streaming")]
  #[test]
  fn stale_api_item_keeps_native_metadata_during_activation() {
    assert!(stale_api_item_should_preserve_native_context(
      StaleApiItemContext {
        native_info_present: true,
        api_item_present: true,
        api_confirms_native_info: false,
        native_track_id_present: true,
        api_item_matches_native_track: false,
        native_streaming_was_active: false,
        native_activation_pending: true,
        api_device_is_native: false,
      },
    ));
  }

  #[cfg(feature = "streaming")]
  #[test]
  fn stale_api_item_keeps_native_context_before_native_metadata_arrives() {
    assert!(stale_api_item_should_preserve_native_context(
      StaleApiItemContext {
        native_info_present: false,
        api_item_present: true,
        api_confirms_native_info: false,
        native_track_id_present: true,
        api_item_matches_native_track: false,
        native_streaming_was_active: true,
        native_activation_pending: false,
        api_device_is_native: false,
      },
    ));
  }

  #[cfg(feature = "streaming")]
  #[test]
  fn stale_native_metadata_clears_after_playback_leaves_native_device() {
    assert!(!stale_api_item_should_preserve_native_context(
      StaleApiItemContext {
        native_info_present: true,
        api_item_present: true,
        api_confirms_native_info: false,
        native_track_id_present: true,
        api_item_matches_native_track: false,
        native_streaming_was_active: false,
        native_activation_pending: false,
        api_device_is_native: false,
      },
    ));
  }

  #[cfg(feature = "streaming")]
  #[test]
  fn confirmed_api_item_no_longer_keeps_native_metadata() {
    assert!(!stale_api_item_should_preserve_native_context(
      StaleApiItemContext {
        native_info_present: true,
        api_item_present: true,
        api_confirms_native_info: true,
        native_track_id_present: true,
        api_item_matches_native_track: true,
        native_streaming_was_active: true,
        native_activation_pending: false,
        api_device_is_native: true,
      },
    ));
  }

  #[cfg(feature = "streaming")]
  #[test]
  fn matching_api_item_without_native_metadata_can_update_context() {
    assert!(!stale_api_item_should_preserve_native_context(
      StaleApiItemContext {
        native_info_present: false,
        api_item_present: true,
        api_confirms_native_info: false,
        native_track_id_present: true,
        api_item_matches_native_track: true,
        native_streaming_was_active: true,
        native_activation_pending: false,
        api_device_is_native: false,
      },
    ));
  }

  #[cfg(feature = "streaming")]
  #[test]
  fn api_item_without_native_track_id_can_update_context() {
    assert!(!stale_api_item_should_preserve_native_context(
      StaleApiItemContext {
        native_info_present: false,
        api_item_present: true,
        api_confirms_native_info: false,
        native_track_id_present: false,
        api_item_matches_native_track: false,
        native_streaming_was_active: true,
        native_activation_pending: false,
        api_device_is_native: false,
      },
    ));
  }
}
