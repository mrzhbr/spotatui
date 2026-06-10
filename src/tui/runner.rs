use crate::core::app::{self, ActiveBlock, App, RouteId};
use crate::core::auth;
use crate::core::user_config::UserConfig;
use crate::infra::network::IoEvent;
use crate::tui::event::{self, Key};
use crate::tui::handlers;
use crate::tui::ui;
use anyhow::Result;
use crossterm::{
  cursor::MoveTo,
  event::{
    DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
  },
  execute,
  terminal::{supports_keyboard_enhancement, SetTitle},
  ExecutableCommand,
};
use log::info;
use ratatui::backend::Backend;
use std::{
  cmp::{max, min},
  io::stdout,
  sync::{atomic::AtomicU64, Arc},
  time::SystemTime,
};
use tokio::sync::Mutex;

const DEFAULT_WINDOW_TITLE: &str = "spt - spotatui";

#[derive(Default)]
struct WindowTitleState {
  last_title: Option<String>,
}

#[derive(Default)]
struct CursorVisibilityState {
  is_visible: Option<bool>,
}

fn next_cursor_visibility_update(
  state: &mut CursorVisibilityState,
  desired_visible: bool,
) -> Option<bool> {
  if state.is_visible == Some(desired_visible) {
    None
  } else {
    state.is_visible = Some(desired_visible);
    Some(desired_visible)
  }
}

fn playback_window_title(app: &App) -> String {
  if app.is_streaming_active {
    if let Some(native_info) = app.native_track_info.as_ref() {
      return playback_window_title_from_parts(&native_info.name, &native_info.artists_display);
    }
  }

  let Some(item) = app
    .current_playback_context
    .as_ref()
    .and_then(|context| context.item.as_ref())
  else {
    return DEFAULT_WINDOW_TITLE.to_string();
  };

  match item {
    rspotify::model::PlayableItem::Track(track) => {
      let artist = crate::tui::ui::util::create_artist_string(&track.artists);
      playback_window_title_from_parts(&track.name, &artist)
    }
    rspotify::model::PlayableItem::Episode(episode) => {
      playback_window_title_from_parts(&episode.name, &episode.show.name)
    }
    rspotify::model::PlayableItem::Unknown(_) => DEFAULT_WINDOW_TITLE.to_string(),
  }
}

fn playback_window_title_from_parts(title: &str, artist: &str) -> String {
  let mut display = String::with_capacity(title.len() + 3 + artist.len());
  append_sanitized_window_title_component(&mut display, title);

  if artist
    .chars()
    .any(|c| !c.is_control() && !c.is_whitespace())
  {
    display.push_str(" — ");
    append_sanitized_window_title_component(&mut display, artist);
  }

  display
}

fn append_sanitized_window_title_component(display: &mut String, value: &str) {
  display.extend(value.chars().filter(|c| !c.is_control()));
}

fn next_window_title(state: &mut WindowTitleState, app: &App) -> Option<String> {
  if !app.user_config.behavior.set_window_title {
    return state
      .last_title
      .take()
      .map(|_| DEFAULT_WINDOW_TITLE.to_string());
  }

  let title = playback_window_title(app);
  if state.last_title.as_ref() == Some(&title) {
    None
  } else {
    state.last_title = Some(title.clone());
    Some(title)
  }
}

fn reset_window_title(state: &mut WindowTitleState) -> Result<()> {
  if state
    .last_title
    .as_deref()
    .is_some_and(|title| title != DEFAULT_WINDOW_TITLE)
  {
    execute!(stdout(), SetTitle(DEFAULT_WINDOW_TITLE))?;
    state.last_title = None;
  }
  Ok(())
}

