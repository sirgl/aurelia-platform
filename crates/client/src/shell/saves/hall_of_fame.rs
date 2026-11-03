//! The Hall of Fame: one text line per finished game (nation, result, turn,
//! score), appended to the module's `hall_of_fame.txt` (`ModuleSaves`).
use std::path::PathBuf;

use crate::shell::storage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HallOfFame {
    path: PathBuf,
}

impl HallOfFame {
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// The lines of earlier games, oldest first (none when the file is missing).
    pub fn lines(&self) -> Vec<String> {
        let Ok(text) = storage::read_to_string(&self.path) else {
            return Vec::new();
        };
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_owned)
            .collect()
    }

    pub fn append(&self, line: &str) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            storage::create_dir_all(dir).map_err(|e| i18n::tf!("shell.errors.cannot-create", path = format!("{dir:?}"), error = e.to_string()))?;
        }
        let mut file = storage::open_append(&self.path, true).map_err(|e| i18n::tf!("shell.errors.hall-open", error = e.to_string()))?;
        file.write_all(format!("{line}\n").as_bytes()).map_err(|e| i18n::tf!("shell.errors.hall-write", error = e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appended_lines_read_back_in_order() {
        let dir = std::env::temp_dir().join(format!("aurelia-fame-{}", std::process::id()));
        let hall = HallOfFame::at(dir.join("hall_of_fame.txt"));
        assert!(hall.lines().is_empty());
        hall.append("Rome · Science victory · turn 300 · score 900")
            .unwrap();
        hall.append("Rome · Eliminated · turn 80 · score 40")
            .unwrap();
        assert_eq!(
            hall.lines(),
            vec![
                "Rome · Science victory · turn 300 · score 900",
                "Rome · Eliminated · turn 80 · score 40"
            ]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
