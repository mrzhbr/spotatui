#[cfg(all(target_os = "linux", feature = "streaming"))]
mod alsa_silence {
  use std::os::raw::{c_char, c_int};

  type SndLibErrorHandlerT =
    Option<unsafe extern "C" fn(*const c_char, c_int, *const c_char, c_int, *const c_char)>;

  extern "C" {
    fn snd_lib_error_set_handler(handler: SndLibErrorHandlerT) -> c_int;
  }

  unsafe extern "C" fn silent_error_handler(
    _file: *const c_char,
    _line: c_int,
    _function: *const c_char,
    _err: c_int,
    _fmt: *const c_char,
  ) {
  }

  pub fn suppress_alsa_errors() {
    unsafe {
      snd_lib_error_set_handler(Some(silent_error_handler));
    }
  }
}

use crate::cli;
use crate::core::app::App;
use crate::core::auth;
use crate::core::config::ClientConfig;
use crate::core::user_config::{
  validate_tick_rate_milliseconds, StartupBehavior, UserConfig, UserConfigPaths,
};
#[cfg(all(feature = "macos-media", target_os = "macos"))]
use crate::infra::macos_media;
#[cfg(feature = "streaming")]
use crate::infra::network::requests::spotify_get_typed_compat_for_with_refresh;
use crate::infra::network::{IoEvent, Network};
#[cfg(feature = "streaming")]
use crate::infra::player;
use crate::tui::banner::BANNER;

use anyhow::{anyhow, Result};
use backtrace::Backtrace;
use clap::{Arg, Command as ClapApp};
use clap_complete::{generate, Shell};
use log::info;
#[cfg(feature = "streaming")]
use log::warn;
#[cfg(feature = "streaming")]
use rspotify::{model::user::PrivateUser, AuthCodePkceSpotify};
#[cfg(feature = "streaming")]
use std::path::Path;
#[cfg(feature = "streaming")]
use std::time::Duration;
use std::{
  fs,
  io::{self, Write},
  panic,
  path::PathBuf,
  sync::{atomic::AtomicU64, Arc},
};
use tokio::sync::Mutex;

#[cfg(all(feature = "macos-media", target_os = "macos"))]
#[derive(Default, PartialEq)]
struct MacosMetadata {
  title: String,
  artists: Vec<String>,
  album: String,
  duration_ms: u32,
  art_url: Option<String>,
}
#[cfg(all(feature = "macos-media", target_os = "macos"))]
fn update_macos_metadata(
  manager: &macos_media::MacMediaManager,
  last_metadata: &mut Option<MacosMetadata>,
  app: &App,
) {
  if let Some(metadata) = crate::infra::media_metadata::current_playback_metadata(app) {
    let new_metadata = MacosMetadata {
      title: metadata.title,
      artists: metadata.artists,
      album: metadata.album,
      duration_ms: metadata.duration_ms,
      art_url: metadata.image_url,
    };

    // Only update if metadata changed to avoid repeated artwork fetches.
    if last_metadata.as_ref() != Some(&new_metadata) {
      manager.set_metadata(
        &new_metadata.title,
        &new_metadata.artists,
        &new_metadata.album,
        new_metadata.duration_ms,
        new_metadata.art_url.clone(),
      );
      *last_metadata = Some(new_metadata);
    }
  } else if last_metadata.is_some() {
    *last_metadata = None;
  }
}

#[cfg(feature = "streaming")]
fn subscription_level_label(level: rspotify::model::SubscriptionLevel) -> &'static str {
  match level {
    rspotify::model::SubscriptionLevel::Premium => "premium",
    rspotify::model::SubscriptionLevel::Free => "free",
  }
}

#[cfg(feature = "streaming")]
async fn account_supports_native_streaming(
  spotify: &AuthCodePkceSpotify,
  token_cache_path: &Path,
  app: &Arc<Mutex<App>>,
) -> (bool, Option<&'static str>) {
  match spotify_get_typed_compat_for_with_refresh::<PrivateUser>(
    spotify,
    "me",
    &[],
    token_cache_path,
    app,
  )
  .await
  {
    #[allow(deprecated)]
    Ok(user) => match user.product {
      Some(rspotify::model::SubscriptionLevel::Premium) => (true, None),
      Some(level) => {
        let plan = subscription_level_label(level);
        info!(
          "spotify {} account detected: playback is unavailable (native streaming and Web API playback controls require premium)",
          plan
        );
        println!(
          "Spotify {} account detected. Playback is unavailable in spotatui: native streaming (librespot) and Web API playback controls both require Premium. Browsing/search/library views still work.",
          plan
        );
        (
          false,
          Some("Spotify Free account: playback controls unavailable (Premium required)"),
        )
      }
      None => {
        info!("spotify account level unknown: native streaming disabled to avoid librespot exit");
        println!(
          "Could not determine Spotify subscription level. Native streaming is disabled to avoid startup exit. If this account is not Premium, playback controls will not work; browsing/search/library views still work."
        );
        (
          false,
          Some("Could not verify Spotify plan: native streaming disabled"),
        )
      }
    },
    Err(e) => {
      info!(
        "spotify account level check failed ({}); native streaming disabled to avoid librespot exit",
        e
      );
      println!(
        "Could not verify Spotify subscription level. Native streaming is disabled to avoid startup exit. If this account is not Premium, playback controls will not work; browsing/search/library views still work."
      );
      (
        false,
        Some("Could not verify Spotify plan: native streaming disabled"),
      )
    }
  }
}

