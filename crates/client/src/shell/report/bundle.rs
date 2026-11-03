//! The report file: a `.zip` (GitHub accepts it as an attachment) with the facts and what a
//! developer needs to replay the game: the current save, and on desktop the autosave and the
//! command journal after it (`desktop --replay autosave.aurlog`, platform/docs/saves.md 3).
//! The zip is written here, by hand: stored headers and a deflate stream of `flate2`, no zip crate.

use std::io::Write;

use flate2::write::DeflateEncoder;
use flate2::{Compression, Crc};

/// The report file stays under this many bytes.
pub const FILE_CAP: usize = 5 * 1024 * 1024;

/// Entry names inside the zip (the replay finds the save beside the journal by the stem).
pub const FACTS: &str = "report.txt";
pub const CURRENT_SAVE: &str = "game.aurelia";
pub const AUTOSAVE: &str = "autosave.aurelia";
pub const JOURNAL: &str = "autosave.aurlog";

/// What the game can give a report; any part may be missing.
#[derive(Debug, Clone, Default)]
pub struct Parts {
    /// The match as it is now, tagged as a save file is.
    pub current_save: Option<Vec<u8>>,
    /// The autosave the journal starts from, and the journal (commands since that autosave).
    pub autosave: Option<Vec<u8>>,
    pub journal: Option<Vec<u8>>,
}

/// The finished report file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    pub bytes: Vec<u8>,
    /// The entries inside, in order.
    pub entries: Vec<String>,
    /// Parts left out because the file would pass `FILE_CAP`.
    pub left_out: Vec<&'static str>,
}

/// One entry, compressed.
struct Packed {
    name: String,
    crc: u32,
    size: u32,
    deflated: Vec<u8>,
}

fn pack(name: &str, bytes: &[u8]) -> Packed {
    let mut crc = Crc::new();
    crc.update(bytes);
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("a Vec takes any write");
    Packed { name: name.to_owned(), crc: crc.sum(), size: bytes.len() as u32, deflated: encoder.finish().expect("a Vec takes any write") }
}

/// What one entry adds to the file: its two headers, its name twice and its data.
fn cost(packed: &Packed) -> usize {
    30 + 46 + 2 * packed.name.len() + packed.deflated.len()
}

/// The report file of `header` (the facts) and `parts`, made at `unix_seconds` (the entries' time).
/// Parts go in by importance, the current save first, the autosave and its journal as a pair; one
/// that would take the file past `FILE_CAP` is left out and named in the facts.
pub fn build(header: &str, parts: &Parts, unix_seconds: u64) -> Built {
    let mut budget = FILE_CAP.saturating_sub(22 + 4096);
    let mut packed: Vec<Packed> = Vec::new();
    let mut left_out = Vec::new();
    let mut take = |candidates: Vec<Packed>, label: &'static str, left_out: &mut Vec<&'static str>, packed: &mut Vec<Packed>| {
        let total: usize = candidates.iter().map(cost).sum();
        if total <= budget {
            budget -= total;
            packed.extend(candidates);
        } else {
            left_out.push(label);
        }
    };
    if let Some(save) = &parts.current_save {
        take(vec![pack(CURRENT_SAVE, save)], CURRENT_SAVE, &mut left_out, &mut packed);
    }
    if let (Some(save), Some(journal)) = (&parts.autosave, &parts.journal) {
        take(vec![pack(AUTOSAVE, save), pack(JOURNAL, journal)], JOURNAL, &mut left_out, &mut packed);
    }
    let mut text = header.trim_end().to_owned();
    text.push_str("\n\nFiles in this report\n"); // i18n-ok: the file is for developers, in English
    for entry in &packed {
        text.push_str(&format!("- {} ({} bytes)\n", entry.name, entry.size)); // i18n-ok: the file is for developers, in English
    }
    for label in &left_out {
        text.push_str(&format!("- {label}: left out, the report file would pass 5 MB\n")); // i18n-ok: the file is for developers, in English
    }
    if packed.iter().any(|entry| entry.name == JOURNAL) {
        text.push_str("\nReplay: unzip, then `aurelia-classic --replay autosave.aurlog` (the save is taken from beside the journal).\n"); // i18n-ok: the file is for developers, in English
    }
    let mut all = vec![pack(FACTS, text.as_bytes())];
    all.extend(packed);
    Built { entries: all.iter().map(|entry| entry.name.clone()).collect(), bytes: zip(&all, unix_seconds), left_out }
}

/// The zip file of `entries` (deflate, no extra fields, names in UTF-8).
fn zip(entries: &[Packed], unix_seconds: u64) -> Vec<u8> {
    let (time, date) = dos_time(unix_seconds);
    let mut out = Vec::new();
    let mut offsets = Vec::new();
    for entry in entries {
        offsets.push(out.len() as u32);
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0x0800u16.to_le_bytes()); // flags: UTF-8 names
        out.extend_from_slice(&8u16.to_le_bytes()); // deflate
        out.extend_from_slice(&time.to_le_bytes());
        out.extend_from_slice(&date.to_le_bytes());
        out.extend_from_slice(&entry.crc.to_le_bytes());
        out.extend_from_slice(&(entry.deflated.len() as u32).to_le_bytes());
        out.extend_from_slice(&entry.size.to_le_bytes());
        out.extend_from_slice(&(entry.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra
        out.extend_from_slice(entry.name.as_bytes());
        out.extend_from_slice(&entry.deflated);
    }
    let directory = out.len() as u32;
    for (entry, offset) in entries.iter().zip(&offsets) {
        out.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // made by
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0x0800u16.to_le_bytes());
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&time.to_le_bytes());
        out.extend_from_slice(&date.to_le_bytes());
        out.extend_from_slice(&entry.crc.to_le_bytes());
        out.extend_from_slice(&(entry.deflated.len() as u32).to_le_bytes());
        out.extend_from_slice(&entry.size.to_le_bytes());
        out.extend_from_slice(&(entry.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0; 2 + 2 + 2 + 2 + 4]); // extra, comment, disk, internal and external attributes
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(entry.name.as_bytes());
    }
    let size = out.len() as u32 - directory;
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]); // this disk, the directory's disk
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&directory.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment
    out
}

