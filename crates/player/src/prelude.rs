//! The one import most users need: `use player::prelude::*;`.
//!
//! Re-exports the player handle and the types used to configure, drive and observe it.

pub use crate::error::{Error, Result};

pub use crate::builder::{Autoplay, PlayerBuilder, VideoOutput};
pub use crate::event::{EndReason, Event, Events, State};
pub use crate::mpv::PropertyValue;
pub use crate::player::{Player, Seek};
pub use crate::source::Source;