#[cfg(any(feature = "streaming", test))]
#[derive(Debug, PartialEq, Eq)]
enum StartupDeviceEvent {
  Transfer {
    device_id: String,
    persist_device_id: bool,
  },
  AutoSelectStreaming {
    device_name: String,
    persist_device_id: bool,
  },
}

#[cfg(any(feature = "streaming", test))]
#[derive(Debug, PartialEq, Eq)]
struct StartupDeviceDecision {
  event: Option<StartupDeviceEvent>,
  status_message: Option<String>,
}

#[cfg(feature = "streaming")]
impl StartupDeviceEvent {
  fn into_io_event(self) -> IoEvent {
    match self {
      StartupDeviceEvent::Transfer {
        device_id,
        persist_device_id,
      } => IoEvent::TransferPlaybackToDevice(device_id, persist_device_id),
      StartupDeviceEvent::AutoSelectStreaming {
        device_name,
        persist_device_id,
      } => IoEvent::AutoSelectStreamingDevice(device_name, persist_device_id),
    }
  }
}

#[cfg(any(feature = "streaming", test))]
fn startup_device_decision(
  startup_behavior: StartupBehavior,
  saved_device_id: Option<String>,
  devices_snapshot: Option<&[rspotify::model::device::Device]>,
  native_device_name: &str,
) -> StartupDeviceDecision {
  if startup_behavior != StartupBehavior::Play {
    return StartupDeviceDecision {
      event: None,
      status_message: None,
    };
  }

  let event = match saved_device_id {
    Some(saved_device_id) => {
      if crate::core::playback_target::parse_sonos_persisted_id(&saved_device_id).is_some() {
        Some(StartupDeviceEvent::Transfer {
          device_id: saved_device_id,
          persist_device_id: true,
        })
      } else if let Some(devices) = devices_snapshot {
        let mut saved_device_available = false;
        let mut native_device_id = None;

        for device in devices {
          if device.id.as_ref() == Some(&saved_device_id) {
            saved_device_available = true;
            break;
          }

          if native_device_id.is_none() && device.name.eq_ignore_ascii_case(native_device_name) {
            native_device_id = device.id.clone();
          }
        }

        if saved_device_available {
          Some(StartupDeviceEvent::Transfer {
            device_id: saved_device_id,
            persist_device_id: true,
          })
        } else {
          native_device_id.map_or_else(
            || {
              Some(StartupDeviceEvent::AutoSelectStreaming {
                device_name: native_device_name.to_string(),
                persist_device_id: false,
              })
            },
            |device_id| {
              Some(StartupDeviceEvent::Transfer {
                device_id,
                persist_device_id: false,
              })
            },
          )
        }
      } else {
        Some(StartupDeviceEvent::Transfer {
          device_id: saved_device_id,
          persist_device_id: true,
        })
      }
    }
    None => Some(StartupDeviceEvent::AutoSelectStreaming {
      device_name: native_device_name.to_string(),
      persist_device_id: true,
    }),
  };

  let status_message = matches!(
    event,
    Some(
      StartupDeviceEvent::Transfer {
        persist_device_id: false,
        ..
      } | StartupDeviceEvent::AutoSelectStreaming {
        persist_device_id: false,
        ..
      }
    )
  )
  .then(|| format!("Saved device unavailable; using {}", native_device_name));

  StartupDeviceDecision {
    event,
    status_message,
  }
}

#[cfg(all(target_os = "linux", feature = "streaming"))]
fn init_audio_backend() {
  alsa_silence::suppress_alsa_errors();
}

#[cfg(not(all(target_os = "linux", feature = "streaming")))]
fn init_audio_backend() {}

