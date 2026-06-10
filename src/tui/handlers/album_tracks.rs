use super::common_key_events;
use crate::core::app::{AlbumTableContext, App, RecommendationsContext};
use crate::infra::network::IoEvent;
use crate::tui::event::Key;
use rspotify::{
  model::{PlayContextId, PlayableId},
  prelude::*,
};

pub fn handler(key: Key, app: &mut App) {
  match key {
    k if common_key_events::left_event(k) => common_key_events::handle_left_event(app),
    k if common_key_events::down_event(k) => match app.album_table_context {
      AlbumTableContext::Full => {
        if let Some(selected_album) = &app.selected_album_full {
          let next_index = common_key_events::on_down_press_handler(
            &selected_album.album.tracks.items,
            Some(app.saved_album_tracks_index),
          );
          app.saved_album_tracks_index = next_index;
        };
      }
      AlbumTableContext::Simplified => {
        if let Some(selected_album_simplified) = &mut app.selected_album_simplified {
          let next_index = common_key_events::on_down_press_handler(
            &selected_album_simplified.tracks.items,
            Some(selected_album_simplified.selected_index),
          );
          selected_album_simplified.selected_index = next_index;
        }
      }
    },
    k if common_key_events::up_event(k) => match app.album_table_context {
      AlbumTableContext::Full => {
        if let Some(selected_album) = &app.selected_album_full {
          let next_index = common_key_events::on_up_press_handler(
            &selected_album.album.tracks.items,
            Some(app.saved_album_tracks_index),
          );
          app.saved_album_tracks_index = next_index;
        };
      }
      AlbumTableContext::Simplified => {
        if let Some(selected_album_simplified) = &mut app.selected_album_simplified {
          let next_index = common_key_events::on_up_press_handler(
            &selected_album_simplified.tracks.items,
            Some(selected_album_simplified.selected_index),
          );
          selected_album_simplified.selected_index = next_index;
        }
      }
    },
    k if common_key_events::high_event(k) => handle_high_event(app),
    k if common_key_events::middle_event(k) => handle_middle_event(app),
    k if common_key_events::low_event(k) => handle_low_event(app),
    Key::Char('s') => handle_save_event(app),
    Key::Char('w') => handle_save_album_event(app),
    Key::Enter => match app.album_table_context {
      AlbumTableContext::Full => {
        if let Some(selected_album) = &app.selected_album_full {
          let context_id = Some(PlayContextId::Album(
            selected_album.album.id.clone().into_static(),
          ));
          app.dispatch(IoEvent::StartPlayback(
            context_id,
            None,
            Some(app.saved_album_tracks_index),
          ));
        };
      }
      AlbumTableContext::Simplified => {
        if let Some(selected_album_simplified) = &app.selected_album_simplified {
          let context_id = selected_album_simplified
            .album
            .id
            .clone()
            .map(|id| PlayContextId::Album(id.into_static()));
          app.dispatch(IoEvent::StartPlayback(
            context_id,
            None,
            Some(selected_album_simplified.selected_index),
          ));
        };
      }
    },
    //recommended playlist based on selected track
    Key::Char('r') => {
      handle_recommended_tracks(app);
    }
    _ if key == app.user_config.keys.add_item_to_queue => match app.album_table_context {
      AlbumTableContext::Full => {
        let playable_id = app
          .selected_album_full
          .as_ref()
          .and_then(|selected_album| {
            selected_album
              .album
              .tracks
              .items
              .get(app.saved_album_tracks_index)
          })
          .and_then(|track| track.id.clone())
          .map(|track_id| PlayableId::Track(track_id.into_static()));
        if let Some(playable_id) = playable_id {
          app.dispatch(IoEvent::AddItemToQueue(playable_id));
        }
      }
      AlbumTableContext::Simplified => {
        let playable_id = app
          .selected_album_simplified
          .as_ref()
          .and_then(|selected_album_simplified| {
            selected_album_simplified
              .tracks
              .items
              .get(selected_album_simplified.selected_index)
          })
          .and_then(|track| track.id.clone())
          .map(|track_id| PlayableId::Track(track_id.into_static()));
        if let Some(playable_id) = playable_id {
          app.dispatch(IoEvent::AddItemToQueue(playable_id));
        }
      }
    },
    _ => {}
  };
}

fn handle_high_event(app: &mut App) {
  match app.album_table_context {
    AlbumTableContext::Full => {
      let next_index = common_key_events::on_high_press_handler();
      app.saved_album_tracks_index = next_index;
    }
    AlbumTableContext::Simplified => {
      if let Some(selected_album_simplified) = &mut app.selected_album_simplified {
        let next_index = common_key_events::on_high_press_handler();
        selected_album_simplified.selected_index = next_index;
      }
    }
  }
}

