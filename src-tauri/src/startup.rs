use crate::db::Store;
use std::path::{Path, PathBuf};

/// `<path><suffix>` (appends to the full file name, e.g. `x.sqlite-wal`).
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Move a bad DB (and `-wal`/`-shm` siblings) aside. Never deletes.
/// Returns the new main-file path, or an error string.
fn quarantine(path: &Path) -> Result<PathBuf, String> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let broken = with_suffix(path, &format!(".broken-{stamp}"));
    std::fs::rename(path, &broken).map_err(|e| e.to_string())?;
    for sfx in ["-wal", "-shm"] {
        let side = with_suffix(path, sfx);
        if side.exists() {
            // Best effort: a stale sibling must not attach to the fresh DB.
            let _ = std::fs::rename(&side, with_suffix(&broken, sfx));
        }
    }
    Ok(broken)
}

/// Open the DB at `path`. On failure, rename the bad file aside and create a
/// fresh DB; if that also fails, fall back to in-memory. Warning is `Some`
/// whenever recovery happened.
pub fn open_store_with_recovery(path: &Path) -> (Store, Option<String>) {
    let first_err = match Store::open(path) {
        Ok(store) => return (store, None),
        Err(e) => e,
    };
    eprintln!("db open failed ({first_err}); attempting recovery");

    let moved = if path.is_file() {
        Some(quarantine(path))
    } else {
        None
    };

    if !matches!(moved, Some(Err(_))) {
        if let Ok(store) = Store::open(path) {
            let msg = match moved {
                Some(Ok(broken)) => format!(
                    "The database could not be opened ({first_err}). It was renamed to {} and a new \
                     one was created. Sessions are rebuilt from existing logs; history of logs that \
                     were already deleted remains only in the renamed file.",
                    broken.display()
                ),
                _ => format!(
                    "The database could not be opened ({first_err}). A new one was created at {}.",
                    path.display()
                ),
            };
            return (store, Some(msg));
        }
    }

    let detail = match moved {
        Some(Ok(broken)) => format!("The old file was renamed to {}.", broken.display()),
        Some(Err(e)) => format!("The old file could not be renamed ({e})."),
        None => format!("{} is not a regular file.", path.display()),
    };
    let store = Store::open_in_memory().expect("in-memory sqlite");
    (
        store,
        Some(format!(
            "The database could not be opened ({first_err}) and a new one could not be created. \
             Persistence is off: data is kept in memory only and lost on exit. {detail}"
        )),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_db_no_warning() {
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join("t.sqlite");
        drop(Store::open(&p).unwrap());
        let (_s, w) = open_store_with_recovery(&p);
        assert!(w.is_none());
    }

    #[test]
    fn garbage_file_renamed_and_fresh_db() {
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join("t.sqlite");
        std::fs::write(
            &p,
            b"this is definitely not a sqlite database, just garbage bytes!!",
        )
        .unwrap();
        let (s, w) = open_store_with_recovery(&p);
        let w = w.expect("warning");
        let broken: Vec<_> = std::fs::read_dir(td.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|f| {
                f.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("t.sqlite.broken-")
            })
            .collect();
        let main: Vec<_> = broken
            .iter()
            .filter(|f| !f.to_string_lossy().ends_with("-wal"))
            .collect();
        assert_eq!(main.len(), 1);
        assert!(w.contains(&main[0].display().to_string()));
        assert_eq!(std::fs::read(main[0]).unwrap()[..4], *b"this");
        assert!(p.exists());
        assert!(s.load_all().unwrap().is_empty());
    }

    #[test]
    fn quarantine_moves_siblings() {
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join("t.sqlite");
        for sfx in ["", "-wal", "-shm"] {
            std::fs::write(with_suffix(&p, sfx), sfx).unwrap();
        }
        let broken = quarantine(&p).unwrap();
        assert!(!p.exists());
        for sfx in ["", "-wal", "-shm"] {
            assert!(!with_suffix(&p, sfx).exists());
            assert_eq!(
                std::fs::read_to_string(with_suffix(&broken, sfx)).unwrap(),
                sfx
            );
        }
    }

    #[test]
    fn unopenable_dir_falls_back_to_memory() {
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join("isdir");
        std::fs::create_dir(&p).unwrap();
        let (s, w) = open_store_with_recovery(&p);
        assert!(w.expect("warning").contains("Persistence is off"));
        assert!(p.is_dir());
        assert!(s.load_all().unwrap().is_empty());
    }
}
