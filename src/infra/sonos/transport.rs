use crate::core::playback_target::SonosRoom;
use crate::infra::sonos::spotify::SonosSpotifyItem;
use anyhow::{Context, Result};
use std::time::Duration;

const AV_TRANSPORT_URN: &str = "urn:schemas-upnp-org:service:AVTransport:1";
const RENDERING_CONTROL_URN: &str = "urn:schemas-upnp-org:service:RenderingControl:1";

pub struct SonosTransport {
  client: reqwest::Client,
}

impl SonosTransport {
  pub fn new() -> Result<Self> {
    Ok(Self {
      client: reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
        .context("failed to build Sonos HTTP client")?,
    })
  }

  pub async fn play_spotify_item(&self, room: &SonosRoom, item: &SonosSpotifyItem) -> Result<()> {
    let mut last_error = None;
    for service_number in [2311_u32, 3079_u32] {
      match self
        .try_play_spotify_item_with_service_number(room, item, service_number)
        .await
      {
        Ok(_) => return Ok(()),
        Err(err) => last_error = Some(err),
      }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Sonos rejected Spotify playback")))
  }

  pub async fn play(&self, room: &SonosRoom) -> Result<()> {
    self
      .soap(
        room,
        "MediaRenderer/AVTransport/Control",
        AV_TRANSPORT_URN,
        "Play",
        "<Speed>1</Speed>",
      )
      .await
  }

  pub async fn pause(&self, room: &SonosRoom) -> Result<()> {
    self
      .soap(
        room,
        "MediaRenderer/AVTransport/Control",
        AV_TRANSPORT_URN,
        "Pause",
        "",
      )
      .await
  }

  pub async fn next(&self, room: &SonosRoom) -> Result<()> {
    self
      .soap(
        room,
        "MediaRenderer/AVTransport/Control",
        AV_TRANSPORT_URN,
        "Next",
        "",
      )
      .await
  }

  pub async fn previous(&self, room: &SonosRoom) -> Result<()> {
    self
      .soap(
        room,
        "MediaRenderer/AVTransport/Control",
        AV_TRANSPORT_URN,
        "Previous",
        "",
      )
      .await
  }

  pub async fn seek(&self, room: &SonosRoom, position_ms: u32) -> Result<()> {
    let body = format!(
      "<Unit>REL_TIME</Unit><Target>{}</Target>",
      format_duration(position_ms)
    );
    self
      .soap(
        room,
        "MediaRenderer/AVTransport/Control",
        AV_TRANSPORT_URN,
        "Seek",
        &body,
      )
      .await
  }

  pub async fn set_volume(&self, room: &SonosRoom, volume: u8) -> Result<()> {
    let body = format!(
      "<Channel>Master</Channel><DesiredVolume>{}</DesiredVolume>",
      volume.min(100)
    );
    self
      .soap(
        room,
        "MediaRenderer/RenderingControl/Control",
        RENDERING_CONTROL_URN,
        "SetVolume",
        &body,
      )
      .await
  }

  async fn add_uri_to_queue(
    &self,
    room: &SonosRoom,
    enqueued_uri: &str,
    enqueued_metadata: &str,
  ) -> Result<()> {
    let body = format!(
      "<EnqueuedURI>{}</EnqueuedURI><EnqueuedURIMetaData>{}</EnqueuedURIMetaData><DesiredFirstTrackNumberEnqueued>1</DesiredFirstTrackNumberEnqueued><EnqueueAsNext>1</EnqueueAsNext>",
      escape_xml(enqueued_uri),
      escape_xml(enqueued_metadata)
    );
    self
      .soap(
        room,
        "MediaRenderer/AVTransport/Control",
        AV_TRANSPORT_URN,
        "AddURIToQueue",
        &body,
      )
      .await
  }

  async fn try_play_spotify_item_with_service_number(
    &self,
    room: &SonosRoom,
    item: &SonosSpotifyItem,
    service_number: u32,
  ) -> Result<()> {
    let service_token = format!("SA_RINCON{service_number}_X_#Svc{service_number}-0-Token");
    let metadata = item
      .enqueued_metadata
      .replace("SA_RINCON2311_X_#Svc2311-0-Token", &service_token);

    self
      .add_uri_to_queue(room, &item.enqueued_uri, &metadata)
      .await?;
    self.play(room).await
  }

  async fn soap(
    &self,
    room: &SonosRoom,
    control_path: &str,
    service_urn: &str,
    action: &str,
    action_body: &str,
  ) -> Result<()> {
    let url = control_url(&room.location, control_path)?;
    let envelope = soap_envelope(service_urn, action, action_body);
    self
      .client
      .post(&url)
      .header("Content-Type", "text/xml; charset=\"utf-8\"")
      .header("SOAPACTION", format!("\"{service_urn}#{action}\""))
      .body(envelope)
      .send()
      .await
      .with_context(|| format!("failed to send Sonos {action} command to {}", room.name))?
      .error_for_status()
      .with_context(|| format!("Sonos room {} rejected {action}", room.name))?;
    Ok(())
  }
}

fn control_url(location: &str, control_path: &str) -> Result<String> {
  let parsed = url::Url::parse(location).context("invalid Sonos device description URL")?;
  let origin = parsed.origin().ascii_serialization();
  Ok(format!("{origin}/{control_path}"))
}

fn soap_envelope(service_urn: &str, action: &str, action_body: &str) -> String {
  format!(
    r#"<?xml version="1.0" encoding="utf-8"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:{action} xmlns:u="{service_urn}"><InstanceID>0</InstanceID>{action_body}</u:{action}></s:Body></s:Envelope>"#
  )
}

fn escape_xml(value: &str) -> String {
  value
    .replace('&', "&amp;")
    .replace('<', "&lt;")
    .replace('>', "&gt;")
    .replace('"', "&quot;")
    .replace('\'', "&apos;")
}

fn format_duration(position_ms: u32) -> String {
  let total_seconds = position_ms / 1_000;
  let hours = total_seconds / 3_600;
  let minutes = (total_seconds % 3_600) / 60;
  let seconds = total_seconds % 60;
  format!("{hours}:{minutes:02}:{seconds:02}")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn builds_control_url_from_device_description_location() {
    assert_eq!(
      control_url(
        "http://192.168.1.20:1400/xml/device_description.xml",
        "MediaRenderer/AVTransport/Control"
      )
      .unwrap(),
      "http://192.168.1.20:1400/MediaRenderer/AVTransport/Control"
    );
  }

  #[test]
  fn formats_sonos_seek_duration() {
    assert_eq!(format_duration(3_723_000), "1:02:03");
  }
}
