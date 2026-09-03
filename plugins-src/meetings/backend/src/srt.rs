use crate::model::SpeakerMeta;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default)]
pub struct SpeakerLookup {
    pub canonical: BTreeMap<String, String>,
    pub metadata: BTreeMap<String, SpeakerMeta>,
}

#[derive(Clone, Debug)]
pub struct SrtValidation {
    pub source_labels: BTreeSet<String>,
    pub canonical_labels: BTreeSet<String>,
    pub cue_count: usize,
}

#[derive(Clone, Debug)]
pub struct MarkdownValidation {
    pub speakers: Vec<String>,
    pub segment_count: usize,
}

/// Validate Hemory's generated `content.md`. Its optional header is bounded by
/// two `---` lines; every non-empty body line is a transcript segment in the
/// production format `HH:MM:SS  speaker: text`.
pub fn validate_content_markdown(bytes: &[u8]) -> Result<MarkdownValidation, String> {
    let raw = std::str::from_utf8(bytes)
        .map_err(|_| "content.md is not UTF-8".to_string())?
        .strip_prefix('\u{feff}')
        .unwrap_or_else(|| std::str::from_utf8(bytes).unwrap());
    let normalized = raw.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.lines().collect();
    let mut body_start = 0;
    if lines
        .iter()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().starts_with('#'))
        == Some(true)
    {
        let separators: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| (line.trim() == "---").then_some(index))
            .collect();
        if separators.len() < 2 {
            return Err("content.md header is not closed by a second ---".into());
        }
        body_start = separators[1] + 1;
    }
    let segment = Regex::new(r"^(\d{2}):(\d{2}):(\d{2})\s+([^:\r\n]+):\s*(\S.*)$").unwrap();
    let mut last_start = 0_u64;
    let mut segment_count = 0;
    let mut speakers = Vec::new();
    for line in &lines[body_start..] {
        if line.trim().is_empty() {
            continue;
        }
        let captures = segment.captures(line).ok_or_else(|| {
            format!("content.md body line has no parseable time and speaker: {line}")
        })?;
        let hour: u64 = captures[1].parse().unwrap();
        let minute: u64 = captures[2].parse().unwrap();
        let second: u64 = captures[3].parse().unwrap();
        if minute > 59 || second > 59 {
            return Err(format!("content.md has invalid timestamp: {line}"));
        }
        let start = hour * 3600 + minute * 60 + second;
        if segment_count > 0 && start < last_start {
            return Err(format!("content.md segment order goes backwards: {line}"));
        }
        let speaker = captures[4].trim().to_string();
        if !speakers.contains(&speaker) {
            speakers.push(speaker);
        }
        segment_count += 1;
        last_start = start;
    }
    if segment_count == 0 {
        return Err("content.md contains no transcript segments".into());
    }
    Ok(MarkdownValidation {
        speakers,
        segment_count,
    })
}

fn timestamp_ms(text: &str) -> Option<u64> {
    let re = Regex::new(r"^(\d{2}):(\d{2}):(\d{2})[,.](\d{3})$").unwrap();
    let captures = re.captures(text.trim())?;
    let hour: u64 = captures[1].parse().ok()?;
    let minute: u64 = captures[2].parse().ok()?;
    let second: u64 = captures[3].parse().ok()?;
    let millis: u64 = captures[4].parse().ok()?;
    if minute > 59 || second > 59 {
        return None;
    }
    Some((((hour * 60) + minute) * 60 + second) * 1000 + millis)
}

pub fn canonical_builtin(label: &str) -> Option<String> {
    let direct = Regex::new(r"^spk_\d+$").unwrap();
    if direct.is_match(label) {
        return Some(label.to_string());
    }
    let segmented = Regex::new(r"^\d+_(spk_\d+)$").unwrap();
    segmented
        .captures(label)
        .map(|captures| captures[1].to_string())
}

