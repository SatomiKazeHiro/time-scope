//! 规则文件的加载与首次拷贝（spec §7.2）。
//!
//! IO 全部在这一层；engine 只见到 `RuleSet` 这个值。
//!

use activity_engine::{Category, RuleSet, DEFAULT_RULES_TOML};
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

/// `rules.toml` 的路径 —— 与库、`config.toml` 同目录。
///
/// 目录本身由 `config::data_dir()` 一处决定（这里曾经重复实现过一遍 `app_dir()`，
/// 改位置得记着改两处）。
pub fn rules_path() -> PathBuf {
    crate::config::data_dir().join("rules.toml")
}

/// 从 raw `rules.toml` 文本里找「已经给这个进程名配过类别」的那条，返回它的 category。
///
/// 比较大小写不敏感 —— 与 `Rule::matches` 的口径一致。两者不一致的话，
/// 用户写 `dbeaver.exe` 就能绕过重复检测，造出两条同名规则。
///
/// 解析不了返回 `None`：那种文件根本走不到这里（调用方先做了语法检查），
/// 这里的 `None` 只是防御，不承担语义。
fn find_rule_for(text: &str, process: &str) -> Option<String> {
    let doc: toml::Table = text.parse().ok()?;
    let target = process.to_ascii_lowercase();
    let rules = doc.get("rule")?.as_array()?;
    rules.iter().find_map(|r| {
        let r = r.as_table()?;
        let hit = r
            .get("process")?
            .as_array()?
            .iter()
            .any(|p| p.as_str().map(|s| s.to_ascii_lowercase() == target) == Some(true));
        hit.then(|| {
            r.get("category")
                .and_then(|c| c.as_str())
                .unwrap_or("unknown")
                .to_string()
        })
    })
}

/// 往 `rules.toml` 追加一条规则，并把新的 `RuleSet` 返回给调用方热替换。
///
/// 只追加，不整份重写 —— 与 `config::patch_line` 同款理由：
/// 文件里可能有用户自己写的注释和空行，整读再整写会全部抹掉。
///
/// 坏文件绝不覆盖：解析不了就返回 `Err`，磁盘上一个字节都不动。
/// 这与 `load_rules` 的既定行为一致（坏文件只读不写，留给用户自己修）。
/// 用户在这里看到的是「请先手工修好 rules.toml」，而不是应用默默
/// 把他写的规则换成内置默认 —— 那会毁掉他手写的东西。
///
/// 追加后立刻重新解析一遍再返回：写进去了不等于写对了。
/// 万一手滑写出一份解析不了的（新进程名里带引号之类），
/// 宁可报错也不能让下次启动炸掉。
pub fn append_rule(
    path: &Path,
    process: &str,
    category: &str,
    confidence: f32,
) -> Result<RuleSet, String> {
    // 进程名先归一：规则匹配只比小写文件名（见 Rule::matches），
    // 这里存原样即可，但空串与路径要挡掉 —— 它们匹配不到任何东西，
    // 只会让用户以为自己配好了。
    let proc_name = process.trim();
    if proc_name.is_empty() {
        return Err("进程名不能为空".to_string());
    }
    if proc_name.contains('/') || proc_name.contains('\\') {
        return Err(format!(
            "只填程序名，不要填路径：`{proc_name}`（规则只比文件名部分）"
        ));
    }
    // 类别必须在白名单里，且不能是 idle / unknown ——
    // 那两个不是「程序属于哪一类」，是「没在用」和「没认出来」，
    // 写进规则只会造出永远命中不了的死规则。
    let cat = Category::from_str(category)
        .ok_or_else(|| format!("未知类别 `{category}`"))?;
    if matches!(cat, Category::Idle | Category::Unknown) {
        return Err("idle / unknown 不是类别，不能作为规则的目标".to_string());
    }

    let existing = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            DEFAULT_RULES_TOML.to_string()
        }
        Err(e) => return Err(format!("读不到 rules.toml：{e}")),
    };
    if existing.parse::<toml::Table>().is_err() {
        return Err(
            "rules.toml 已有语法错误，没有改动它。请先手工修好再试（本次选择未保存）"
                .to_string(),
        );
    }

    // id 用进程名派生，保证重复添加时后写的会覆盖前一条的语义，
    // 而不是同一个进程名在文件里出现两次、匹配结果取决于顺序。
    let id = proc_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(32)
        .collect::<String>()
        .to_ascii_lowercase();
    let id = if id.is_empty() {
        "custom".to_string()
    } else {
        id
    };

    let mut text = existing;

    // 已有同名规则就拒绝，不追加第二条。
    //
    // 引擎按文件顺序取第一个命中的，两条同名规则并存时用户改的那条
    // 永远不生效 —— 而界面会告诉他「已保存」。那种静默失败比报错难查得多。
    // 改已有规则请手工编辑 rules.toml（追加式写法无法在不重排全文、
    // 不丢注释的前提下替换一条，中间那条块属于谁是有歧义的）。
    //
    // 走原始 TOML 而非 `RuleSet`：`Rule.process_lower` 是 engine 的私有字段
    // （engine 不依赖 IO 也不该为此暴露内部），app 层读同一份文件的 raw 表即可。
    if let Some(existing_cat) = find_rule_for(&text, proc_name) {
        return Err(format!(
            "`{proc_name}` 已有一条规则（category = {existing_cat}）。\
             追加第二条不会覆盖它 —— 请手工编辑 {} 修改。",
            path.display()
        ));
    }

    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!(
        "\n# 从设置页添加\n[[rule]]\nid = \"{id}\"\nprocess = [\"{proc_name}\"]\ncategory = \"{}\"\nconfidence = {confidence}\n",
        cat.as_str()
    ));

    let new_set = RuleSet::from_toml(&text)
        .map_err(|e| format!("新规则写进去后解析失败（{e}），已放弃写入：{proc_name}"))?;

    // 先解析成功、再落盘：写文件是唯一不可撤销的一步
    std::fs::write(path, &text).map_err(|e| format!("写 rules.toml 失败：{e}"))?;
    Ok(new_set)
}

