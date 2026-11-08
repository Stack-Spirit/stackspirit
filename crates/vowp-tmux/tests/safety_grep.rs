//! Safety grep: the crate source must never build a respawn or keystroke
//! command. Comments may mention the ban; code may not contain it.
//!
//! Spec: `docs/design/crates/vowp-tmux.md` §3 rule 1 and §7.

use std::fs;
use std::path::{Path, PathBuf};

const BANNED: [&str; 5] = [
    "respawn-pane",
    "respawn-window",
    "send-keys",
    "send_keys",
    "respawn_pane",
];

#[test]
fn source_has_no_respawn_or_send_keys() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_files(&root, &mut files);
    files.sort();
    assert!(!files.is_empty(), "no source files found under {root:?}");

    for path in files {
        let text = fs::read_to_string(&path).expect("read source file");
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            for banned in BANNED {
                assert!(
                    !trimmed.contains(banned),
                    "{}:{} contains banned capability {banned:?}",
                    path.display(),
                    index + 1
                );
            }
        }
    }
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).expect("read source directory");
    for entry in entries {
        let path = entry.expect("read directory entry").path();
        if path.is_dir() {
            collect_files(&path, files);
        } else {
            files.push(path);
        }
    }
}
