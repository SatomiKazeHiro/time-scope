//! 应用状态：几条线程与 IPC 之间共享的东西。
//!
//! 从 `lib.rs` 里拆出来，是为了让「共享什么」成为一份可以独立读完的清单 ——
//! 入口函数那边只剩启动顺序与插件顺序。
//!
//! 这里的每个字段都至少被两方读写（IPC / 采集线程 / 引擎线程），
//! 所以字段本身是 `pub(crate)`，但**跨模块的写只走方法**（见 `replace_rules`）。

use crate::day_replay::ReplayedDays;
use crate::engine_runtime::EngineRuntime;
use crate::redact::Redactor;
use activity_engine::{EngineConfig, RuleSet};
use activity_storage::{BatchWriter, SharedConn};
use std::sync::{Arc, Mutex};

pub struct AppState {
    pub(crate) writer: Arc<BatchWriter>,
    /// 与 writer 共用同一个连接，供"按需重放历史日期"用。
    pub(crate) conn: SharedConn,
    pub(crate) engine: Arc<Mutex<EngineRuntime>>,
    /// 已重放过的日期。启动只重放"今天"；用户翻到别的日子时按需重放，
    /// 把上次崩溃留下的孤儿事件补成分段（spec §7.4）。
    pub(crate) replayed: ReplayedDays,
    /// 当前生效的规则集，供**按需重放**读（引擎实时流那份在 `EngineRuntime` 里）。
    ///
    /// 为什么是两份：引擎线程持有 `EngineRuntime`（它自己一份 `rules`），
    /// 而重放走的是 `day_replay`，不经过引擎线程。所以换规则时两边都要换 ——
    /// 那是 `replace_rules` 存在的原因。
    pub(crate) rules: Arc<Mutex<RuleSet>>,
    pub(crate) config: EngineConfig,
    /// 标题脱敏器。consumer 与"按需重放"两条路径共用（spec §11）。
    pub(crate) redactor: Arc<Redactor>,
    /// 原始事件保留天数（B3）。0 = 永不删。
    ///
    /// 共享可变：从 365 天改成 30 天应当**立刻**按新值清理，而不是等下次启动。
    pub(crate) retention_days: Arc<Mutex<u32>>,
    /// `config.toml` 的路径。写回要定位文件；`residency` 里也有一份同样的。
    pub(crate) config_path: std::path::PathBuf,
}

impl AppState {
    /// 换规则集 —— **唯一入口**。
    ///
    /// 规则集有两个读者：引擎实时流（`EngineRuntime.rules`）与按需重放
    /// （`AppState.rules`）。必须同时换，否则会「时间线用新规则、历史重放用旧规则」，
    /// 而那种错读代码看不出来。原来这段写在 `add_rule` 里、靠一行注释提醒；
    /// 现在是一个方法，将来出现第三个读者也必须走这里。
    pub fn replace_rules(&self, new_set: RuleSet) {
        if let Ok(mut rt) = self.engine.lock() {
            rt.rules = new_set.clone();
        }
        if let Ok(mut rs) = self.rules.lock() {
            *rs = new_set;
        }
    }

    /// 当前保留天数（0 = 永不删）。每次现读 —— 设置页改完，下一次轮询就按新值清理。
    pub fn retention_days(&self) -> u32 {
        *self
            .retention_days
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    /// 当前生效的规则集快照（重放用）。
    pub fn rules_snapshot(&self) -> RuleSet {
        self.rules
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
}
