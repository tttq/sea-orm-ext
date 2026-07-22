//! 数据库健康检查模块。
//!
//! 提供对默认数据库 fallback 链、租户专属数据库、以及任意 `DatabaseConnection`
//! 的健康状态进行检查的功能。所有检查均为异步，基于 `sea_orm::ConnectionTrait::ping()`。
//!
//! # 用法
//!
//! ```ignore
//! use summer_sea_orm_ext::health::check_default_databases;
//!
//! # async fn run() {
//! let report = check_default_databases().await;
//! println!("{}", report.summary());
//! for entry in &report.entries {
//!     println!("- {:?}", entry);
//! }
//! # }
//! ```

use sea_orm::{DatabaseConnection, DbErr};
use std::time::Duration;

/// 单个数据库的健康检查结果。
#[derive(Debug, Clone)]
pub struct HealthEntry {
    /// 数据库标识（如 URL、tenant_id 或 fallback 索引）。
    pub label: String,
    /// 是否健康（ping 成功）。
    pub healthy: bool,
    /// 错误信息（若不健康）。
    pub error: Option<String>,
    /// ping 耗时。
    pub elapsed: Duration,
}

impl HealthEntry {
    fn ok(label: impl Into<String>, elapsed: Duration) -> Self {
        Self {
            label: label.into(),
            healthy: true,
            error: None,
            elapsed,
        }
    }

    fn fail(label: impl Into<String>, err: DbErr, elapsed: Duration) -> Self {
        Self {
            label: label.into(),
            healthy: false,
            error: Some(err.to_string()),
            elapsed,
        }
    }
}

/// 健康检查报告。
#[derive(Debug, Clone, Default)]
pub struct HealthReport {
    pub entries: Vec<HealthEntry>,
}

impl HealthReport {
    /// 所有数据库是否都健康。
    pub fn all_healthy(&self) -> bool {
        !self.entries.is_empty() && self.entries.iter().all(|e| e.healthy)
    }

    /// 至少有一个数据库健康（可用于判断系统是否可用）。
    pub fn any_healthy(&self) -> bool {
        self.entries.iter().any(|e| e.healthy)
    }

    /// 健康数据库的数量。
    pub fn healthy_count(&self) -> usize {
        self.entries.iter().filter(|e| e.healthy).count()
    }

    /// 总数据库数量。
    pub fn total_count(&self) -> usize {
        self.entries.len()
    }

    /// 简短摘要：`healthy/total`。
    pub fn summary(&self) -> String {
        format!("{}/{}", self.healthy_count(), self.total_count())
    }
}

/// 对单个数据库连接执行 ping 检查。
pub async fn check_database(label: impl Into<String>, db: &DatabaseConnection) -> HealthEntry {
    let label = label.into();
    let start = std::time::Instant::now();
    match db.ping().await {
        Ok(()) => HealthEntry::ok(label, start.elapsed()),
        Err(e) => HealthEntry::fail(label, e, start.elapsed()),
    }
}

/// 检查默认数据库 fallback 链的健康状态。
///
/// 返回 fallback 链中每个数据库的检查结果。若 fallback 链为空，
/// 但设置了单库默认数据库，则检查单库。
pub async fn check_default_databases() -> HealthReport {
    let dbs = crate::get_default_databases();
    let mut entries = Vec::with_capacity(dbs.len());

    for (idx, db) in dbs.iter().enumerate() {
        let label = format!("default_fallback[{}]", idx);
        entries.push(check_database(label, db).await);
    }

    if entries.is_empty() {
        if let Some(db) = crate::get_default_database() {
            entries.push(check_database("default_single", &db).await);
        }
    }

    HealthReport { entries }
}

/// 检查指定租户的数据库连接健康状态。
///
/// 若租户存在专属连接，则检查之；否则回退到默认数据库健康检查。
pub async fn check_tenant_database(tenant_id: &sea_query::Value) -> Result<HealthEntry, DbErr> {
    let label = format!("tenant:{:?}", tenant_id);

    // 优先从 store 获取租户专属连接
    if let Some(store) = crate::get_tenant_store() {
        if let Some(conn) = store.get(tenant_id) {
            return Ok(check_database(label, &conn).await);
        }
    }

    // 回退到默认 fallback 链中的第一个（同步获取，不做 ping 检查）
    if let Some(db) = crate::get_available_default_database() {
        return Ok(check_database(format!("{}->fallback", label), &db).await);
    }

    Err(DbErr::Custom(format!(
        "HEALTH: no database available for tenant {:?}",
        tenant_id
    )))
}

/// 检查所有已注册租户的数据库连接健康状态。
///
/// 遍历 `tenant_store` 中所有租户，逐个 ping。返回完整报告。
pub async fn check_all_tenant_databases() -> HealthReport {
    let mut entries = Vec::new();

    if let Some(store) = crate::get_tenant_store() {
        for tenant_id in store.get_all_tenants() {
            let label = format!("tenant:{:?}", tenant_id);
            if let Some(conn) = store.get(&tenant_id) {
                entries.push(check_database(label, &conn).await);
            }
        }
    }

    if entries.is_empty() {
        // 没有租户专属连接，检查默认 fallback 链
        return check_default_databases().await;
    }

    HealthReport { entries }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_report_summary() {
        let report = HealthReport {
            entries: vec![
                HealthEntry::ok("db1", Duration::from_millis(5)),
                HealthEntry::ok("db2", Duration::from_millis(10)),
                HealthEntry::fail("db3", DbErr::Custom("conn refused".to_owned()), Duration::from_millis(2)),
            ],
        };
        assert_eq!(report.total_count(), 3);
        assert_eq!(report.healthy_count(), 2);
        assert!(!report.all_healthy());
        assert!(report.any_healthy());
        assert_eq!(report.summary(), "2/3");
    }

    #[test]
    fn test_empty_report() {
        let report = HealthReport::default();
        assert_eq!(report.total_count(), 0);
        assert_eq!(report.healthy_count(), 0);
        assert!(!report.all_healthy());
        assert!(!report.any_healthy());
        assert_eq!(report.summary(), "0/0");
    }
}
