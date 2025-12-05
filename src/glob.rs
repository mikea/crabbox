use std::path::Path;

use regex::Regex;

pub fn glob_to_regex(pattern: &str) -> Result<Regex, regex::Error> {
    let mut regex_str = String::from("^");
    let mut literal = String::new();
    let mut chars = pattern.chars().peekable();

    let flush_literal = |buf: &mut String, out: &mut String| {
        if !buf.is_empty() {
            out.push_str(&regex::escape(buf));
            buf.clear();
        }
    };

    while let Some(ch) = chars.next() {
        match ch {
            '*' => {
                flush_literal(&mut literal, &mut regex_str);
                regex_str.push_str(".*");
            }
            '?' => {
                flush_literal(&mut literal, &mut regex_str);
                regex_str.push('.');
            }
            '\\' => {
                flush_literal(&mut literal, &mut regex_str);
                if let Some(next) = chars.next() {
                    regex_str.push_str(&regex::escape(&next.to_string()));
                } else {
                    regex_str.push_str(r"\\");
                }
            }
            _ => literal.push(ch),
        }
    }

    flush_literal(&mut literal, &mut regex_str);
    regex_str.push('$');

    Regex::new(&regex_str)
}

pub struct Glob {
    regex: Regex,
}

impl Glob {
    pub fn new(pattern: &str) -> Result<Self, regex::Error> {
        let regex = glob_to_regex(pattern)?;
        Ok(Self { regex })
    }

    pub fn is_match(&self, value: &str) -> bool {
        self.regex.is_match(value)
    }

    pub fn is_match_path(&self, path: &Path) -> bool {
        let value = path.to_string_lossy();
        self.is_match(&value)
    }
}

#[cfg(test)]
mod tests {
    use super::{Glob, glob_to_regex};
    use std::path::Path;

    fn glob_matches(pattern: &str, path: &Path) -> Result<bool, regex::Error> {
        let matcher = Glob::new(pattern)?;
        Ok(matcher.is_match_path(path))
    }

    #[test]
    fn literal_matches_exactly() {
        let glob = Glob::new("song.mp3").unwrap();
        assert!(glob.is_match("song.mp3"));
        assert!(!glob.is_match("song.mp4"));
    }

    #[test]
    fn wildcard_star_expands() {
        let glob = Glob::new("rock-*").unwrap();
        assert!(glob.is_match("rock-anthem"));
        assert!(glob.is_match("rock-"));
        assert!(!glob.is_match("pop-anthem"));
    }

    #[test]
    fn wildcard_question_matches_single_char() {
        let glob = Glob::new("a?c").unwrap();
        assert!(glob.is_match("abc"));
        assert!(!glob.is_match("abbc"));
        assert!(!glob.is_match("ac"));
    }

    #[test]
    fn mixed_wildcards() {
        let glob = Glob::new("*track??.flac").unwrap();
        assert!(glob.is_match("cooltrack01.flac"));
        assert!(!glob.is_match("track1.flac"));
        assert!(!glob.is_match("cooltrack001.flac"));
    }

    #[test]
    fn matches_full_path() {
        let glob = Glob::new("music/*/song?.mp3").unwrap();
        assert!(glob.is_match("music/rock/song1.mp3"));
        assert!(glob.is_match("music//song2.mp3"));
        assert!(!glob.is_match("music/rock/song12.mp3"));
    }

    #[test]
    fn escape_star_and_question() {
        let glob = Glob::new(r"file\*name\?").unwrap();
        assert!(glob.is_match("file*name?"));
        assert!(!glob.is_match("filename"));
        assert!(!glob.is_match("fileXnameY"));
    }

    #[test]
    fn regex_metacharacters_are_escaped() {
        let glob = Glob::new("song.(v1)").unwrap();
        assert!(glob.is_match("song.(v1)"));
        assert!(!glob.is_match("song-av1"));
    }

    #[test]
    fn trailing_backslash_is_literal() {
        let glob = Glob::new(r"path\\").unwrap();
        assert!(glob.is_match("path\\"));
        assert!(!glob.is_match("path/"));
    }

    #[test]
    fn glob_matches_path_helper() {
        let path = Path::new("/music/rock/anthem.mp3");
        assert!(glob_matches("*/rock/*.mp3", path).unwrap());
        assert!(!glob_matches("*/jazz/*.mp3", path).unwrap());
    }

    #[test]
    fn empty_pattern_matches_only_empty_string() {
        let glob = Glob::new("").unwrap();
        assert!(glob.is_match(""));
        assert!(!glob.is_match("anything"));
    }

    #[test]
    fn glob_to_regex_builds_anchored_patterns() {
        let regex = glob_to_regex("song*").unwrap();
        assert!(regex.is_match("song"));
        assert!(regex.is_match("song-extended"));
        assert!(!regex.is_match("best song"));
    }
}
