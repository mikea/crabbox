use std::str::FromStr;

use serde::Deserialize;

use crate::tag::TagId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Play { filter: Option<String> },
    PlayPause { filter: Option<String> },
    Shuffle { filter: Option<String> },
    Stop,
    Next,
    Prev,
    VolumeUp,
    VolumeDown,
    Shutdown,
    Tag { id: TagId },
}

impl FromStr for Command {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_command(s).ok_or_else(|| format!("Invalid command '{s}'"))
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