fn setup_logging() -> anyhow::Result<()> {
  // Get the current Process ID
  let pid = std::process::id();

  // Construct the log file path using the PID
  let log_dir = "/tmp/spotatui_logs/";
  let log_path = format!("{}spotatuilog{}", log_dir, pid);

  // Ensure the directory exists. If not, create.
  if !std::path::Path::new(log_dir).exists() {
    std::fs::create_dir_all(log_dir)
      .map_err(|e| anyhow::anyhow!("Failed to create log directory {}: {}", log_dir, e))?;
  }
  // define format of log messages.
  fern::Dispatch::new()
    .format(|out, message, record| {
      out.finish(format_args!(
        "{}[{}][{}] {}",
        chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
        record.target(),
        record.level(),
        message
      ))
    })
    .level(log::LevelFilter::Info)
    .chain(fern::log_file(&log_path)?) // Use the dynamic path
    .apply()
    .map_err(|e| anyhow::anyhow!("Failed to initialize logger: {}", e))?;

  // Print the location of log for user reference.
  println!("Logging to: {}", log_path);

  Ok(())
}

fn install_panic_hook() {
  let default_hook = panic::take_hook();
  panic::set_hook(Box::new(move |info| {
    let is_audio_backend_panic = info
      .location()
      .map(|location| {
        let file = location.file();
        file.contains("audio_backend/portaudio.rs") || file.contains("audio_backend/rodio.rs")
      })
      .unwrap_or(false);

    if is_audio_backend_panic {
      eprintln!(
        "Recoverable audio backend panic detected. Playback may pause while the output device changes."
      );
      return;
    }

    ratatui::restore();
    let panic_log_path = dirs::home_dir().map(|home| {
      home
        .join(".config")
        .join("spotatui")
        .join("spotatui_panic.log")
    });

    if let Some(path) = panic_log_path.as_ref() {
      if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
      }
      if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
      {
        let _ = writeln!(f, "\n==== spotatui panic ====");
        let _ = writeln!(f, "{}", info);
        let _ = writeln!(f, "{:?}", Backtrace::new());
      }
      eprintln!("A crash log was written to: {}", path.to_string_lossy());
    }
    default_hook(info);

    if cfg!(debug_assertions) && std::env::var_os("RUST_BACKTRACE").is_none() {
      eprintln!("{:?}", Backtrace::new());
    }

    if cfg!(target_os = "windows") && std::env::var_os("SPOTATUI_PAUSE_ON_PANIC").is_some() {
      eprintln!("Press Enter to close...");
      let mut s = String::new();
      let _ = std::io::stdin().read_line(&mut s);
    }
  }));
}