fn handle_middle_event(app: &mut App) {
  match app.album_table_context {
    AlbumTableContext::Full => {
      if let Some(selected_album) = &app.selected_album_full {
        let next_index =
          common_key_events::on_middle_press_handler(&selected_album.album.tracks.items);
        app.saved_album_tracks_index = next_index;
      };
    }
    AlbumTableContext::Simplified => {
      if let Some(selected_album_simplified) = &mut app.selected_album_simplified {
        let next_index =
          common_key_events::on_middle_press_handler(&selected_album_simplified.tracks.items);
        selected_album_simplified.selected_index = next_index;
      }
    }
  }
}

fn handle_low_event(app: &mut App) {
  match app.album_table_context {
    AlbumTableContext::Full => {
      if let Some(selected_album) = &app.selected_album_full {
        let next_index =
          common_key_events::on_low_press_handler(&selected_album.album.tracks.items);
        app.saved_album_tracks_index = next_index;
      };
    }
    AlbumTableContext::Simplified => {
      if let Some(selected_album_simplified) = &mut app.selected_album_simplified {
        let next_index =
          common_key_events::on_low_press_handler(&selected_album_simplified.tracks.items);
        selected_album_simplified.selected_index = next_index;
      }
    }
  }
}

fn handle_recommended_tracks(app: &mut App) {
  match app.album_table_context {
    AlbumTableContext::Full => {
      let selected_track = app
        .library
        .saved_albums
        .get_results(None)
        .and_then(|albums| albums.items.get(app.album_list_index))
        .and_then(|selected_album| {
          selected_album
            .album
            .tracks
            .items
            .get(app.saved_album_tracks_index)
        })
        .and_then(|track| {
          track
            .id
            .as_ref()
            .map(|id| (track.name.clone(), id.id().to_string()))
        });

      if let Some((track_name, track_id)) = selected_track {
        app.recommendations_context = Some(RecommendationsContext::Song);
        app.recommendations_seed = track_name;
        app.get_recommendations_for_track_id(track_id);
      }
    }
    AlbumTableContext::Simplified => {
      let selected_track = app
        .selected_album_simplified
        .as_ref()
        .and_then(|selected_album_simplified| {
          selected_album_simplified
            .tracks
            .items
            .get(selected_album_simplified.selected_index)
        })
        .and_then(|track| {
          track
            .id
            .as_ref()
            .map(|id| (track.name.clone(), id.id().to_string()))
        });

      if let Some((track_name, track_id)) = selected_track {
        app.recommendations_context = Some(RecommendationsContext::Song);
        app.recommendations_seed = track_name;
        app.get_recommendations_for_track_id(track_id);
      }
    }
  }
}

fn handle_save_event(app: &mut App) {
  match app.album_table_context {
    AlbumTableContext::Full => {
      let playable_id = app
        .selected_album_full
        .as_ref()
        .and_then(|selected_album| {
          selected_album
            .album
            .tracks
            .items
            .get(app.saved_album_tracks_index)
        })
        .and_then(|selected_track| selected_track.id.clone())
        .map(|track_id| PlayableId::Track(track_id.into_static()));
      if let Some(playable_id) = playable_id {
        app.dispatch(IoEvent::ToggleSaveTrack(playable_id));
      }
    }
    AlbumTableContext::Simplified => {
      let playable_id = app
        .selected_album_simplified
        .as_ref()
        .and_then(|selected_album_simplified| {
          selected_album_simplified
            .tracks
            .items
            .get(selected_album_simplified.selected_index)
        })
        .and_then(|selected_track| selected_track.id.clone())
        .map(|track_id| PlayableId::Track(track_id.into_static()));
      if let Some(playable_id) = playable_id {
        app.dispatch(IoEvent::ToggleSaveTrack(playable_id));
      }
    }
  }
}

fn handle_save_album_event(app: &mut App) {
  match app.album_table_context {
    AlbumTableContext::Full => {
      let album_id = app
        .selected_album_full
        .as_ref()
        .map(|selected_album| selected_album.album.id.clone().into_static());
      if let Some(album_id) = album_id {
        app.dispatch(IoEvent::CurrentUserSavedAlbumAdd(album_id));
      }
    }
    AlbumTableContext::Simplified => {
      let album_id = app
        .selected_album_simplified
        .as_ref()
        .and_then(|selected_album_simplified| selected_album_simplified.album.id.clone())
        .map(|album_id| album_id.into_static());
      if let Some(album_id) = album_id {
        app.dispatch(IoEvent::CurrentUserSavedAlbumAdd(album_id));
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::app::ActiveBlock;

  #[test]
  fn on_left_press() {
    let mut app = App::default();
    app.set_current_route_state(
      Some(ActiveBlock::AlbumTracks),
      Some(ActiveBlock::AlbumTracks),
    );

    handler(Key::Left, &mut app);
    let current_route = app.get_current_route();
    assert_eq!(current_route.active_block, ActiveBlock::Empty);
    assert_eq!(current_route.hovered_block, ActiveBlock::Library);
  }

  #[test]
  fn on_esc() {
    let mut app = App::default();

    handler(Key::Esc, &mut app);

    let current_route = app.get_current_route();
    assert_eq!(current_route.active_block, ActiveBlock::Empty);
  }
}
