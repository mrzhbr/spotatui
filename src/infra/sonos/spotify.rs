use anyhow::{anyhow, Result};
use rspotify::model::idtypes::{PlayContextId, PlayableId};
use rspotify::prelude::Id;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SonosSpotifyItem {
  pub spotify_uri: String,
  pub enqueued_uri: String,
  pub enqueued_metadata: String,
}

pub fn item_from_playback_request(
  context_id: Option<&PlayContextId<'static>>,
  uris: Option<&[PlayableId<'static>]>,
  offset: Option<usize>,
) -> Result<SonosSpotifyItem> {
  let spotify_uri = if let Some(context) = context_id {
    context.uri()
  } else if let Some(uris) = uris {
    let index = offset.unwrap_or(0).min(uris.len().saturating_sub(1));
    uris
      .get(index)
      .map(Id::uri)
      .ok_or_else(|| anyhow!("No Spotify URI selected for Sonos playback"))?
  } else {
    return Err(anyhow!(
      "Sonos cannot resume playback until a Spotify item has been selected"
    ));
  };

  item_from_spotify_uri(&spotify_uri)
}

pub fn item_from_spotify_uri(spotify_uri: &str) -> Result<SonosSpotifyItem> {
  let item_type = spotify_uri
    .strip_prefix("spotify:")
    .and_then(|rest| rest.split(':').next())
    .ok_or_else(|| anyhow!("Unsupported Spotify URI for Sonos: {spotify_uri}"))?;
  let item_id = spotify_uri
    .rsplit(':')
    .next()
    .ok_or_else(|| anyhow!("Unsupported Spotify URI for Sonos: {spotify_uri}"))?;

  let (enqueued_uri, item_class, item_tag_id) = match item_type {
    "track" => (
      format!(
        "x-sonos-spotify:spotify%3Atrack%3A{}?sid=9&flags=8224&sn=7",
        percent_encode(item_id)
      ),
      "object.item.audioItem.musicTrack",
      format!("spotify:track:{item_id}"),
    ),
    "album" => (
      format!(
        "x-rincon-cpcontainer:0004206cspotify%3Aalbum%3A{}?sid=9&flags=8300&sn=7",
        percent_encode(item_id)
      ),
      "object.container.album.musicAlbum",
      format!("0004206cspotify:album:{item_id}"),
    ),
    "playlist" => (
      format!(
        "x-rincon-cpcontainer:1006206cspotify%3Aplaylist%3A{}?sid=9&flags=8300&sn=7",
        percent_encode(item_id)
      ),
      "object.container.playlistContainer",
      format!("1006206cspotify:playlist:{item_id}"),
    ),
    "show" => (
      format!(
        "x-rincon-cpcontainer:1004206cspotify%3Ashow%3A{}?sid=9&flags=8300&sn=7",
        percent_encode(item_id)
      ),
      "object.container.album.audioShow",
      format!("1004206cspotify:show:{item_id}"),
    ),
    "episode" => (
      format!(
        "x-sonos-spotify:spotify%3Aepisode%3A{}?sid=9&flags=8224&sn=7",
        percent_encode(item_id)
      ),
      "object.item.audioItem.audioBroadcast",
      format!("spotify:episode:{item_id}"),
    ),
    _ => {
      return Err(anyhow!(
        "Unsupported Spotify item type for Sonos: {item_type}"
      ))
    }
  };

  let didl = sonos_metadata(item_class, &item_tag_id, spotify_uri);

  Ok(SonosSpotifyItem {
    spotify_uri: spotify_uri.to_string(),
    enqueued_uri,
    enqueued_metadata: didl,
  })
}

fn escape_xml(value: &str) -> String {
  value
    .replace('&', "&amp;")
    .replace('<', "&lt;")
    .replace('>', "&gt;")
    .replace('"', "&quot;")
    .replace('\'', "&apos;")
}

fn percent_encode(value: &str) -> String {
  value
    .bytes()
    .map(|byte| match byte {
      b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
        (byte as char).to_string()
      }
      _ => format!("%{byte:02X}"),
    })
    .collect()
}

fn sonos_metadata(item_class: &str, item_id: &str, spotify_uri: &str) -> String {
  let escaped_id = escape_xml(item_id);
  let escaped_title = escape_xml(spotify_uri);
  let service_token = "SA_RINCON2311_X_#Svc2311-0-Token";

  format!(
    r#"<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/" xmlns:r="urn:schemas-rinconnetworks-com:metadata-1-0/" xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/"><item id="{escaped_id}" restricted="true"><dc:title>{escaped_title}</dc:title><upnp:class>{item_class}</upnp:class><desc id="cdudn" nameSpace="urn:schemas-rinconnetworks-com:metadata-1-0/">{service_token}</desc></item></DIDL-Lite>"#
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn converts_track_uri_to_sonos_uri() {
    let item = item_from_spotify_uri("spotify:track:abc123").unwrap();
    assert_eq!(
      item.enqueued_uri,
      "x-sonos-spotify:spotify%3Atrack%3Aabc123?sid=9&flags=8224&sn=7"
    );
    assert!(item
      .enqueued_metadata
      .contains("object.item.audioItem.musicTrack"));
  }

  #[test]
  fn escapes_didl_xml() {
    let item = item_from_spotify_uri("spotify:playlist:a&b").unwrap();
    assert!(item.enqueued_metadata.contains("spotify:playlist:a&amp;b"));
    assert!(item
      .enqueued_uri
      .contains("x-rincon-cpcontainer:1006206cspotify%3Aplaylist%3Aa%26b?sid=9&flags=8300&sn=7"));
  }

  #[test]
  fn rejects_unsupported_types() {
    assert!(item_from_spotify_uri("spotify:artist:abc").is_err());
  }
}