pub async fn run() -> Result<()> {
  setup_logging()?;
  info!("spotatui {} starting up", env!("CARGO_PKG_VERSION"));
  init_audio_backend();
  info!("audio backend initialized");

  install_panic_hook();
  info!("panic hook configured");

  let mut clap_app = ClapApp::new(env!("CARGO_PKG_NAME"))
    .version(env!("CARGO_PKG_VERSION"))
    .author(env!("CARGO_PKG_AUTHORS"))
    .about(env!("CARGO_PKG_DESCRIPTION"))
    .override_usage("Press `?` while running the app to see keybindings")
    .before_help(BANNER)
    .after_help(
      "Client authentication settings are stored in $HOME/.config/spotatui/client.yml (use --reconfigure-auth to update them)",
    )
    .arg(
      Arg::new("tick-rate")
        .short('t')
        .long("tick-rate")
        .help("Set the normal UI tick rate in milliseconds.")
        .long_help(
          "Specify the UI tick rate in milliseconds. Lower values refresh screens more often and cost more CPU.",
        ),
    )
    .arg(
      Arg::new("config")
        .short('c')
        .long("config")
        .help("Specify configuration file path."),
    )
    .arg(
      Arg::new("reconfigure-auth")
        .long("reconfigure-auth")
        .action(clap::ArgAction::SetTrue)
        .help("Rerun client authentication setup wizard"),
    )
    .arg(
      Arg::new("completions")
        .long("completions")
        .help("Generates completions for your preferred shell")
        .value_parser(["bash", "zsh", "fish", "power-shell", "elvish"])
        .value_name("SHELL"),
    )
    // Control spotify from the command line
    .subcommand(cli::playback_subcommand())
    .subcommand(cli::play_subcommand())
    .subcommand(cli::list_subcommand())
    .subcommand(cli::history_subcommand())
    .subcommand(cli::search_subcommand());

  let matches = clap_app.clone().get_matches();

  // Shell completions don't need any spotify work
  if let Some(s) = matches.get_one::<String>("completions") {
    let shell = match s.as_str() {
      "fish" => Shell::Fish,
      "bash" => Shell::Bash,
      "zsh" => Shell::Zsh,
      "power-shell" => Shell::PowerShell,
      "elvish" => Shell::Elvish,
      _ => return Err(anyhow!("no completions avaible for '{}'", s)),
    };
    generate(shell, &mut clap_app, "spotatui", &mut io::stdout());
    return Ok(());
  }

  if let Some(history_matches) = matches.subcommand_matches("history") {
    println!("{}", cli::handle_history_matches(history_matches)?);
    return Ok(());
  }

  let mut user_config = UserConfig::new();
  if let Some(config_file_path) = matches.get_one::<String>("config") {
    let config_file_path = PathBuf::from(config_file_path);
    let path = UserConfigPaths { config_file_path };
    user_config.path_to_config.replace(path);
  }
  user_config.load_config()?;
  info!("user config loaded successfully");

  let initial_shuffle_enabled = user_config.behavior.shuffle_enabled;
  let initial_startup_behavior = user_config.behavior.startup_behavior;

  if let Some(tick_rate) = matches
    .get_one::<String>("tick-rate")
    .and_then(|tick_rate| tick_rate.parse().ok())
  {
    user_config.behavior.tick_rate_milliseconds =
      validate_tick_rate_milliseconds(tick_rate, "Tick rate")?;
  }

  let mut client_config = ClientConfig::new();
  client_config.load_config()?;
  info!("client authentication config loaded");

  let reconfigure_auth = matches.get_flag("reconfigure-auth");

  if reconfigure_auth {
    println!("\nReconfiguring client authentication...");
    client_config.reconfigure_auth()?;
    println!("Client authentication setup updated.\n");
  } else if matches.subcommand_name().is_none() && client_config.needs_auth_setup_migration() {
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Authentication Setup Update");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!(
      "\nConfiguration handling has changed and your authentication setup may need an update."
    );
    println!("Would you like to run the new auth setup wizard now? (Y/n): ");

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim().to_lowercase();
    let run_migration = input.is_empty() || input == "y" || input == "yes";

    if run_migration {
      client_config.reconfigure_auth()?;
      println!("Client authentication setup updated.\n");
    } else {
      client_config.mark_auth_setup_migrated()?;
      println!("Skipped. You can run this anytime with `spotatui --reconfigure-auth`.\n");
    }
  }

  let config_paths = client_config.get_or_build_paths()?;
  let authenticated = auth::authenticate_with_fallback(&mut client_config, &config_paths).await?;
  let spotify = authenticated.spotify;
  let final_token_cache_path = authenticated.token_cache_path;
  #[cfg(feature = "streaming")]
  let selected_redirect_uri = authenticated.redirect_uri;

  // Persist whatever token is now in memory. All later Spotify requests go through
  // spotatui's refresh-and-cache path so the on-disk token stays current.
  if let Err(e) = auth::save_token_to_file(&spotify, &final_token_cache_path).await {
    log::warn!("Failed to cache token on startup: {}", e);
  }
  // Verify that we have a valid token before proceeding
  let token_expiry = auth::token_expiry(&spotify).await?;

  let (sync_io_tx, sync_io_rx) = std::sync::mpsc::channel::<IoEvent>();
  info!("app state initialized");

  // Initialise app state
  let app = Arc::new(Mutex::new(App::new(
    sync_io_tx,
    user_config.clone(),
    token_expiry,
  )));

  // Work with the cli (not really async)
  if let Some(cmd) = matches.subcommand_name() {
    info!("running in cli mode with command: {}", cmd);
    // Save, because we checked if the subcommand is present at runtime
    let m = matches.subcommand_matches(cmd).unwrap();
    #[cfg(feature = "streaming")]
    let network = Network::new(spotify, client_config, &app, final_token_cache_path); // CLI doesn't use streaming
    #[cfg(not(feature = "streaming"))]
    let network = Network::new(spotify, client_config, &app, final_token_cache_path);
    println!(
      "{}",
      cli::handle_matches(m, cmd.to_string(), network, user_config).await?
    );
  // Launch the UI (async)
  } else {
    info!("launching interactive terminal ui");
    #[cfg(feature = "streaming")]
    let (streaming_supported_for_account, streaming_startup_status_message) =
      if client_config.enable_streaming {
        account_supports_native_streaming(&spotify, &final_token_cache_path, &app).await
      } else {
        (false, None)
      };

    #[cfg(feature = "streaming")]
    if let Some(message) = streaming_startup_status_message {
      let mut app_mut = app.lock().await;
      app_mut.set_status_message(message, 12);
    }

    // Initialize streaming player if enabled
    #[cfg(feature = "streaming")]
    let streaming_player = if client_config.enable_streaming && streaming_supported_for_account {
      info!("initializing native streaming player");
      let streaming_config = player::StreamingConfig {
        device_name: client_config.streaming_device_name.clone(),
        bitrate: client_config.streaming_bitrate,
        audio_cache: client_config.streaming_audio_cache,
        cache_path: player::get_default_cache_path(),
        initial_volume: user_config.behavior.volume_percent,
      };

      let client_id = client_config.client_id.clone();
      let redirect_uri = selected_redirect_uri.clone();

      // Internal Spirc timeout defaults to 30s (configurable via
      // SPOTATUI_STREAMING_INIT_TIMEOUT_SECS). The outer timeout here is a safety net
      // that catches hangs *outside* Spirc init (e.g. OAuth callback never arriving,
      // blocking I/O in credential retrieval). Set it above the internal timeout.
      let internal_timeout_secs: u64 = std::env::var("SPOTATUI_STREAMING_INIT_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&v: &u64| v > 0)
        .unwrap_or(30);
      let outer_timeout = Duration::from_secs(internal_timeout_secs.saturating_add(15));

      let init_task = tokio::spawn(async move {
        player::StreamingPlayer::new(&client_id, &redirect_uri, streaming_config).await
      });
      let abort_handle = init_task.abort_handle();

      match tokio::time::timeout(outer_timeout, init_task).await {
        Ok(Ok(Ok(p))) => {
          info!(
            "native streaming player initialized as '{}'",
            p.device_name()
          );
          // Note: We don't activate() here - that's handled by AutoSelectStreamingDevice
          // which respects the user's saved device preference (e.g., spotifyd)
          Some(Arc::new(p))
        }
        Ok(Ok(Err(e))) => {
          info!(
            "failed to initialize streaming: {} - falling back to web api",
            e
          );
          None
        }
        Ok(Err(e)) => {
          info!(
            "streaming initialization panicked: {} - falling back to web api",
            e
          );
          None
        }
        Err(_) => {
          abort_handle.abort();
          warn!(
            "streaming initialization hung unexpectedly (outer timeout {}s) - falling back to web api",
            outer_timeout.as_secs()
          );
          None
        }
      }
    } else {
      None
    };

    #[cfg(feature = "streaming")]
    if streaming_player.is_some() {
      info!("native playback enabled - spotatui is available as a spotify connect device");
    }

    // Store streaming player reference in App for direct control (bypasses event channel)
    #[cfg(feature = "streaming")]
    {
      let mut app_mut = app.lock().await;
      app_mut.streaming_player = streaming_player.clone();
    }

    // Clone the device name for startup device selection in the network task.
    #[cfg(feature = "streaming")]
    let streaming_device_name = streaming_player
      .as_ref()
      .map(|p| p.device_name().to_string());

    // Create shared atomic for real-time position updates from native player
    // This avoids lock contention - the player event handler can update position
    // without needing to acquire the app mutex
    #[cfg(feature = "streaming")]
    let shared_position = Arc::new(AtomicU64::new(0));
    #[cfg(feature = "streaming")]
    let shared_position_for_events = Arc::clone(&shared_position);
    #[cfg(feature = "streaming")]
    let shared_position_for_ui = Arc::clone(&shared_position);

    // Create shared atomic for playing state used by native/media-control handlers.
    #[cfg(feature = "streaming")]
    let shared_is_playing = Arc::new(std::sync::atomic::AtomicBool::new(false));
    #[cfg(feature = "streaming")]
    let shared_is_playing_for_events = Arc::clone(&shared_is_playing);
    #[cfg(all(feature = "macos-media", target_os = "macos"))]
    let shared_is_playing_for_macos = Arc::clone(&shared_is_playing);
    #[cfg(feature = "streaming")]
    let (streaming_recovery_tx, streaming_recovery_rx) =
      tokio::sync::mpsc::unbounded_channel::<player::StreamingRecoveryRequest>();

    // Initialize macOS Now Playing integration for media key control.
    // Keep this independent from native streaming: if librespot fails or the user
    // selects Sonos/external Spotify Connect, media keys should still dispatch
    // through the normal playback path.
    #[cfg(all(feature = "macos-media", target_os = "macos"))]
    let macos_media_manager: Option<Arc<macos_media::MacMediaManager>> =
      match macos_media::MacMediaManager::new() {
        Ok(mgr) => {
          info!("macos now playing interface registered - media keys enabled");
          Some(Arc::new(mgr))
        }
        Err(e) => {
          info!(
            "failed to initialize macos media control: {} - media keys disabled",
            e
          );
          None
        }
      };

    // Spawn macOS media event handler to process external control requests (media keys, Control Center)
    #[cfg(all(feature = "macos-media", target_os = "macos"))]
    if let Some(ref macos_media) = macos_media_manager {
      if let Some(event_rx) = macos_media.take_event_rx() {
        let app_for_macos = Arc::clone(&app);
        tokio::spawn(async move {
          handle_macos_media_events(event_rx, app_for_macos, shared_is_playing_for_macos).await;
        });
      }
    }

    // Keep Now Playing metadata (including artwork URL from Web API playback state)
    // synchronized with Control Center.
    #[cfg(all(feature = "macos-media", target_os = "macos"))]
    if let Some(ref macos_media) = macos_media_manager {
      let macos_media_for_metadata = Arc::clone(macos_media);
      let app_for_macos_metadata = Arc::clone(&app);
      tokio::spawn(async move {
        let mut last_metadata: Option<MacosMetadata> = None;
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));

        loop {
          interval.tick().await;
          if let Ok(app) = app_for_macos_metadata.try_lock() {
            update_macos_metadata(&macos_media_for_metadata, &mut last_metadata, &app);
          }
        }
      });
    }

    // Clone macOS media manager for player event handler
    #[cfg(all(feature = "macos-media", target_os = "macos"))]
    let macos_media_for_events = macos_media_manager.clone();

    // Spawn player event listener (updates app state from native player events)
    #[cfg(feature = "streaming")]
    if let Some(ref player) = streaming_player {
      player::spawn_player_event_handler(player::PlayerEventContext {
        player: Arc::clone(player),
        app: Arc::clone(&app),
        shared_position: shared_position_for_events,
        shared_is_playing: shared_is_playing_for_events,
        recovery_tx: streaming_recovery_tx.clone(),
        #[cfg(all(feature = "macos-media", target_os = "macos"))]
        macos_media_manager: macos_media_for_events,
      });
    }

    #[cfg(feature = "streaming")]
    {
      player::spawn_streaming_recovery_handler(player::StreamingRecoveryContext {
        app: Arc::clone(&app),
        shared_position: Arc::clone(&shared_position),
        shared_is_playing: Arc::clone(&shared_is_playing),
        recovery_rx: streaming_recovery_rx,
        recovery_tx: streaming_recovery_tx.clone(),
        client_config: client_config.clone(),
        redirect_uri: selected_redirect_uri.clone(),
        #[cfg(all(feature = "macos-media", target_os = "macos"))]
        macos_media_manager: macos_media_manager.clone(),
      });
    }

    let cloned_app = Arc::clone(&app);
    info!("spawning spotify network event handler");
    tokio::spawn(async move {
      #[cfg(feature = "streaming")]
      let mut network = Network::new(spotify, client_config, &app, final_token_cache_path);
      #[cfg(not(feature = "streaming"))]
      let mut network = Network::new(spotify, client_config, &app, final_token_cache_path);

      // Restore a saved Sonos room directly. This must not depend on native
      // streaming or StartupBehavior::Play; otherwise a persisted Sonos target
      // would be ignored on the default passive Continue startup and later
      // playback commands could fall through to Spotify/native routing.
      let saved_sonos_device_id = network.client_config.device_id.clone().filter(|device_id| {
        crate::core::playback_target::parse_sonos_persisted_id(device_id).is_some()
      });

      if let Some(device_id) = saved_sonos_device_id.as_ref() {
        network
          .handle_network_event(IoEvent::TransferPlaybackToDevice(device_id.clone(), true))
          .await;
      }

      // Auto-select the saved playback device when available (fallback to native streaming).
      #[cfg(feature = "streaming")]
      if saved_sonos_device_id.is_none() {
        if let Some(device_name) = streaming_device_name {
          let saved_device_id = network.client_config.device_id.clone();
          let devices_snapshot = network
            .spotify_get_typed::<rspotify::model::device::DevicePayload>("me/player/devices", &[])
            .await
            .ok()
            .map(|devices| devices.devices);

          let startup_decision = startup_device_decision(
            initial_startup_behavior,
            saved_device_id,
            devices_snapshot.as_deref(),
            &device_name,
          );

          if devices_snapshot.is_some() || startup_decision.status_message.is_some() {
            let mut app = network.app.lock().await;
            if let Some(devices) = devices_snapshot {
              app.devices = Some(rspotify::model::device::DevicePayload { devices });
            }
            if let Some(message) = startup_decision.status_message {
              app.set_status_message(message, 5);
            }
          }

          if let Some(event) = startup_decision.event {
            network.handle_network_event(event.into_io_event()).await;
          }
        }
      }

      // Apply configured startup play behavior. Continue is passive and must not
      // transfer devices, change shuffle, or otherwise activate Spotatui.
      match initial_startup_behavior {
        StartupBehavior::Continue => {}
        StartupBehavior::Play => {
          network
            .handle_network_event(IoEvent::Shuffle(initial_shuffle_enabled))
            .await;
          network
            .handle_network_event(IoEvent::StartPlayback(None, None, None))
            .await;
        }
        StartupBehavior::Pause => {
          network.handle_network_event(IoEvent::PausePlayback).await;
        }
      }

      start_tokio(sync_io_rx, &mut network).await;
    });
    // The UI must run in the "main" thread
    info!("starting terminal ui event loop");
    #[cfg(feature = "streaming")]
    let shared_pos_for_start_ui: Option<Arc<AtomicU64>> = Some(shared_position_for_ui);
    #[cfg(not(feature = "streaming"))]
    let shared_pos_for_start_ui: Option<Arc<AtomicU64>> = None;
    crate::tui::runner::start_ui(user_config, &cloned_app, shared_pos_for_start_ui).await?;
  }

  Ok(())
}

