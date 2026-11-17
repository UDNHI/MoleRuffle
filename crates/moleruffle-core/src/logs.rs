//! 日志清理:启动器每次开游戏都在资料夹(`MOLERUFFLE_DATA_DIR`)新建一个日志档,
//! 不清的话会无限累积。启动时每种日志只保留最新的 [`KEEP_PER_KIND`] 个。
//!
//! 日志由各启动方式写入,档名以时间戳开头,所以按档名排序就是按时间排序:
//! - `game-YYYYMMDD-HHMMSS.log`:打包版启动器(`launcher.m`)
//! - `game-YYYYMMDD-HHMMSS-local.log`:`scripts/run-local.sh`
//! - `test-YYYYMMDD-HHMMSS.log`:打包版 `launch.sh`
//!
//! `launcher.log` 等其他档案不动。

use std::path::Path;

/// 每种日志保留的数量。
pub const KEEP_PER_KIND: usize = 20;

const KINDS: &[&str] = &["game-", "test-"];

/// 资料夹由 `MOLERUFFLE_DATA_DIR` 指定时(即由启动器启动),清理其中的旧日志。
pub fn prune_old_logs() {
    if let Some(dir) = std::env::var_os("MOLERUFFLE_DATA_DIR") {
        let removed = prune(Path::new(&dir), KEEP_PER_KIND);
        if removed > 0 {
            tracing::info!("已删除 {removed} 个旧日志,每种保留最新 {KEEP_PER_KIND} 个");
        }
    }
}

/// 每种日志只留最新的 `keep` 个,回传删除的数量。读不到资料夹或删不掉就略过。
fn prune(dir: &Path, keep: usize) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let names: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    let mut removed = 0;
    for kind in KINDS {
        let mut logs: Vec<&String> = names
            .iter()
            .filter(|name| is_log_of_kind(name, kind))
            .collect();
        logs.sort_unstable_by(|a, b| b.cmp(a)); // 新的在前
        for name in logs.into_iter().skip(keep) {
            if std::fs::remove_file(dir.join(name)).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

/// `game-20261116-202348.log` 这类:前缀 + 8 位日期 + `-` + 6 位时间,`.log` 结尾。
fn is_log_of_kind(name: &str, kind: &str) -> bool {
    let Some(rest) = name.strip_prefix(kind) else {
        return false;
    };
    let bytes = rest.as_bytes();
    name.ends_with(".log")
        && bytes.len() >= 15
        && bytes[..8].iter().all(u8::is_ascii_digit)
        && bytes[8] == b'-'
        && bytes[9..15].iter().all(u8::is_ascii_digit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_newest_logs_of_each_kind_only() {
        let dir = std::env::temp_dir().join(format!("moleruffle-logs-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let files = [
            "game-20261101-000000.log",
            "game-20261102-000000-local.log",
            "game-20261103-000000.log",
            "test-20261101-000000.log",
            "test-20261102-000000.log",
            "launcher.log",
            "game-notes.log",
            "bank-patched.swf",
        ];
        for name in files {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        std::fs::create_dir(dir.join("game-20261001-000000.log")).unwrap(); // 资料夹不动

        assert_eq!(prune(&dir, 1), 3);

        let mut left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "bank-patched.swf",
                "game-20261001-000000.log",
                "game-20261103-000000.log",
                "game-notes.log",
                "launcher.log",
                "test-20261102-000000.log",
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_directory_is_ignored() {
        assert_eq!(prune(Path::new("/nonexistent/moleruffle-logs"), 1), 0);
    }
}