/// A time as the zip format keeps it (MS-DOS time and date, UTC, two-second steps).
fn dos_time(unix_seconds: u64) -> (u16, u16) {
    let (year, month, day, hour, minute, second) = civil(unix_seconds);
    let year = year.clamp(1980, 2107) - 1980;
    ((hour << 11 | minute << 5 | second / 2) as u16, (year << 9 | month << 5 | day) as u16)
}

/// (year, month, day, hour, minute, second) in UTC of a Unix time.
pub fn civil(unix_seconds: u64) -> (u64, u64, u64, u64, u64, u64) {
    let days = unix_seconds / 86_400;
    let rest = unix_seconds % 86_400;
    // Days since 1970-01-01 to a calendar date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day, rest / 3600, rest % 3600 / 60, rest % 60)
}

/// `aurelia-report-20261103-161205.zip`
pub fn file_name(unix_seconds: u64) -> String {
    let (year, month, day, hour, minute, second) = civil(unix_seconds);
    format!("aurelia-report-{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}.zip")
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::DeflateDecoder;
    use std::io::Read;

    /// Reads a zip as an extractor does: the central directory, then each entry, CRC checked.
    pub(crate) fn unzip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let u16_at = |at: usize| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) as usize;
        let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let end = bytes.len() - 22;
        assert_eq!(u32_at(end), 0x0605_4b50, "end of central directory");
        let count = u16_at(end + 10);
        let mut at = u32_at(end + 16) as usize;
        let mut out = Vec::new();
        for _ in 0..count {
            assert_eq!(u32_at(at), 0x0201_4b50);
            let (crc, packed, size, name_len) = (u32_at(at + 16), u32_at(at + 20) as usize, u32_at(at + 24) as usize, u16_at(at + 28));
            let local = u32_at(at + 42) as usize;
            let name = String::from_utf8(bytes[at + 46..at + 46 + name_len].to_vec()).unwrap();
            at += 46 + name_len;
            assert_eq!(u32_at(local), 0x0403_4b50);
            let data = local + 30 + u16_at(local + 26) + u16_at(local + 28);
            let mut plain = Vec::new();
            DeflateDecoder::new(&bytes[data..data + packed]).read_to_end(&mut plain).unwrap();
            let mut check = Crc::new();
            check.update(&plain);
            assert_eq!((check.sum(), plain.len()), (crc, size), "{name}");
            out.push((name, plain));
        }
        out
    }

    #[test]
    fn the_file_is_a_zip_with_the_facts_the_save_and_the_journal() {
        let parts = Parts { current_save: Some(vec![7; 5000]), autosave: Some(b"autosave".to_vec()), journal: Some(b"journal".to_vec()) };
        let built = build("Aurelia bug report\n\n- Game: Aurelia 0.2.0", &parts, 1_793_000_000);
        assert_eq!(built.entries, vec![FACTS, CURRENT_SAVE, AUTOSAVE, JOURNAL]);
        let files = unzip(&built.bytes);
        assert_eq!(files.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), vec![FACTS, CURRENT_SAVE, AUTOSAVE, JOURNAL]);
        assert_eq!(files[1].1, vec![7; 5000]);
        assert_eq!(files[3].1, b"journal");
        let facts = String::from_utf8(files[0].1.clone()).unwrap();
        assert!(facts.starts_with("Aurelia bug report") && facts.contains("autosave.aurlog (7 bytes)") && facts.contains("--replay"), "{facts}");
    }

    #[test]
    fn a_part_that_would_pass_five_megabytes_is_left_out_and_said_so() {
        // Noise does not compress: a journal of 6 MB cannot fit.
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let noise: Vec<u8> = (0..6 * 1024 * 1024)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect();
        let parts = Parts { current_save: Some(b"save".to_vec()), autosave: Some(b"autosave".to_vec()), journal: Some(noise) };
        let built = build("head", &parts, 0);
        assert!(built.bytes.len() <= FILE_CAP);
        assert_eq!(built.entries, vec![FACTS, CURRENT_SAVE]);
        assert_eq!(built.left_out, vec![JOURNAL]);
        let facts = String::from_utf8(unzip(&built.bytes)[0].1.clone()).unwrap();
        assert!(facts.contains("autosave.aurlog: left out"), "{facts}");
    }

    #[test]
    fn a_report_without_a_match_is_the_facts_alone() {
        let built = build("head", &Parts::default(), 86_400);
        assert_eq!(built.entries, vec![FACTS]);
        assert_eq!(unzip(&built.bytes).len(), 1);
    }

    #[test]
    fn times_and_names_are_utc_dates() {
        assert_eq!(civil(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(civil(1_709_164_800), (2024, 2, 29, 0, 0, 0), "a leap day");
        assert_eq!(file_name(1_793_000_000), "aurelia-report-20261026-073320.zip");
        let (time, date) = dos_time(1_793_000_000);
        assert_eq!((date >> 9, date >> 5 & 15, date & 31), (2026 - 1980, 10, 26));
        assert_eq!((time >> 11, time >> 5 & 63, (time & 31) * 2), (7, 33, 20));
    }
}
