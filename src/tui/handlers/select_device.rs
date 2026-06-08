use super::common_key_events;
use crate::core::app::{ActiveBlock, App};
use crate::core::playback_target::PlaybackTarget;
use crate::infra::network::IoEvent;
use crate::tui::event::Key;

pub fn handler(key: Key, app: &mut App) {
  match key {
    Key::Esc => {
      app.set_current_route_state(Some(ActiveBlock::Library), None);
    }
    k if common_key_events::down_event(k) => {
      let targets = app.playback_targets();
      if !targets.is_empty() {
        if let Some(selected_device_index) = app.selected_device_index {
          let next_index =
            common_key_events::on_down_press_handler(&targets, Some(selected_device_index));
          app.selected_device_index = Some(next_index);
        }
      }
    }
    k if common_key_events::up_event(k) => {
      let targets = app.playback_targets();
      if !targets.is_empty() {
        if let Some(selected_device_index) = app.selected_device_index {
          let next_index =
            common_key_events::on_up_press_handler(&targets, Some(selected_device_index));
          app.selected_device_index = Some(next_index);
        }
      }
    }
    k if common_key_events::high_event(k) => {
      if !app.playback_targets().is_empty() {
        if let Some(_selected_device_index) = app.selected_device_index {
          let next_index = common_key_events::on_high_press_handler();
          app.selected_device_index = Some(next_index);
        }
      }
    }
    k if common_key_events::middle_event(k) => {
      let targets = app.playback_targets();
      if !targets.is_empty() {
        if let Some(_selected_device_index) = app.selected_device_index {
          let next_index = common_key_events::on_middle_press_handler(&targets);
          app.selected_device_index = Some(next_index);
        }
      }
    }
    k if common_key_events::low_event(k) => {
      let targets = app.playback_targets();
      if !targets.is_empty() {
        if let Some(_selected_device_index) = app.selected_device_index {
          let next_index = common_key_events::on_low_press_handler(&targets);
          app.selected_device_index = Some(next_index);
        }
      }
    }
    Key::Enter => {
      if let Some(index) = app.selected_device_index {
        if let Some(target) = app.playback_targets().get(index) {
          match target {
            PlaybackTarget::Spotify { id, .. } => {
              app.dispatch(IoEvent::TransferPlaybackToDevice(id.clone(), true));
            }
            PlaybackTarget::Sonos { room, .. } => {
              app.dispatch(IoEvent::TransferPlaybackToSonosRoom(
                room.uuid.clone(),
                true,
              ));
            }
          }
        }
      }
    }
    _ => {}
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::playback_target::SonosRoom;
  use crate::core::user_config::UserConfig;

  #[test]
  fn enter_dispatches_selected_sonos_room() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut app = App::new(tx, UserConfig::new(), std::time::SystemTime::now());
    app.sonos_rooms.push(SonosRoom {
      uuid: "RINCON_123".to_string(),
      name: "Living Room".to_string(),
      location: "http://192.168.1.20:1400/xml/device_description.xml".to_string(),
    });
    app.selected_device_index = Some(0);

    handler(Key::Enter, &mut app);

    match rx.try_recv().unwrap() {
      IoEvent::TransferPlaybackToSonosRoom(uuid, persist) => {
        assert_eq!(uuid, "RINCON_123");
        assert!(persist);
      }
      _ => panic!("expected Sonos transfer event"),
    }
  }
}
