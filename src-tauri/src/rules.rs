//! 规则文件的加载与首次拷贝（spec §7.2）。
//!
//! IO 全部在这一层；engine 只见到 `RuleSet` 这个值。
//!

use activity_engine::{RuleSet, DEFAULT_RULES_TOML};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RulesSource {
    /// 首次启动，已从内置默认写盘
    Created,
    /// 读到了用户自己的文件
    Loaded,
    /// 文件坏了，内存里退回内置默认（磁盘上不动）
    FellBackToDefault,
}

pub fn app_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA").expect("APPDATA env var");
    PathBuf::from(appdata).join("time-scope")
}

pub fn rules_path() -> PathBuf {
    app_dir().join("rules.toml")
}

fn default_rule_set() -> RuleSet {
    RuleSet::from_toml(DEFAULT_RULES_TOML).expect("内置默认规则必须始终可解析")
}

/// 加载规则集。**任何失败都不 panic**，一律退回内置默认。
pub fn load_rules(path: &Path) -> (RuleSet, RulesSource) {
    match std::fs::read_to_string(path) {
        Ok(text) => match RuleSet::from_toml(&text) {
            Ok(rs) => (rs, RulesSource::Loaded),
            Err(e) => {
                eprintln!(
                    "[time-scope] {} 解析失败（{e}），本次使用内置默认规则；\
                     修好后重启生效",
                    path.display()
                );
                (default_rule_set(), RulesSource::FellBackToDefault)
            }
        },
        Err(_) => {
            // 首次启动：把内置默认写出去，用户可以改
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if std::fs::write(path, DEFAULT_RULES_TOML).is_ok() {
                (default_rule_set(), RulesSource::Created)
            } else {
                (default_rule_set(), RulesSource::FellBackToDefault)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ts-rules-{}-{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d.join("rules.toml")
    }

    #[test]
    fn creates_default_rules_file_on_first_run() {
        let p = tmp("first");
        let (rs, src) = load_rules(&p);
        assert_eq!(src, RulesSource::Created);
        assert!(p.exists(), "首次启动应写出规则文件");
        assert!(!rs.rules.is_empty());
        assert_eq!(rs.classify(Some("Code.exe")).category.as_str(), "work");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn second_run_reads_the_user_file() {
        let p = tmp("second");
        let _ = load_rules(&p);
        fs::write(
            &p,
            "[[rule]]\nid=\"x\"\nprocess=[\"a.exe\"]\ncategory=\"life\"\nconfidence=0.5\n",
        )
        .unwrap();
        let (rs, src) = load_rules(&p);
        assert_eq!(src, RulesSource::Loaded);
        assert_eq!(rs.classify(Some("a.exe")).category.as_str(), "life");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn broken_user_file_falls_back_to_defaults_without_crashing() {
        // Review Focus #5：用户把 rules.toml 改坏了应用不能崩
        let p = tmp("broken");
        fs::write(&p, "[[[ this is not toml").unwrap();
        let (rs, src) = load_rules(&p);
        assert_eq!(src, RulesSource::FellBackToDefault);
        assert!(!rs.rules.is_empty());
        assert_eq!(rs.classify(Some("Code.exe")).category.as_str(), "work");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn semantically_invalid_file_falls_back_too() {
        let p = tmp("badcat");
        fs::write(
            &p,
            "[[rule]]\nid=\"x\"\nprocess=[\"a.exe\"]\ncategory=\"nonsense\"\nconfidence=0.5\n",
        )
        .unwrap();
        let (_, src) = load_rules(&p);
        assert_eq!(src, RulesSource::FellBackToDefault);
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn broken_file_is_not_overwritten() {
        // 退回默认只是内存行为，磁盘上用户的文件要留着给他自己修
        let p = tmp("keep");
        let original = "[[[ broken";
        fs::write(&p, original).unwrap();
        let _ = load_rules(&p);
        assert_eq!(fs::read_to_string(&p).unwrap(), original);
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn empty_user_file_is_valid_but_classifies_nothing() {
        // 空文件能解析，所以不是"坏了"。用户清空规则后所有程序落到 unknown——
        // 这不该算错误，用户可能就想先清空试试。
        let p = tmp("empty");
        fs::write(&p, "").unwrap();
        let (rs, src) = load_rules(&p);
        assert_eq!(src, RulesSource::Loaded);
        assert!(rs.rules.is_empty());
        assert_eq!(rs.classify(Some("Code.exe")).category.as_str(), "unknown");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn rules_path_sits_next_to_the_database() {
        let p = rules_path();
        assert_eq!(p.file_name().unwrap(), "rules.toml");
        assert_eq!(p.parent().unwrap(), app_dir().as_path());
    }
}
