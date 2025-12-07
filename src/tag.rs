use std::str::FromStr;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TagId([u8; 4]);

impl TagId {
    pub fn from_uid(uid: [u8; 4]) -> Self {
        Self(uid)
    }

    pub fn from_hex_str(s: &str) -> Result<Self, String> {
        let trimmed = s.trim();
        if trimmed.len() != 8 {
            return Err("RFID tag IDs must be 8 hexadecimal characters".to_string());
        }

        let bytes = trimmed
            .as_bytes()
            .chunks(2)
            .map(std::str::from_utf8)
            .map(|chunk| chunk.map_err(|err| err.to_string()))
            .map(|res| {
                res.and_then(|hex| u8::from_str_radix(hex, 16).map_err(|err| err.to_string()))
            })
            .collect::<Result<Vec<u8>, String>>()?;

        let bytes: [u8; 4] = bytes
            .try_into()
            .map_err(|_| "RFID tag IDs must be exactly 4 bytes (8 hex chars)".to_string())?;

        Ok(Self(bytes))
    }
}

impl std::fmt::Display for TagId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02X}")?;
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for TagId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        TagId::from_hex_str(&s).map_err(serde::de::Error::custom)
    }
}

impl FromStr for TagId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        TagId::from_hex_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_id_parses_hex() {
        let tag = TagId::from_hex_str("0a1b2c3d").expect("valid hex");
        assert_eq!(format!("{tag}"), "0A1B2C3D");
    }

    #[test]
    fn tag_id_rejects_wrong_length() {
        assert!(TagId::from_hex_str("123").is_err());
    }

    #[test]
    fn parses_via_from_str() {
        let tag: TagId = "0a1b2c3d".parse().expect("should parse");
        assert_eq!(format!("{tag}"), "0A1B2C3D");
    }
}
