mod clap;
mod cli_app;
mod handle;
#[cfg(feature = "self-update")]
mod update;
mod util;

pub use self::clap::{list_subcommand, play_subcommand, playback_subcommand, search_subcommand};
use cli_app::CliApp;
pub use handle::handle_matches;
#[cfg(feature = "self-update")]
pub use update::{check_for_update, install_update_silent, parse_delay_secs, UpdateOutcome};
