
//! SQL 日志模块
//!
//! 提供可配置的 SQL 日志开关，配合 [`crate::SeaOrmExtConnection`] 使用。
//!
//! # 工作原理
//!
//! 1. 通过全局原子开关控制是否打印 SQL 日志
//! 2. [`crate::SeaOrmExtConnection`] 在执行每条 SQL 前检查开关
//! 3. 开启时，利用 `sea_orm::Statement` 的 `Display` 实现
//!    将参数值注入到 SQL 模板中，输出完整可执行 SQL
//!
//! # 配置方式
//!
//! **方式一：Summer Plugin 配置（推荐）**
//!
//! ```toml
//! [summer-sea-orm-ext]
//! enable_sql_log = true
//! ```
//!
//! **方式二：运行时 API**
//!
//! ```ignore
//! use summer_sea_orm_ext::{enable_sql_log, disable_sql_log};
//!
//! enable_sql_log();   // 开启
//! disable_sql_log();  // 关闭
//! ```
//!
//! # 使用方式
//!
//! ```ignore
//! use summer_sea_orm_ext::{SeaOrmExtConnection, enable_sql_log};
//!
//! let db = Database::connect("...").await?;
//! let db = SeaOrmExtConnection::new(db);
//!
//! enable_sql_log();
//!
//! // 所有 SQL 操作都会打印完整语句
//! let users = User::find().all(&db).await?;
//! // 输出: [summer-sea-orm-ext SQL] SELECT "user"."id", "user"."name" FROM "user" WHERE "user"."is_deleted" = 0
//! ```

use std::sync::atomic::{AtomicBool, Ordering};

static SQL_LOG_ENABLED: AtomicBool = AtomicBool::new(false);

/// 启用 SQL 日志
pub fn enable_sql_log() {
    SQL_LOG_ENABLED.store(true, Ordering::SeqCst);
}

/// 禁用 SQL 日志
pub fn disable_sql_log() {
    SQL_LOG_ENABLED.store(false, Ordering::SeqCst);
}

/// 设置 SQL 日志开关状态
pub fn set_sql_log_enabled(enabled: bool) {
    SQL_LOG_ENABLED.store(enabled, Ordering::SeqCst);
}

/// 检查 SQL 日志是否启用
pub fn is_sql_log_enabled() -> bool {
    SQL_LOG_ENABLED.load(Ordering::SeqCst)
}
