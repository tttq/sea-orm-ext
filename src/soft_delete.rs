use sea_orm::EntityTrait;
use sea_query::Value;

/// 软删除 trait，为实体提供默认的软删除标记逻辑。
///
/// 实现此 trait 的实体需同时实现 `EntityTrait`。
/// 默认使用 `0` 表示未删除、`1` 表示已删除。
/// 可通过重写默认方法来定制删除标记值。
pub trait SoftDeleteTrait: EntityTrait {
    /// 软删除的"未删除"标记默认值（0）。
    ///
    /// 重写此方法可自定义未删除状态的值（如 `false`、`"active"` 等）。
    fn soft_delete_default() -> Value {
        0i32.into()
    }
    /// 软删除的"已删除"标记默认值（1）。
    ///
    /// 重写此方法可自定义已删除状态的值（如 `true`、`"deleted"` 等）。
    fn soft_delete_del() -> Value {
        1i32.into()
    }
}