fn default_rule_set() -> RuleSet {
    RuleSet::from_toml(DEFAULT_RULES_TOML).expect("内置默认规则必须始终可解析")
}

/// 加载规则集。任何失败都不 panic，一律退回内置默认。
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
        assert_eq!(p.parent().unwrap(), crate::config::data_dir().as_path());
    }

    // ---- append_rule：设置页「加规则」的写回 ----

    #[test]
    fn appending_a_rule_makes_it_take_effect_immediately() {
        let p = tmp("append");
        let _ = load_rules(&p);
        let rs = append_rule(&p, "DBeaver.exe", "work", 0.9).unwrap();

        // 返回的 RuleSet 已经含新规则 —— 引擎靠它热替换，不必重启
        assert_eq!(rs.classify(Some("DBeaver.exe")).category.as_str(), "work");
        // 落盘的也要一致，否则重启后行为变了
        let text = fs::read_to_string(&p).unwrap();
        assert!(text.contains("DBeaver.exe"));
        assert_eq!(RuleSet::from_toml(&text).unwrap().rules.len(), rs.rules.len());
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn appending_keeps_the_users_comments_and_existing_rules() {
        let p = tmp("append-keep");
        let _ = load_rules(&p);
        let before = fs::read_to_string(&p).unwrap();
        let before_count = RuleSet::from_toml(&before).unwrap().rules.len();

        append_rule(&p, "Zed.exe", "study", 0.9).unwrap();
        let after = fs::read_to_string(&p).unwrap();

        // 整读再整写会把这些全部抹掉 —— 那是 append 而不是 patch 的理由
        assert!(after.starts_with(&before), "原有内容（含注释）必须原样保留");
        assert_eq!(
            RuleSet::from_toml(&after).unwrap().rules.len(),
            before_count + 1
        );
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn a_broken_file_is_refused_not_overwritten() {
        // 与 load_rules 的既定行为对齐：坏文件只读不写。
        // 用户在这里必须看到错误，而不是发现自己的规则被换成内置默认。
        let p = tmp("append-broken");
        let original = "[[[ broken";
        fs::write(&p, original).unwrap();

        let err = append_rule(&p, "DBeaver.exe", "work", 0.9).unwrap_err();
        assert!(err.contains("语法错误"), "错误要说清原因：{err}");
        assert_eq!(fs::read_to_string(&p).unwrap(), original);
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn a_rejected_rule_leaves_the_file_untouched() {
        // 校验失败必须发生在写盘之前 —— 否则会留下一条半截的规则
        let p = tmp("append-reject");
        let _ = load_rules(&p);
        let before = fs::read_to_string(&p).unwrap();

        for (proc_name, category, why) in [
            ("", "work", "空进程名"),
            ("   ", "work", "纯空白"),
            ("C:\\bin\\foo.exe", "work", "带路径"),
            ("foo.exe", "nonsense", "未知类别"),
            ("foo.exe", "idle", "idle 不是类别"),
            ("foo.exe", "unknown", "unknown 不是类别"),
        ] {
            let err = append_rule(&p, proc_name, category, 0.9).unwrap_err();
            assert!(!err.is_empty(), "{why} 应该给出原因");
            assert_eq!(
                fs::read_to_string(&p).unwrap(),
                before,
                "{why} 不该动文件"
            );
        }
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn adding_a_process_twice_is_refused_rather_than_silently_shadowed() {
        // 引擎按文件顺序取第一个命中的。追加第二条同名规则时，
        // 用户刚选的那个类别永远不生效 —— 而界面会说「已保存」。
        // 那种静默失败比报错难查得多，所以明确拒绝并说清去哪改。
        let p = tmp("append-twice");
        let _ = load_rules(&p);
        append_rule(&p, "DBeaver.exe", "work", 0.9).unwrap();
        let before = fs::read_to_string(&p).unwrap();

        let err = append_rule(&p, "DBeaver.exe", "study", 0.9).unwrap_err();
        assert!(err.contains("已有一条规则"), "错误要说清原因：{err}");
        assert!(err.contains("work"), "该告诉用户现有类别是什么：{err}");
        assert_eq!(fs::read_to_string(&p).unwrap(), before, "拒绝时不许动文件");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn the_existing_category_is_detected_case_insensitively() {
        // 匹配本身就大小写不敏感（见 Rule::matches），重复检测必须一致 ——
        // 否则用户写 `dbeaver.exe` 会绕过检查，造出两条规则。
        let p = tmp("append-case");
        let _ = load_rules(&p);
        append_rule(&p, "DBeaver.exe", "work", 0.9).unwrap();
        let before = fs::read_to_string(&p).unwrap();

        assert!(append_rule(&p, "DBEAVER.EXE", "study", 0.9).is_err());
        assert_eq!(fs::read_to_string(&p).unwrap(), before);
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn a_different_process_with_the_same_id_prefix_is_still_added() {
        // id 是进程名的字母数字前缀，两个不同进程可能撞上 ——
        // 那是 id 撞了，不是同一进程，追加是正确行为。
        let p = tmp("append-prefix");
        let _ = load_rules(&p);
        append_rule(&p, "dbviewer2.exe", "work", 0.9).unwrap();
        append_rule(&p, "dbviewr3.exe", "study", 0.9).unwrap();
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn appending_to_a_missing_file_creates_it_from_the_defaults() {
        let p = tmp("append-missing");
        assert!(!p.exists());
        let rs = append_rule(&p, "Zed.exe", "work", 0.9).unwrap();
        assert!(p.exists());
        // 默认规则不该丢
        assert_eq!(rs.classify(Some("Code.exe")).category.as_str(), "work");
        assert_eq!(rs.classify(Some("Zed.exe")).category.as_str(), "work");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn a_process_name_with_a_quote_is_refused_rather_than_corrupting_the_file() {
        // 进程名会被原样写进 TOML 字符串。带引号/反斜杠的那一条会让整份
        // 文件解析失败 —— 那等于应用自己把用户的规则文件写坏了。
        let p = tmp("append-quote");
        let _ = load_rules(&p);
        let before = fs::read_to_string(&p).unwrap();

        let result = append_rule(&p, r#"we"ird.exe"#, "work", 0.9);
        assert!(result.is_err(), "带引号的进程名必须被挡下");
        assert_eq!(
            fs::read_to_string(&p).unwrap(),
            before,
            "挡下之后文件不能变"
        );
        let _ = fs::remove_file(&p);
    }
}
