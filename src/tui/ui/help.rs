use crate::core::app::App;
use std::borrow::Cow;

pub type HelpDocRow = [Cow<'static, str>; 3];

#[cfg(feature = "cover-art")]
pub const HELP_DOCS_LEN: usize = 71;
#[cfg(not(feature = "cover-art"))]
pub const HELP_DOCS_LEN: usize = 70;

pub fn get_help_docs(app: &App) -> Vec<HelpDocRow> {
  let key_bindings = &app.user_config.keys;
  vec![
    [
      Cow::Borrowed("Scroll down to next result page"),
      Cow::Owned(key_bindings.next_page.to_string()),
      Cow::Borrowed("Pagination"),
    ],
    [
      Cow::Borrowed("Scroll up to previous result page"),
      Cow::Owned(key_bindings.previous_page.to_string()),
      Cow::Borrowed("Pagination"),
    ],
    [
      Cow::Borrowed("Jump to start of playlist"),
      Cow::Owned(key_bindings.jump_to_start.to_string()),
      Cow::Borrowed("Pagination"),
    ],
    [
      Cow::Borrowed("Jump to end of playlist"),
      Cow::Owned(key_bindings.jump_to_end.to_string()),
      Cow::Borrowed("Pagination"),
    ],
    [
      Cow::Borrowed("Jump to currently playing album"),
      Cow::Owned(key_bindings.jump_to_album.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Jump to currently playing artist's album list"),
      Cow::Owned(key_bindings.jump_to_artist_album.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Jump to current play context"),
      Cow::Owned(key_bindings.jump_to_context.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Increase volume by 10%"),
      Cow::Owned(key_bindings.increase_volume.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Decrease volume by 10%"),
      Cow::Owned(key_bindings.decrease_volume.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Skip to next track"),
      Cow::Owned(key_bindings.next_track.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Skip to previous track"),
      Cow::Owned(key_bindings.previous_track.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Force skip to previous track"),
      Cow::Owned(key_bindings.force_previous_track.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Seek backwards 5 seconds"),
      Cow::Owned(key_bindings.seek_backwards.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Seek forwards 5 seconds"),
      Cow::Owned(key_bindings.seek_forwards.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Toggle shuffle"),
      Cow::Owned(key_bindings.shuffle.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Copy url to currently playing song/episode"),
      Cow::Owned(key_bindings.copy_song_url.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Copy url to currently playing album/show"),
      Cow::Owned(key_bindings.copy_album_url.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Cycle repeat mode"),
      Cow::Owned(key_bindings.repeat.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Move selection left"),
      Cow::Borrowed("h | <Left Arrow Key> | <Ctrl+b>"),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Move selection down"),
      Cow::Borrowed("j | <Down Arrow Key> | <Ctrl+n>"),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Move selection up"),
      Cow::Borrowed("k | <Up Arrow Key> | <Ctrl+p>"),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Move selection right"),
      Cow::Borrowed("l | <Right Arrow Key> | <Ctrl+f>"),
      Cow::Borrowed("General (Ctrl+f searches inside playlist track tables)"),
    ],
    [
      Cow::Borrowed("Move selection to top of list"),
      Cow::Borrowed("H"),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Move selection to middle of list"),
      Cow::Borrowed("M"),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Move selection to bottom of list"),
      Cow::Borrowed("L"),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Enter input for search"),
      Cow::Owned(key_bindings.search.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Pause/Resume playback"),
      Cow::Owned(key_bindings.toggle_playback.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Enter active mode"),
      Cow::Borrowed("<Enter>"),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Go to lyrics view"),
      Cow::Owned(key_bindings.lyrics_view.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Toggle miniplayer view"),
      Cow::Owned(key_bindings.miniplayer_view.to_string()),
      Cow::Borrowed("General"),
    ],
    #[cfg(feature = "cover-art")]
    [
      Cow::Borrowed("Go to cover art view"),
      Cow::Owned(key_bindings.cover_art_view.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Go back or exit when nowhere left to back to"),
      Cow::Owned(key_bindings.back.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Select device to play music on"),
      Cow::Owned(key_bindings.manage_devices.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Open settings"),
      Cow::Owned(app.effective_open_settings_key().to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Save settings"),
      Cow::Owned(app.effective_save_settings_key().to_string()),
      Cow::Borrowed("Settings"),
    ],
    [
      Cow::Borrowed("Enter hover mode"),
      Cow::Borrowed("<Esc>"),
      Cow::Borrowed("Selected block"),
    ],
    [
      Cow::Borrowed("Save track in list or table"),
      Cow::Borrowed("s"),
      Cow::Borrowed("Selected block"),
    ],
    [
      Cow::Borrowed("Add selected track to playlist"),
      Cow::Borrowed("w"),
      Cow::Borrowed("Track table / search songs / artist top tracks / recently played"),
    ],
    [
      Cow::Borrowed("Add currently playing track to playlist"),
      Cow::Borrowed("w"),
      Cow::Borrowed("Playbar"),
    ],
    [
      Cow::Borrowed("Quick-add currently playing track to playlist"),
      Cow::Borrowed("W"),
      Cow::Borrowed("Global"),
    ],
    [
      Cow::Borrowed("Decrease sidebar width"),
      Cow::Borrowed("{"),
      Cow::Borrowed("Layout"),
    ],
    [
      Cow::Borrowed("Increase sidebar width"),
      Cow::Borrowed("}"),
      Cow::Borrowed("Layout"),
    ],
    [
      Cow::Borrowed("Decrease playbar or library height"),
      Cow::Borrowed("("),
      Cow::Borrowed("Layout"),
    ],
    [
      Cow::Borrowed("Increase playbar or library height"),
      Cow::Borrowed(")"),
      Cow::Borrowed("Layout"),
    ],
    [
      Cow::Borrowed("Reset layout to defaults"),
      Cow::Borrowed("|"),
      Cow::Borrowed("Layout"),
    ],
    [
      Cow::Borrowed("Remove selected track from current playlist"),
      Cow::Borrowed("x"),
      Cow::Borrowed("Track table (playlist views)"),
    ],
    [
      Cow::Borrowed("Search tracks in current playlist"),
      Cow::Borrowed("<Ctrl+f>"),
      Cow::Borrowed("Track table (playlist views)"),
    ],
    [
      Cow::Borrowed("Clear playlist track search filter"),
      Cow::Owned(key_bindings.back.to_string()),
      Cow::Borrowed("Track table (filtered playlist views)"),
    ],
    [
      Cow::Borrowed("Start playback or enter album/artist/playlist"),
      Cow::Owned(key_bindings.submit.to_string()),
      Cow::Borrowed("Selected block"),
    ],
    [
      Cow::Borrowed("Play recommendations for song/artist"),
      Cow::Borrowed("r"),
      Cow::Borrowed("Selected block"),
    ],
    [
      Cow::Borrowed("Play all tracks for artist"),
      Cow::Borrowed("e"),
      Cow::Borrowed("Library -> Artists"),
    ],
    [
      Cow::Borrowed("Search with input text"),
      Cow::Borrowed("<Enter>"),
      Cow::Borrowed("Search input"),
    ],
    [
      Cow::Borrowed("Move cursor one space left"),
      Cow::Borrowed("<Left Arrow Key>"),
      Cow::Borrowed("Search input"),
    ],
    [
      Cow::Borrowed("Move cursor one space right"),
      Cow::Borrowed("<Right Arrow Key>"),
      Cow::Borrowed("Search input"),
    ],
    [
      Cow::Borrowed("Delete entire input"),
      Cow::Borrowed("<Ctrl+l>"),
      Cow::Borrowed("Search input"),
    ],
    [
      Cow::Borrowed("Delete text from cursor to start of input"),
      Cow::Borrowed("<Ctrl+u>"),
      Cow::Borrowed("Search input"),
    ],
    [
      Cow::Borrowed("Delete text from cursor to end of input"),
      Cow::Borrowed("<Ctrl+k>"),
      Cow::Borrowed("Search input"),
    ],
    [
      Cow::Borrowed("Delete previous word"),
      Cow::Borrowed("<Ctrl+w>"),
      Cow::Borrowed("Search input"),
    ],
    [
      Cow::Borrowed("Jump to start of input"),
      Cow::Borrowed("<Ctrl+a>"),
      Cow::Borrowed("Search input"),
    ],
    [
      Cow::Borrowed("Jump to end of input"),
      Cow::Borrowed("<Ctrl+e>"),
      Cow::Borrowed("Search input"),
    ],
    [
      Cow::Borrowed("Escape from the input back to hovered block"),
      Cow::Borrowed("<Esc>"),
      Cow::Borrowed("Search input"),
    ],
    [
      Cow::Borrowed("Delete saved album"),
      Cow::Borrowed("D"),
      Cow::Borrowed("Library -> Albums"),
    ],
    [
      Cow::Borrowed("Delete saved playlist"),
      Cow::Borrowed("D"),
      Cow::Borrowed("Playlist"),
    ],
    [
      Cow::Borrowed("Follow an artist/playlist"),
      Cow::Borrowed("w"),
      Cow::Borrowed("Search result"),
    ],
    [
      Cow::Borrowed("Save (like) album to library"),
      Cow::Borrowed("w"),
      Cow::Borrowed("Search result"),
    ],
    [
      Cow::Borrowed("Play random song in playlist"),
      Cow::Borrowed("S"),
      Cow::Borrowed("Selected Playlist"),
    ],
    [
      Cow::Borrowed("Toggle sort order of podcast episodes"),
      Cow::Borrowed("S"),
      Cow::Borrowed("Selected Show"),
    ],
    [
      Cow::Borrowed("Add track to queue"),
      Cow::Owned(key_bindings.add_item_to_queue.to_string()),
      Cow::Borrowed("Hovered over track"),
    ],
    [
      Cow::Borrowed("Show queue"),
      Cow::Owned(key_bindings.show_queue.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Toggle saved state for currently playing track/episode"),
      Cow::Owned(key_bindings.like_track.to_string()),
      Cow::Borrowed("General"),
    ],
    [
      Cow::Borrowed("Open sort menu"),
      Cow::Borrowed(","),
      Cow::Borrowed("Track/Album/Artist list"),
    ],
  ]
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn help_docs_len_matches_generated_rows() {
    assert_eq!(HELP_DOCS_LEN, get_help_docs(&App::default()).len());
  }
}
