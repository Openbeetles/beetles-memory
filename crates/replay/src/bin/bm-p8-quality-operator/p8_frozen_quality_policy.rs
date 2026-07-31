//! 唯一可变的 P8 quality policy 物理锚点。
//!
//! 该文件必须从其授权的 unanchored P8 source fingerprint 中排除。只有在真实 baseline、
//! threshold freeze 与 pre-anchor review 全部完成后，才允许把 `None` 确定性改写为 `Some`。

#[allow(dead_code)]
pub(super) struct P8FrozenQualityPolicy {
    pub(super) protocol_digest: &'static str,
    pub(super) threshold_digest: &'static str,
}

#[allow(dead_code)]
pub(super) const P8_FROZEN_QUALITY_POLICY: Option<P8FrozenQualityPolicy> = None;
