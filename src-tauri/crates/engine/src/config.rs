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
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            idle_threshold_s: 300,
            grace_period_s: 60,
            min_segment_duration_s: 30,
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
}
