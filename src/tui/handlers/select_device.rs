use super::common_key_events;
use crate::core::app::{ActiveBlock, App};
use crate::core::playback_target::PlaybackTargetRef;
use crate::infra::network::IoEvent;
use crate::tui::event::Key;

pub fn handler(key: Key, app: &mut App) {
  match key {
    Key::Esc => {
      app.set_current_route_state(Some(ActiveBlock::Library), None);
    }
    k if common_key_events::down_event(k) => {
      move_selection_down(app);
    }
    k if common_key_events::up_event(k) => {
      move_selection_up(app);
    }
    k if common_key_events::high_event(k) => {
      if app.playback_target_count() > 0 && app.selected_device_index.is_some() {
        let next_index = common_key_events::on_high_press_handler();
        app.selected_device_index = Some(next_index);
      }
    }
    k if common_key_events::middle_event(k) => {
      let target_count = app.playback_target_count();
      if target_count > 0 && app.selected_device_index.is_some() {
        let next_index = middle_index(target_count);
        app.selected_device_index = Some(next_index);
      }
    }
    k if common_key_events::low_event(k) => {
      let target_count = app.playback_target_count();
      if target_count > 0 && app.selected_device_index.is_some() {
        app.selected_device_index = Some(target_count - 1);
      }
    }
    Key::Enter => {
      if let Some(index) = app.selected_device_index {
        let event = app.playback_target_at(index).map(|target| match target {
          PlaybackTargetRef::Spotify { id, .. } => {
            IoEvent::TransferPlaybackToDevice(id.to_string(), true)
          }
          PlaybackTargetRef::Sonos { room, .. } => {
            IoEvent::TransferPlaybackToSonosRoom(room.uuid.clone(), true)
          }
        });

        if let Some(event) = event {
          app.dispatch(event);
        }
      }
    }
    _ => {}
  }
}

fn move_selection_down(app: &mut App) {
  let target_count = app.playback_target_count();
  if target_count == 0 {
    return;
  }

  let next_index = app
    .selected_device_index
    .map(|index| (index + 1) % target_count)
    .unwrap_or(0);
  app.selected_device_index = Some(next_index);
}

fn move_selection_up(app: &mut App) {
  let target_count = app.playback_target_count();
  if target_count == 0 {
    return;
  }

  let next_index = app
    .selected_device_index
    .map(|index| {
      if index == 0 {
        target_count - 1
      } else {
        index - 1
      }
    })
    .unwrap_or(0);
  app.selected_device_index = Some(next_index);
}

fn middle_index(target_count: usize) -> usize {
  target_count.saturating_sub(1) / 2
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