async fn start_tokio(io_rx: std::sync::mpsc::Receiver<IoEvent>, network: &mut Network) {
  loop {
    match io_rx.try_recv() {
      Ok(io_event) => {
        network.handle_network_event(io_event).await;
      }
      Err(std::sync::mpsc::TryRecvError::Empty) => {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
      }
      Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
    }
  }
}

#[cfg_attr(
  not(all(feature = "macos-media", target_os = "macos")),
  allow(dead_code)
)]
fn macos_media_should_use_native_streaming(
  is_streaming_active: bool,
  streaming_player_available: bool,
) -> bool {
  is_streaming_active && streaming_player_available
}

/// Handle macOS media events from external sources (media keys, Control Center, AirPods, etc.).
/// Native streaming is used only when it is the active playback target; otherwise
/// commands fall back to the normal app dispatch path, which covers Sonos and
/// external Spotify Connect devices.
#[cfg(all(feature = "macos-media", target_os = "macos"))]
async fn handle_macos_media_events(
  mut event_rx: tokio::sync::mpsc::UnboundedReceiver<macos_media::MacMediaEvent>,
  app: Arc<Mutex<App>>,
  shared_is_playing: Arc<std::sync::atomic::AtomicBool>,
) {
  use macos_media::MacMediaEvent;
  use std::sync::atomic::Ordering;

  while let Some(event) = event_rx.recv().await {
    let native_player = {
      let app_lock = app.lock().await;
      let streaming_player = app_lock.streaming_player.clone();
      macos_media_should_use_native_streaming(
        app_lock.is_streaming_active,
        streaming_player.is_some(),
      )
      .then_some(streaming_player)
      .flatten()
    };

    if let Some(player) = native_player {
      match event {
        MacMediaEvent::PlayPause => {
          // Toggle based on atomic state (lock-free, always up-to-date)
          if shared_is_playing.load(Ordering::Relaxed) {
            player.pause();
          } else {
            player.play();
          }
        }
        MacMediaEvent::Play => {
          player.play();
        }
        MacMediaEvent::Pause => {
          player.pause();
        }
        MacMediaEvent::Next => {
          player.activate();
          player.next();
          // Keep Connect + audio state in sync.
          player.play();
        }
        MacMediaEvent::Previous => {
          player.activate();
          player.prev();
          // Keep Connect + audio state in sync.
          player.play();
        }
        MacMediaEvent::Stop => {
          player.stop();
        }
      }
      continue;
    }

    let mut app_lock = app.lock().await;
    match event {
      MacMediaEvent::PlayPause => app_lock.toggle_playback(),
      MacMediaEvent::Play => {
        app_lock.dispatch(IoEvent::StartPlayback(None, None, None));
      }
      MacMediaEvent::Pause | MacMediaEvent::Stop => {
        app_lock.dispatch(IoEvent::PausePlayback);
      }
      MacMediaEvent::Next => app_lock.next_track(),
      MacMediaEvent::Previous => app_lock.previous_track(),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::{
    macos_media_should_use_native_streaming, startup_device_decision, StartupDeviceEvent,
  };
  use crate::core::user_config::StartupBehavior;
  use rspotify::model::{device::Device, DeviceType};

  const NATIVE_NAME: &str = "spotatui";
  const NATIVE_ID: &str = "native-device";
  const EXTERNAL_ID: &str = "phone-device";

  #[allow(deprecated)]
  fn device(id: &str, name: &str) -> Device {
    Device {
      id: Some(id.to_string()),
      is_active: false,
      is_private_session: false,
      is_restricted: false,
      name: name.to_string(),
      _type: DeviceType::Computer,
      volume_percent: Some(50),
    }
  }

  fn startup_device_event(
    startup_behavior: StartupBehavior,
    saved_device_id: Option<String>,
    devices_snapshot: Option<&[Device]>,
  ) -> Option<StartupDeviceEvent> {
    startup_device_decision(
      startup_behavior,
      saved_device_id,
      devices_snapshot,
      NATIVE_NAME,
    )
    .event
  }

  #[test]
  fn continue_without_saved_device_does_not_transfer() {
    let devices = vec![device(NATIVE_ID, NATIVE_NAME)];

    assert_eq!(
      startup_device_event(StartupBehavior::Continue, None, Some(&devices)),
      None
    );
  }

  #[test]
  fn continue_with_saved_native_device_does_not_transfer() {
    let devices = vec![device(NATIVE_ID, NATIVE_NAME)];

    assert_eq!(
      startup_device_event(
        StartupBehavior::Continue,
        Some(NATIVE_ID.to_string()),
        Some(&devices),
      ),
      None
    );
  }

  #[test]
  fn continue_with_saved_external_device_does_not_transfer() {
    let devices = vec![
      device(EXTERNAL_ID, "Jay's phone"),
      device(NATIVE_ID, NATIVE_NAME),
    ];

    assert_eq!(
      startup_device_event(
        StartupBehavior::Continue,
        Some(EXTERNAL_ID.to_string()),
        Some(&devices),
      ),
      None
    );
  }

  #[test]
  fn play_with_saved_available_device_transfers_to_saved_device() {
    let devices = vec![
      device(EXTERNAL_ID, "Jay's phone"),
      device(NATIVE_ID, NATIVE_NAME),
    ];

    assert_eq!(
      startup_device_event(
        StartupBehavior::Play,
        Some(EXTERNAL_ID.to_string()),
        Some(&devices),
      ),
      Some(StartupDeviceEvent::Transfer {
        device_id: EXTERNAL_ID.to_string(),
        persist_device_id: true,
      })
    );
  }

  #[test]
  fn macos_media_uses_native_only_when_active_player_exists() {
    assert!(macos_media_should_use_native_streaming(true, true));
    assert!(!macos_media_should_use_native_streaming(true, false));
    assert!(!macos_media_should_use_native_streaming(false, true));
    assert!(!macos_media_should_use_native_streaming(false, false));
  }

  #[test]
  fn play_with_saved_sonos_device_transfers_without_native_fallback() {
    let devices = vec![device(NATIVE_ID, NATIVE_NAME)];
    let sonos_id = "sonos:RINCON_123".to_string();

    assert_eq!(
      startup_device_event(
        StartupBehavior::Play,
        Some(sonos_id.clone()),
        Some(&devices)
      ),
      Some(StartupDeviceEvent::Transfer {
        device_id: sonos_id,
        persist_device_id: true,
      })
    );
  }

  #[test]
  fn play_without_saved_device_auto_selects_native_fallback() {
    let devices = vec![device(NATIVE_ID, NATIVE_NAME)];

    assert_eq!(
      startup_device_event(StartupBehavior::Play, None, Some(&devices)),
      Some(StartupDeviceEvent::AutoSelectStreaming {
        device_name: NATIVE_NAME.to_string(),
        persist_device_id: true,
      })
    );
  }

  #[test]
  fn continue_with_unavailable_saved_device_does_not_fall_back_to_native() {
    let devices = vec![device(NATIVE_ID, NATIVE_NAME)];

    assert_eq!(
      startup_device_event(
        StartupBehavior::Continue,
        Some(EXTERNAL_ID.to_string()),
        Some(&devices),
      ),
      None
    );
  }

  #[test]
  fn play_with_unavailable_saved_device_transfers_to_native_without_persisting() {
    let devices = vec![device(NATIVE_ID, NATIVE_NAME)];

    let decision = startup_device_decision(
      StartupBehavior::Play,
      Some(EXTERNAL_ID.to_string()),
      Some(&devices),
      NATIVE_NAME,
    );

    assert_eq!(
      decision.event,
      Some(StartupDeviceEvent::Transfer {
        device_id: NATIVE_ID.to_string(),
        persist_device_id: false,
      })
    );
    assert_eq!(
      decision.status_message,
      Some(format!("Saved device unavailable; using {}", NATIVE_NAME))
    );
  }

  #[test]
  fn play_with_unavailable_saved_device_auto_selects_native_without_persisting() {
    let devices = vec![device("other-device", "Other speaker")];

    let decision = startup_device_decision(
      StartupBehavior::Play,
      Some(EXTERNAL_ID.to_string()),
      Some(&devices),
      NATIVE_NAME,
    );

    assert_eq!(
      decision.event,
      Some(StartupDeviceEvent::AutoSelectStreaming {
        device_name: NATIVE_NAME.to_string(),
        persist_device_id: false,
      })
    );
    assert_eq!(
      decision.status_message,
      Some(format!("Saved device unavailable; using {}", NATIVE_NAME))
    );
  }

  #[test]
  fn play_with_saved_device_and_no_snapshot_transfers_to_saved_device() {
    let decision = startup_device_decision(
      StartupBehavior::Play,
      Some(EXTERNAL_ID.to_string()),
      None,
      NATIVE_NAME,
    );

    assert_eq!(
      decision.event,
      Some(StartupDeviceEvent::Transfer {
        device_id: EXTERNAL_ID.to_string(),
        persist_device_id: true,
      })
    );
    assert_eq!(decision.status_message, None);
  }
}
