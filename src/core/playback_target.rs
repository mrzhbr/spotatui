use rspotify::model::device::Device;

pub const SONOS_DEVICE_ID_PREFIX: &str = "sonos:";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SonosRoom {
  pub uuid: String,
  pub name: String,
  pub location: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaybackTarget {
  Spotify {
    id: String,
    name: String,
    is_active: bool,
  },
  Sonos {
    room: SonosRoom,
    is_selected: bool,
  },
}

impl PlaybackTarget {
  pub fn label(&self) -> String {
    match self {
      PlaybackTarget::Spotify {
        name, is_active, ..
      } => {
        if *is_active {
          format!("{name} (Spotify, active)")
        } else {
          format!("{name} (Spotify)")
        }
      }
      PlaybackTarget::Sonos { room, is_selected } => {
        if *is_selected {
          format!("{} (Sonos, selected)", room.name)
        } else {
          format!("{} (Sonos)", room.name)
        }
      }
    }
  }
}

pub fn sonos_persisted_id(uuid: &str) -> String {
  format!("{SONOS_DEVICE_ID_PREFIX}{uuid}")
}

pub fn parse_sonos_persisted_id(device_id: &str) -> Option<&str> {
  device_id.strip_prefix(SONOS_DEVICE_ID_PREFIX)
}

pub fn spotify_target_from_device(device: &Device) -> Option<PlaybackTarget> {
  Some(PlaybackTarget::Spotify {
    id: device.id.clone()?,
    name: device.name.clone(),
    is_active: device.is_active,
  })
}
