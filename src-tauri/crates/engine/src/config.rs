/// 可调参数。默认值取自 spec §7.3 的表格。
///
/// 这些值由 app 层从配置文件读进来传给引擎；engine 自己不碰磁盘（spec §4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineConfig {
    /// 无输入多少秒判定为空闲
    pub idle_threshold_s: u64,
    /// 短暂切换的宽限窗口
    pub grace_period_s: u64,
    /// 短于此的段并入相邻段
    pub min_segment_duration_s: u64,
    /// 观测空档超过多少秒就认为「这段时间没有证据」，段在此收尾。
    ///
    /// **为什么必须有它**（2026-10-12 事故）：`extend` 用
    /// `end_at = end_at.max(ts)` 把段拉到下一条事件的时间戳。
    /// 应用退出 / 机器睡眠的整段时间里**一条事件都没有**，于是那段空白
    /// 被当成同一应用的连续工作记了下来 —— 一次停机凭空造出 11 小时。
    /// 两条这样的段让 24h 时段分布 18/24 根柱子满格、专注度大片 100%。
    ///
    /// **与 `idle_threshold_s` 不是一回事，不要合并**：那条是「没有输入」
    /// （采集得到 heartbeat，判定用户没动），这条是「**没有观测**」
    /// （压根没有事件，可能是进程没跑）。前者记 idle，后者**什么都不记**。
    ///
    /// 默认 2 小时：足够容下「合盖午休」「开会走开」这类真实空档，
    /// 又远小于 2026-10-12 那次 7~11 小时的停机。
    pub max_observation_gap_s: u64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            idle_threshold_s: 300,
            grace_period_s: 60,
            min_segment_duration_s: 30,
            max_observation_gap_s: 2 * 3600,
        }
    }
}

impl EngineConfig {
    pub fn with_idle_threshold_s(mut self, v: u64) -> Self {
        self.idle_threshold_s = v;
        self
    }

    pub fn with_grace_period_s(mut self, v: u64) -> Self {
        self.grace_period_s = v;
        self
    }

    pub fn with_min_segment_duration_s(mut self, v: u64) -> Self {
        self.min_segment_duration_s = v;
        self
    }

    pub fn with_max_observation_gap_s(mut self, v: u64) -> Self {
        self.max_observation_gap_s = v;
        self
    }
}