fn back_key_clears_playlist_filter(app: &mut App, active_block: ActiveBlock) -> bool {
  if active_block == ActiveBlock::TrackTable && app.is_playlist_track_filter_active() {
    app.clear_playlist_track_filter();
    true
  } else {
    false
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::app::{NativeTrackInfo, TrackTableContext};
  use rspotify::model::idtypes::PlaylistId;
  use std::{sync::mpsc::channel, time::SystemTime};

  fn app() -> App {
    let (tx, _rx) = channel();
    App::new(
      tx,
      crate::core::user_config::UserConfig::new(),
      SystemTime::now(),
    )
  }

  #[test]
  fn cursor_visibility_update_only_emits_on_changes() {
    let mut state = CursorVisibilityState::default();

    assert_eq!(
      next_cursor_visibility_update(&mut state, false),
      Some(false)
    );
    assert_eq!(next_cursor_visibility_update(&mut state, false), None);
    assert_eq!(next_cursor_visibility_update(&mut state, true), Some(true));
    assert_eq!(next_cursor_visibility_update(&mut state, true), None);
    assert_eq!(
      next_cursor_visibility_update(&mut state, false),
      Some(false)
    );
  }

  #[test]
  fn playback_window_title_uses_current_native_track() {
    let mut app = app();
    app.is_streaming_active = true;
    app.native_track_info = Some(NativeTrackInfo {
      name: "The Track".to_string(),
      artists_display: "The Artist".to_string(),
      album: "The Album".to_string(),
      duration_ms: 180_000,
    });

    assert_eq!(playback_window_title(&app), "The Track — The Artist");
  }

  #[test]
  fn playback_window_title_strips_control_characters() {
    let mut app = app();
    app.is_streaming_active = true;
    app.native_track_info = Some(NativeTrackInfo {
      name: "The\x1b]2;Bad\x07 Track".to_string(),
      artists_display: "The\nArtist".to_string(),
      album: "The Album".to_string(),
      duration_ms: 180_000,
    });

    assert_eq!(playback_window_title(&app), "The]2;Bad Track — TheArtist");
  }

  #[test]
  fn playback_window_title_falls_back_without_playback() {
    let app = app();

    assert_eq!(playback_window_title(&app), DEFAULT_WINDOW_TITLE);
  }

  #[test]
  fn disabling_window_title_restores_default_once() {
    let mut app = app();
    let mut state = WindowTitleState {
      last_title: Some("The Track — The Artist".to_string()),
    };
    app.user_config.behavior.set_window_title = false;

    assert_eq!(
      next_window_title(&mut state, &app).as_deref(),
      Some(DEFAULT_WINDOW_TITLE)
    );
    assert_eq!(next_window_title(&mut state, &app), None);
  }

  #[test]
  fn back_key_clears_playlist_filter_before_navigation_pop() {
    let mut app = app();
    app.track_table.context = Some(TrackTableContext::MyPlaylists);
    app.playlist_track_table_id = Some(
      PlaylistId::from_id("37i9dQZF1DX4WYpdgoIcn6")
        .unwrap()
        .into_static(),
    );
    app.active_playlist_track_filter = Some("query".to_string());
    app.push_navigation_stack(RouteId::TrackTable, ActiveBlock::TrackTable);

    assert!(back_key_clears_playlist_filter(
      &mut app,
      ActiveBlock::TrackTable
    ));

    assert!(app.active_playlist_track_filter.is_none());
    assert_eq!(app.get_current_route().id, RouteId::TrackTable);
  }
}

#[cfg(feature = "streaming")]
async fn pause_native_playback_before_exit(app: &Arc<Mutex<App>>) {
  let player = {
    let mut app = app.lock().await;
    if !app.is_streaming_active {
      return;
    }

    let Some(player) = app.streaming_player.clone() else {
      return;
    };

    let is_playing = app.native_is_playing.unwrap_or_else(|| {
      app
        .current_playback_context
        .as_ref()
        .map(|context| context.is_playing)
        .unwrap_or(false)
    });

    if !is_playing {
      return;
    }

    app.native_is_playing = Some(false);
    if let Some(context) = app.current_playback_context.as_mut() {
      context.is_playing = false;
    }

    player
  };

  player.pause();
  tokio::time::sleep(std::time::Duration::from_millis(150)).await;
}

pub async fn start_ui(
  user_config: UserConfig,
  app: &Arc<Mutex<App>>,
  shared_position: Option<Arc<AtomicU64>>,
) -> Result<()> {
  info!("ui thread initialized");
  #[cfg(not(feature = "streaming"))]
  let _ = &shared_position;

  let mut terminal = ratatui::init();
  execute!(stdout(), EnableMouseCapture)?;
  let keyboard_enhancement_supported = supports_keyboard_enhancement().unwrap_or(false);
  let keyboard_enhancement_enabled = keyboard_enhancement_supported
    && execute!(
      stdout(),
      PushKeyboardEnhancementFlags(
        KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
          | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
          | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
      )
    )
    .is_ok();
  if keyboard_enhancement_enabled {
    info!("enabled keyboard enhancement flags");
  }
  {
    let mut app = app.lock().await;
    app.terminal_input_caps.keyboard_enhancement_supported = keyboard_enhancement_supported;
    app.terminal_input_caps.keyboard_enhancement_enabled = keyboard_enhancement_enabled;
    app.terminal_input_caps.ctrl_punct_reliable = app::CapabilityState::Unknown;
  }

  let events = event::Events::new(user_config.behavior.tick_rate_milliseconds);

  let mut window_title_state = WindowTitleState::default();
  let mut cursor_visibility_state = CursorVisibilityState::default();
  let mut is_first_render = true;

  loop {
    let terminal_size = terminal.backend().size().ok();
    let title_update = {
      let mut app = app.lock().await;

      if let Some(size) = terminal_size {
        if is_first_render || app.size != size {
          app.help_menu_max_lines = 0;
          app.help_menu_offset = 0;
          app.help_menu_page = 0;
          app.size = size;

          let potential_limit = max((app.size.height as i32) - 13, 0) as u32;
          let max_limit = min(potential_limit, 50);
          let large_search_limit = min((f32::from(size.height) / 1.4) as u32, max_limit);
          let small_search_limit = min((f32::from(size.height) / 2.85) as u32, max_limit / 2);

          app.dispatch(IoEvent::UpdateSearchLimits(
            large_search_limit,
            small_search_limit,
          ));

          app.help_menu_max_lines = if app.size.height > 8 {
            (app.size.height as u32) - 8
          } else {
            0
          };
        }
      };

      let current_route = app.get_current_route();
      events.set_tick_rate(app.user_config.behavior.tick_rate_milliseconds);

      terminal.draw(|f| {
        use ratatui::{prelude::Style, widgets::Block};
        f.render_widget(
          Block::default().style(Style::default().bg(app.user_config.theme.background)),
          f.area(),
        );

        match current_route.active_block {
          ActiveBlock::HelpMenu => ui::draw_help_menu(f, &app),
          ActiveBlock::Queue => ui::draw_queue(f, &app),
          ActiveBlock::Error => ui::draw_error_screen(f, &app),
          ActiveBlock::SelectDevice => ui::draw_device_list(f, &app),
          ActiveBlock::LyricsView => ui::draw_lyrics_view(f, &app),
          ActiveBlock::MiniPlayer => ui::draw_miniplayer(f, &app),
          #[cfg(feature = "cover-art")]
          ActiveBlock::CoverArtView => ui::draw_cover_art_view(f, &app),
          ActiveBlock::ExitPrompt => ui::draw_exit_prompt(f, &app),
          ActiveBlock::Settings => ui::settings::draw_settings(f, &app),
          ActiveBlock::CreatePlaylistForm => {
            ui::draw_main_layout(f, &app);
            ui::draw_create_playlist_form(f, &app);
          }
          _ => ui::draw_main_layout(f, &app),
        }
      })?;

      let cursor_should_be_visible = current_route.active_block == ActiveBlock::Input;
      if let Some(visible) =
        next_cursor_visibility_update(&mut cursor_visibility_state, cursor_should_be_visible)
      {
        if visible {
          terminal.show_cursor()?;
        } else {
          terminal.hide_cursor()?;
        }
      }

      if cursor_should_be_visible {
        let cursor_offset = if app.size.height > ui::util::SMALL_TERMINAL_HEIGHT {
          2
        } else {
          1
        };

        terminal.backend_mut().execute(MoveTo(
          cursor_offset + app.input_cursor_position - app.input_scroll_offset.get(),
          cursor_offset,
        ))?;
      }

      if auth::should_refresh_token_at(app.spotify_token_expiry, SystemTime::now())
        && !app.auth_refresh_in_progress
      {
        app.auth_refresh_in_progress = true;
        app.dispatch(IoEvent::RefreshAuthentication);
      }
      next_window_title(&mut window_title_state, &app)
    };
    if let Some(title) = title_update {
      execute!(stdout(), SetTitle(title.as_str()))?;
    }

    match events.next()? {
      event::Event::Input(key) => {
        let mut app = app.lock().await;
        if key == Key::Ctrl('c') {
          app.close_io_channel();
          break;
        }

        let current_active_block = app.get_current_route().active_block;

        if current_active_block == ActiveBlock::ExitPrompt {
          match key {
            Key::Enter | Key::Char('y') | Key::Char('Y') => {
              app.close_io_channel();
              break;
            }
            Key::Esc | Key::Char('n') | Key::Char('N') => {
              app.pop_navigation_stack();
            }
            _ if key == app.user_config.keys.back => {
              app.pop_navigation_stack();
            }
            _ => {}
          }
        } else if current_active_block == ActiveBlock::Input {
          handlers::input_handler(key, &mut app);
        } else if key == app.user_config.keys.back {
          if !back_key_clears_playlist_filter(&mut app, current_active_block) {
            if current_active_block == ActiveBlock::Settings {
              handlers::handle_app(key, &mut app);
            } else if app.get_current_route().active_block != ActiveBlock::Input {
              let pop_result = match app.pop_navigation_stack() {
                Some(ref x) if x.id == RouteId::Search => app.pop_navigation_stack(),
                Some(x) => Some(x),
                None => None,
              };
              if pop_result.is_none() {
                app.push_navigation_stack(RouteId::ExitPrompt, ActiveBlock::ExitPrompt);
              }
            }
          }
        } else {
          handlers::handle_app(key, &mut app);
        }
      }
      event::Event::Mouse(mouse) => {
        let mut app = app.lock().await;
        if !app.user_config.behavior.disable_mouse_inputs {
          handlers::mouse_handler(mouse, &mut app);
        }
      }
      event::Event::Tick(elapsed) => {
        #[cfg(all(feature = "macos-media", target_os = "macos"))]
        {
          use objc2_foundation::{NSDate, NSRunLoop};
          NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.001));
        }

        let mut app = app.lock().await;
        app.update_on_tick(elapsed);

        #[cfg(feature = "streaming")]
        app.flush_pending_native_seek();
        app.flush_pending_api_seek();
        app.flush_pending_volume();

        #[cfg(feature = "streaming")]
        if let Some(ref pos) = shared_position {
          if app.is_streaming_active {
            let recently_seeked = app
              .last_native_seek
              .is_some_and(|t| t.elapsed().as_millis() < app::SEEK_POSITION_IGNORE_MS);

            if !recently_seeked {
              let position_ms = pos.load(std::sync::atomic::Ordering::Relaxed);
              if position_ms > 0 {
                app.song_progress_ms = position_ms as u128;
              }
            }
          }
        }
        #[cfg(not(feature = "streaming"))]
        if let Some(ref pos) = shared_position {
          if app.is_streaming_active {
            let position_ms = pos.load(std::sync::atomic::Ordering::Relaxed);
            if position_ms > 0 {
              app.song_progress_ms = position_ms as u128;
            }
          }
        }
      }
    }

    if is_first_render {
      let mut app = app.lock().await;
      app.dispatch(IoEvent::GetPlaylists);
      app.dispatch(IoEvent::GetUser);
      app.dispatch(IoEvent::GetCurrentPlayback);
      app.help_docs_size = ui::help::HELP_DOCS_LEN as u32;
      is_first_render = false;
    }
  }

  #[cfg(feature = "streaming")]
  pause_native_playback_before_exit(app).await;

  reset_window_title(&mut window_title_state)?;
  execute!(stdout(), DisableMouseCapture)?;
  if keyboard_enhancement_enabled {
    let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
  }
  ratatui::restore();

  Ok(())
}
