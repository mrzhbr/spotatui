use super::Network;
use crate::core::app::LyricsStatus;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[allow(non_snake_case)]
struct LrcResponse {
  syncedLyrics: Option<String>,
  plainLyrics: Option<String>,
}

pub trait UtilsNetwork {
  async fn get_lyrics(&mut self, track: String, artist: String, duration: f64);
}

impl UtilsNetwork for Network {
  async fn get_lyrics(&mut self, track: String, artist: String, duration: f64) {
    let client = reqwest::Client::new();
    let query = vec![
      ("track_name", track.clone()),
      ("artist_name", artist.clone()),
      ("duration", duration.to_string()),
    ];

    // Update state to loading
    {
      let mut app = self.app.lock().await;
      app.lyrics_status = LyricsStatus::Loading;
      app.lyrics = None;
    }

    match client
      .get("https://lrclib.net/api/get")
      .query(&query)
      .send()
      .await
    {
      Ok(resp) => {
        if resp.status().is_success() {
          if let Ok(lrc_resp) = resp.json::<LrcResponse>().await {
            let lyrics_text = lrc_resp
              .syncedLyrics
              .or(lrc_resp.plainLyrics)
              .unwrap_or_default();

            if !lyrics_text.is_empty() {
              let mut app = self.app.lock().await;
              // Simple LRC parser
              let parsed: Vec<(u128, String)> = lyrics_text
                .lines()
                .filter_map(|line| {
                  // [mm:ss.xx] text
                  if let Some(idx) = line.find(']') {
                    if idx > 1 && line.starts_with('[') {
                      let timestamp = &line[1..idx];
                      let content = line[idx + 1..].trim().to_string();

                      // Parse timestamp
                      let parts: Vec<&str> = timestamp.split(':').collect();
                      if parts.len() == 2 {
                        let mins = parts[0].parse::<u64>().unwrap_or(0);
                        let secs_parts: Vec<&str> = parts[1].split('.').collect();
                        let secs = secs_parts[0].parse::<u64>().unwrap_or(0);
                        let ms = if secs_parts.len() > 1 {
                          // Handle 2 or 3 digit ms
                          let ms_str = secs_parts[1];
                          let ms_val = ms_str.parse::<u64>().unwrap_or(0);
                          if ms_str.len() == 2 {
                            ms_val * 10
                          } else {
                            ms_val
                          }
                        } else {
                          0
                        };

                        let total_ms = (mins * 60 * 1000) + (secs * 1000) + ms;
                        return Some((total_ms as u128, content));
                      }
                    }
                  }
                  None
                })
                .collect();

              if !parsed.is_empty() {
                app.lyrics = Some(parsed);
                app.lyrics_status = LyricsStatus::Found;
              } else {
                app.lyrics_status = LyricsStatus::NotFound;
              }
            } else {
              let mut app = self.app.lock().await;
              app.lyrics_status = LyricsStatus::NotFound;
            }
          }
        } else {
          let mut app = self.app.lock().await;
          app.lyrics_status = LyricsStatus::NotFound;
        }
      }
      Err(_) => {
        let mut app = self.app.lock().await;
        app.lyrics_status = LyricsStatus::NotFound;
      }
    }
  }
}
