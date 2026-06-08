use crate::core::playback_target::{parse_sonos_persisted_id, sonos_persisted_id};
use crate::core::user_config::UserConfig;
use crate::infra::network::{IoEvent, Network};

use super::{
  util::{Flag, JumpDirection, Type},
  CliApp,
};

use anyhow::{anyhow, Result};
use clap::ArgMatches;

// Handle the different subcommands
pub async fn handle_matches(
  matches: &ArgMatches,
  cmd: String,
  net: Network,
  config: UserConfig,
) -> Result<String> {
  let mut cli = CliApp::new(net, config);
  let playback_command = matches!(cmd.as_str(), "playback" | "play");
  let should_load_devices = matches.get_one::<String>("device").is_some()
    || (cmd == "playback" && matches.get_one::<String>("transfer").is_some())
    || (cmd == "list" && matches.get_flag("devices"))
    || (playback_command && cli.net.client_config.device_id.is_none());

  if should_load_devices {
    cli.net.handle_network_event(IoEvent::GetDevices).await;

    let devices_list = {
      let app = cli.net.app.lock().await;
      let mut devices = app
        .devices
        .as_ref()
        .map(|p| {
          p.devices
            .iter()
            .filter_map(|d| d.id.clone())
            .collect::<Vec<String>>()
        })
        .unwrap_or_default();
      devices.extend(
        app
          .sonos_rooms
          .iter()
          .map(|room| sonos_persisted_id(&room.uuid)),
      );
      devices
    };

    // If the device_id is not specified, select the first available Spotify device.
    // A saved Sonos room may be temporarily undiscoverable; keep it selected so CLI
    // playback commands do not silently overwrite it and route to Spotify instead.
    let device_id = cli.net.client_config.device_id.clone();
    let needs_device = match &device_id {
      Some(id) if parse_sonos_persisted_id(id).is_some() => false,
      Some(id) => !devices_list.contains(id),
      None => playback_command,
    };
    if needs_device {
      if let Some(d) = devices_list
        .iter()
        .find(|device_id| parse_sonos_persisted_id(device_id).is_none())
        .or_else(|| devices_list.first())
      {
        cli.net.client_config.set_device_id(d.clone())?;
      }
    }

    if let Some(d) = matches.get_one::<String>("device") {
      cli.set_device(d.to_string()).await?;
    }
  }

  if playback_command {
    if let Some(device_id) = cli
      .net
      .client_config
      .device_id
      .clone()
      .filter(|device_id| parse_sonos_persisted_id(device_id).is_some())
    {
      cli
        .net
        .handle_network_event(IoEvent::TransferPlaybackToDevice(device_id, true))
        .await;
    }

    cli
      .net
      .handle_network_event(IoEvent::GetCurrentPlayback)
      .await;
  }

  // Evalute the subcommand
  let output = match cmd.as_str() {
    "playback" => {
      let format = matches.get_one::<String>("format").unwrap();

      // Commands that are 'single'
      if matches.get_flag("share-track") {
        return cli.share_track_or_episode().await;
      } else if matches.get_flag("share-album") {
        return cli.share_album_or_show().await;
      }

      // Run the action, and print out the status
      // No 'else if's because multiple different commands are possible
      if matches.get_flag("toggle") {
        cli.toggle_playback().await;
      }
      if let Some(d) = matches.get_one::<String>("transfer") {
        cli.transfer_playback(d).await?;
      }
      // Handle flags (like, dislike, shuffle, repeat)
      let flags = Flag::from_matches(matches);
      for f in flags {
        cli.mark(f).await?;
      }
      if matches.get_count("next") > 0 || matches.get_count("previous") > 0 {
        let (direction, amount) = JumpDirection::from_matches(matches);
        for _ in 0..amount {
          cli.jump(&direction).await;
        }
      }
      if let Some(vol) = matches.get_one::<String>("volume") {
        cli.volume(vol.to_string()).await?;
      }
      if let Some(secs) = matches.get_one::<String>("seek") {
        cli.seek(secs.to_string()).await?;
      }

      // Print out the status if no errors were found
      cli.get_status(format.to_string()).await
    }
    "play" => {
      let queue = matches.get_flag("queue");
      let random = matches.get_flag("random");
      let format = matches.get_one::<String>("format").unwrap();

      if let Some(uri) = matches.get_one::<String>("uri") {
        cli.play_uri(uri.to_string(), queue, random).await;
      } else if let Some(name) = matches.get_one::<String>("name") {
        let category = Type::play_from_matches(matches);
        cli.play(name.to_string(), category, queue, random).await?;
      }

      cli.get_status(format.to_string()).await
    }
    "list" => {
      let format = matches.get_one::<String>("format").unwrap().to_string();

      // Update the limits for the list and search functions
      // I think the small and big search limits are very confusing
      // so I just set them both to max, is this okay?
      if let Some(max) = matches.get_one::<String>("limit") {
        cli.update_query_limits(max.to_string()).await?;
      }

      let category = Type::list_from_matches(matches);
      Ok(cli.list(category, &format).await)
    }
    "search" => {
      let format = matches.get_one::<String>("format").unwrap().to_string();

      // Update the limits for the list and search functions
      // I think the small and big search limits are very confusing
      // so I just set them both to max, is this okay?
      if let Some(max) = matches.get_one::<String>("limit") {
        cli.update_query_limits(max.to_string()).await?;
      }

      let category = Type::search_from_matches(matches);
      Ok(
        cli
          .query(
            matches.get_one::<String>("search").unwrap().to_string(),
            format,
            category,
          )
          .await,
      )
    }
    // Clap enforces that one of the things above is specified
    _ => unreachable!(),
  };

  // Check if there was an error
  let api_error = cli.net.app.lock().await.api_error.clone();
  if api_error.is_empty() {
    output
  } else {
    Err(anyhow!("{}", api_error))
  }
}
