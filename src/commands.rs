use std::{fmt, str::FromStr};

use serde::Deserialize;

use crate::tag::TagId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Play {
        filter: Option<String>,
    },
    PlayPause {
        filter: Option<String>,
    },
    Shuffle {
        filter: Option<String>,
    },
    Stop,
    Next,
    Prev,
    TrackDone,
    VolumeUp,
    VolumeDown,
    Shutdown,
    AssignTag {
        id: TagId,
        command: Option<String>,
    },
    #[cfg(feature = "rpi")]
    Tag {
        id: TagId,
    },
}

impl FromStr for Command {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_command(s).ok_or_else(|| format!("Invalid command '{s}'"))
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Command::Play { filter } => write_name_with_filter(f, "PLAY", filter.as_deref()),
            Command::PlayPause { filter } => {
                write_name_with_filter(f, "PLAYPAUSE", filter.as_deref())
            }
            Command::Shuffle { filter } => write_name_with_filter(f, "SHUFFLE", filter.as_deref()),
            Command::Stop => f.write_str("STOP"),
            Command::Next => f.write_str("NEXT"),
            Command::Prev => f.write_str("PREV"),
            Command::TrackDone => f.write_str("TRACKDONE"),
            Command::VolumeUp => f.write_str("VOLUMEUP"),
            Command::VolumeDown => f.write_str("VOLUMEDOWN"),
            Command::Shutdown => f.write_str("SHUTDOWN"),
            Command::AssignTag { id, .. } => write!(f, "ASSIGN_TAG {id}"),
            #[cfg(feature = "rpi")]
            Command::Tag { id } => write!(f, "TAG {id}"),
        }
    }
}

impl<'de> Deserialize<'de> for Command {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Command::from_str(&s).map_err(serde::de::Error::custom)
    }
}

pub fn parse_command(input: &str) -> Option<Command> {
    let mut parts = input.trim().splitn(2, char::is_whitespace);
    let command = parts.next()?.to_ascii_uppercase();
    let filter = parts
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    match command.as_str() {
        "PLAY" => Some(Command::Play { filter }),
        "PLAYPAUSE" => Some(Command::PlayPause { filter }),
        "SHUFFLE" => Some(Command::Shuffle { filter }),
        "STOP" => Some(Command::Stop),
        "NEXT" => Some(Command::Next),
        "PREV" | "PREVIOUS" => Some(Command::Prev),
        "SHUTDOWN" => Some(Command::Shutdown),
        "VOLUMEUP" => Some(Command::VolumeUp),
        "VOLUMEDOWN" => Some(Command::VolumeDown),
        _ => None,
    }
}

impl Command {
    pub fn has_filter(&self) -> bool {
        matches!(
            self,
            Command::Play { .. } | Command::PlayPause { .. } | Command::Shuffle { .. }
        )
    }

    pub fn name(&self) -> &'static str {
        match self {
            Command::Play { .. } => "PLAY",
            Command::PlayPause { .. } => "PLAYPAUSE",
            Command::Shuffle { .. } => "SHUFFLE",
            Command::Stop => "STOP",
            Command::Next => "NEXT",
            Command::Prev => "PREV",
            Command::TrackDone => "TRACKDONE",
            Command::VolumeUp => "VOLUMEUP",
            Command::VolumeDown => "VOLUMEDOWN",
            Command::Shutdown => "SHUTDOWN",
            Command::AssignTag { .. } => "ASSIGN_TAG",
            #[cfg(feature = "rpi")]
            Command::Tag { .. } => "TAG",
        }
    }
}

fn write_name_with_filter(
    f: &mut fmt::Formatter<'_>,
    name: &str,
    filter: Option<&str>,
) -> fmt::Result {
    if let Some(filter) = filter {
        write!(f, "{name} {filter}")
    } else {
        f.write_str(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_commands() {
        assert_eq!(parse_command("PLAY"), Some(Command::Play { filter: None }));
        assert_eq!(parse_command("Stop"), Some(Command::Stop));
        assert_eq!(parse_command("previous"), Some(Command::Prev));
    }

    #[test]
    fn parses_with_filter() {
        assert_eq!(
            parse_command("play chill/*"),
            Some(Command::Play {
                filter: Some("chill/*".to_string())
            })
        );

        assert_eq!(
            parse_command(" shuffle   synthwave "),
            Some(Command::Shuffle {
                filter: Some("synthwave".to_string())
            })
        );
    }

    #[test]
    fn rejects_unknown() {
        assert_eq!(parse_command("dance"), None);
        assert_eq!(parse_command(""), None);
    }

    #[test]
    fn parses_via_from_str() {
        let cmd: Command = "play mix/*".parse().expect("should parse");
        assert_eq!(
            cmd,
            Command::Play {
                filter: Some("mix/*".to_string())
            }
        );
    }
}