pub fn validate_srt(bytes: &[u8], lookup: &SpeakerLookup) -> Result<SrtValidation, String> {
    let raw = std::str::from_utf8(bytes).map_err(|_| "SRT is not UTF-8".to_string())?;
    let raw = raw
        .strip_prefix('\u{feff}')
        .unwrap_or(raw)
        .replace("\r\n", "\n");
    let timeline = Regex::new(r"^\s*([^\s]+)\s+-->\s+([^\s]+)(?:\s+.*)?$").unwrap();
    let speaker = Regex::new(r"^\s*\[([^\]\r\n]+)\]\s*(.*)$").unwrap();
    let mut last_seq = 0_u64;
    let mut last_start = 0_u64;
    let mut nonempty = 0_usize;
    let mut source_labels = BTreeSet::new();
    let mut canonical_labels = BTreeSet::new();

    for block in raw.split("\n\n").filter(|block| !block.trim().is_empty()) {
        let lines: Vec<&str> = block.lines().collect();
        if lines.len() < 3 {
            return Err("SRT cue must contain sequence, timeline, and text".into());
        }
        let seq: u64 = lines[0]
            .trim()
            .parse()
            .map_err(|_| format!("invalid SRT sequence: {}", lines[0].trim()))?;
        if seq == 0 || seq <= last_seq {
            return Err(format!("SRT sequence is not strictly increasing at {seq}"));
        }
        let cap = timeline
            .captures(lines[1])
            .ok_or_else(|| format!("invalid SRT timeline: {}", lines[1].trim()))?;
        let start = timestamp_ms(&cap[1])
            .ok_or_else(|| format!("invalid SRT start timestamp: {}", &cap[1]))?;
        let end = timestamp_ms(&cap[2])
            .ok_or_else(|| format!("invalid SRT end timestamp: {}", &cap[2]))?;
        if start > end {
            return Err(format!("SRT start exceeds end at cue {seq}"));
        }
        if nonempty > 0 && start < last_start {
            return Err(format!("SRT cue order goes backwards at cue {seq}"));
        }
        let text = lines[2..].join("\n");
        if text.trim().is_empty() {
            last_seq = seq;
            last_start = start;
            continue;
        }
        let speaker_cap = speaker
            .captures(&text)
            .ok_or_else(|| format!("non-empty cue {seq} has no speaker label"))?;
        let label = speaker_cap[1].trim().to_string();
        let canonical = canonical_builtin(&label)
            .or_else(|| lookup.canonical.get(&label).cloned())
            .ok_or_else(|| format!("speaker label '{label}' is not recognized or mapped"))?;
        source_labels.insert(label);
        canonical_labels.insert(canonical);
        nonempty += 1;
        last_seq = seq;
        last_start = start;
    }
    if nonempty == 0 {
        return Err("SRT contains no non-empty cues".into());
    }
    Ok(SrtValidation {
        source_labels,
        canonical_labels,
        cue_count: nonempty,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_bom_segment_and_plain_speaker_labels() {
        let bytes = b"\xef\xbb\xbf1\r\n00:00:00,010 --> 00:00:01,020\r\n[00_spk_01] hi\r\n\r\n2\r\n00:00:01,020 --> 00:00:02,000\r\n[spk_02] there\r\n";
        let parsed = validate_srt(bytes, &SpeakerLookup::default()).unwrap();
        assert_eq!(parsed.cue_count, 2);
        assert!(parsed.canonical_labels.contains("spk_01"));
        assert!(parsed.canonical_labels.contains("spk_02"));
    }

    #[test]
    fn rejects_missing_speaker_and_bad_time_order() {
        let missing = b"1\n00:00:00,000 --> 00:00:01,000\nhello\n";
        assert!(validate_srt(missing, &SpeakerLookup::default())
            .unwrap_err()
            .contains("no speaker"));
        let backwards = b"1\n00:00:02,000 --> 00:00:01,000\n[spk_01] hi\n";
        assert!(validate_srt(backwards, &SpeakerLookup::default())
            .unwrap_err()
            .contains("exceeds"));
    }

    #[test]
    fn accepts_only_complete_explicit_historical_mapping() {
        let bytes = b"1\n00:00:00,000 --> 00:00:01,000\n[Alice?] hi\n";
        let mut lookup = SpeakerLookup::default();
        lookup.canonical.insert("Alice?".into(), "spk_01".into());
        assert!(validate_srt(bytes, &lookup).is_ok());
        assert!(validate_srt(bytes, &SpeakerLookup::default()).is_err());
    }

    #[test]
    fn validates_generated_content_header_and_all_body_segments() {
        let bytes = b"# Weekly\n---\n2026-04-03 17:33\nSummary: x\n---\n\n00:00:00  Alice: hello\n00:00:03  Bob: hi\n";
        let parsed = validate_content_markdown(bytes).unwrap();
        assert_eq!(parsed.segment_count, 2);
        assert_eq!(parsed.speakers, vec!["Alice", "Bob"]);
        assert!(validate_content_markdown(b"00:00:00  Alice: ok\nplain text\n").is_err());
    }
}
