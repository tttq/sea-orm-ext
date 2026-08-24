//! # 动态租户管理模块
//!
//! 提供基于主库查询的动态租户连接管理，**仅在 `TenantMode::Database` 下生效**。
//!
//! ## 工作原理
//!
//! 1. 用户实现 [`DynamicTenantConfigProvider`] trait，从主库查询租户连接配置
//! 2. `DynamicTenantPlugin` 启动时自动调用 `load_all()` 加载所有租户
//! 3. 框架为每个租户创建数据库连接并缓存到 `ConnectionStore`
//! 4. 后台健康检查任务定时 ping 所有租户库，连续失败的自动移除
//! 5. 业务层通过 [`TenantManager`] 动态增删改租户，自动同步缓存
//!
//! ## 用户使用示例
//!
//! ```ignore
//! use async_trait::async_trait;
//! use sea_orm::{DatabaseConnection, DbErr, EntityTrait, QueryFilter};
//! use sea_orm_ext::{
//!     DynamicTenantConfigProvider, TenantConnectionConfig,
//! };
//!
//! pub struct AppTenantConfigProvider;
//!
//! #[async_trait]
//! impl DynamicTenantConfigProvider for AppTenantConfigProvider {
//!     async fn load_all(&self, main_db: &DatabaseConnection) -> Result<Vec<TenantConnectionConfig>, DbErr> {
//!         // 用户自定义查询：SELECT * FROM sys_tenant WHERE enabled = true
//!         let rows = sys_tenant::Entity::find()
//!             .filter(sys_tenant::Column::Enabled.eq(true))
//!             .all(main_db).await?;
//!         Ok(rows.into_iter().map(Into::into).collect())
//!     }
//!
//!     async fn load_one(&self, main_db: &DatabaseConnection, tenant_id: &str) -> Result<Option<TenantConnectionConfig>, DbErr> {
//!         let row = sys_tenant::Entity::find()
//!             .filter(sys_tenant::Column::TenantId.eq(tenant_id))
//!             .one(main_db).await?;
//!         Ok(row.map(Into::into))
//!     }
//! }
//! ```

use async_trait::async_trait;
use sea_orm::{ConnectOptions, DatabaseConnection, DbErr};
use sea_query::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::tenant_store::ConnectionStore;
use crate::SeaOrmExtConnection;

/// 租户连接配置
///
/// 由框架定义的数据结构，与用户的 Entity 无关。
/// 用户在自己的 Entity 上实现 `From<Model> for TenantConnectionConfig` 转换。
#[derive(Debug, Clone)]
pub struct TenantConnectionConfig {
    /// 租户业务标识
    pub tenant_id: String,
    /// 数据库连接 URL
    pub db_url: String,
    /// 数据库驱动："postgres" | "mysql" | "sqlite"
    pub db_driver: String,
    /// 最大连接数（默认 50）
    pub max_connections: Option<u32>,
    /// 最小连接数（默认 5）
    pub min_connections: Option<u32>,
    /// 连接超时（秒，默认 30）
    pub connect_timeout_secs: Option<u64>,
    /// 获取连接超时（秒，默认 30）
    pub acquire_timeout_secs: Option<u64>,
    /// 空闲连接超时（秒，默认 600）
    pub idle_timeout_secs: Option<u64>,
    /// 是否开启 sqlx 原生日志
    pub enable_logging: Option<bool>,
}

impl Default for TenantConnectionConfig {
    fn default() -> Self {
        Self {
            tenant_id: String::new(),
            db_url: String::new(),
            db_driver: "postgres".to_string(),
            max_connections: Some(50),
            min_connections: Some(5),
            connect_timeout_secs: Some(30),
            acquire_timeout_secs: Some(30),
            idle_timeout_secs: Some(600),
            enable_logging: Some(false),
        }
    }
}

/// 动态租户连接配置 provider
///
/// 由用户实现，从主库租户表查询并返回所需的连接参数。
/// 框架不关心租户表的结构和 Entity 定义，只通过此 trait 获取连接信息。
///
/// `main_db` 由框架自动传入（从 `SeaOrmExtPlugin` 注册的主库连接），
/// 用户实现时无需自己持有主库连接。
#[async_trait]
pub trait DynamicTenantConfigProvider: Send + Sync + 'static {
    /// 启动时加载所有启用的租户配置（从主库查询）
    ///
    /// 框架在 `DynamicTenantPlugin::build()` 时自动调用此方法，
    /// 传入主库连接，用户只需执行查询并返回配置列表。
    async fn load_all(&self, main_db: &DatabaseConnection) -> Result<Vec<TenantConnectionConfig>, DbErr>;

    /// 查询单个租户配置（动态添加/更新时使用）
    ///
    /// `TenantManager::add_tenant()` 和 `update_tenant()` 会调用此方法。
    async fn load_one(
        &self,
        main_db: &DatabaseConnection,
        tenant_id: &str,
    ) -> Result<Option<TenantConnectionConfig>, DbErr>;
}

// ============================================================================
// TenantManager
// ============================================================================

/// 动态租户管理器内部状态
struct TenantManagerInner {
    /// 主库连接（用于查询租户配置表）
    main_db: DatabaseConnection,
    /// 租户连接缓存
    store: Arc<dyn ConnectionStore>,
    /// 用户的配置 provider
    provider: Arc<dyn DynamicTenantConfigProvider>,
    /// 动态租户管理配置
    #[allow(dead_code)] // 在非 runtime-tokio feature 下不读取（健康检查被 cfg 掉）
    config: DynamicTenantConfig,
    /// 健康检查失败计数（tenant_id -> 连续失败次数）
    failure_counts: std::sync::Mutex<HashMap<String, u32>>,
}

/// 动态租户管理器
///
/// 由 `DynamicTenantPlugin` 自动创建并注册为组件。
/// 业务层通过依赖注入获取，调用以下方法同步缓存。
///
/// # 注意
///
/// 此管理器**仅在 `TenantMode::Database` 下激活**。
/// 所有方法只同步缓存，**不写主库**。主库的增删改由业务层使用自己的 Entity 完成。
#[derive(Clone)]
pub struct TenantManager {
    inner: Arc<TenantManagerInner>,
}

impl TenantManager {
    /// 创建新的 TenantManager
    pub fn new(
        main_db: DatabaseConnection,
        store: Arc<dyn ConnectionStore>,
        provider: Arc<dyn DynamicTenantConfigProvider>,
        config: DynamicTenantConfig,
    ) -> Self {
        Self {
            inner: Arc::new(TenantManagerInner {
                main_db,
                store,
                provider,
                config,
                failure_counts: std::sync::Mutex::new(HashMap::new()),
            }),
        }
    }

    /// 获取主库连接（业务层写租户表时使用）
    pub fn main_db(&self) -> &DatabaseConnection {
        &self.inner.main_db
    }

    /// 获取内部 ConnectionStore 的 Arc 引用（供测试和高级用法使用）
    pub fn inner_get_store(&self) -> Arc<dyn ConnectionStore> {
        self.inner.store.clone()
    }

    /// 启动时初始化：从主库加载所有 enabled 租户，建立连接并缓存
    ///
    /// 由 `DynamicTenantPlugin::build()` 自动调用。
    pub async fn initialize(&self) -> Result<(), DbErr> {
        tracing::info!("DYNAMIC-TENANT: initializing tenant connections from main database...");

        let configs = self.inner.provider.load_all(&self.inner.main_db).await?;
        let total = configs.len();
        let mut connected = 0usize;

        for cfg in configs {
            match self.connect_and_cache(&cfg).await {
                Ok(()) => {
                    tracing::info!(
                        "DYNAMIC-TENANT: connected to tenant {} (driver={}, url={})",
                        cfg.tenant_id, cfg.db_driver, cfg.db_url
                    );
                    connected += 1;
                }
                Err(e) => {
                    tracing::error!(
                        "DYNAMIC-TENANT: failed to connect to tenant {}: {} (skip, continue with remaining)",
                        cfg.tenant_id, e
                    );
                }
            }
        }

        tracing::info!(
            "DYNAMIC-TENANT: initialization complete: {}/{} tenants connected",
            connected,
            total
        );

        Ok(())
    }

    /// 动态添加租户：从主库查询配置 → 建立连接 → 缓存
    ///
    /// **注意**：此方法只同步缓存，不写主库。
    /// 主库的插入由业务层使用自己的 Entity 完成，完成后调用此方法刷新缓存。
    pub async fn add_tenant(&self, tenant_id: &str) -> Result<(), DbErr> {
        tracing::info!("DYNAMIC-TENANT: adding tenant {}", tenant_id);

        let cfg = self
            .inner
            .provider
            .load_one(&self.inner.main_db, tenant_id)
            .await?
            .ok_or_else(|| {
                DbErr::Custom(format!(
                    "DYNAMIC-TENANT: tenant {} not found in main database or not enabled",
                    tenant_id
                ))
            })?;

        self.connect_and_cache(&cfg).await?;

        tracing::info!("DYNAMIC-TENANT: tenant {} added to cache", tenant_id);
        Ok(())
    }

    /// 动态更新租户：关闭旧连接 → 查询新配置 → 建立新连接 → 替换缓存
    ///
    /// **注意**：此方法只同步缓存，不写主库。
    pub async fn update_tenant(&self, tenant_id: &str) -> Result<(), DbErr> {
        tracing::info!("DYNAMIC-TENANT: updating tenant {}", tenant_id);

        // 先移除旧连接
        let tenant_value = Value::String(Some(tenant_id.to_string()));
        self.inner.store.remove(&tenant_value)?;

        // 重新查询并建立连接
        self.add_tenant(tenant_id).await?;

        tracing::info!("DYNAMIC-TENANT: tenant {} updated in cache", tenant_id);
        Ok(())
    }

    /// 动态移除租户：从缓存移除
    ///
    /// **注意**：此方法只清理缓存，不删主库记录。
    pub async fn remove_tenant(&self, tenant_id: &str) -> Result<(), DbErr> {
        tracing::info!("DYNAMIC-TENANT: removing tenant {} from cache", tenant_id);

        let tenant_value = Value::String(Some(tenant_id.to_string()));
        self.inner.store.remove(&tenant_value)?;

        // 清理失败计数
        self.inner.failure_counts.lock().unwrap().remove(tenant_id);

        tracing::info!("DYNAMIC-TENANT: tenant {} removed from cache", tenant_id);
        Ok(())
    }

    /// 全量重载：清空缓存 → 重新从主库加载所有租户
    pub async fn refresh_cache(&self) -> Result<(), DbErr> {
        tracing::info!("DYNAMIC-TENANT: refreshing cache (full reload)");

        // 清理失败计数
        self.inner.failure_counts.lock().unwrap().clear();

        // 重新初始化
        self.initialize().await
    }

    /// 启动后台健康检查任务
    ///
    /// 由 `DynamicTenantPlugin::build()` 自动调用。
    /// 仅在 `runtime-tokio` feature 下有效。
    #[cfg(feature = "runtime-tokio")]
    pub fn start_health_check(&self) -> tokio::task::JoinHandle<()> {
        let manager = self.clone();
        let interval = Duration::from_secs(
            manager.inner.config.health_check_interval_secs.unwrap_or(60),
        );

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                manager.run_health_check().await;
            }
        })
    }

    /// 执行一次健康检查
    ///
    /// 遍历所有缓存连接，ping 失败的累计失败次数，达到阈值自动移除。
    #[cfg(feature = "runtime-tokio")]
    async fn run_health_check(&self) {
        let tenants = self.inner.store.get_all_tenants();
        let threshold = self.inner.config.health_check_failure_threshold.unwrap_or(3);
        let auto_remove = self.inner.config.auto_remove_on_failure.unwrap_or(true);

        for tenant_id in tenants {
            let tenant_str = match &tenant_id {
                Value::String(Some(s)) => s.clone(),
                _ => continue,
            };

            let ext_conn = match self.inner.store.get_ext(&tenant_id) {
                Some(c) => c,
                None => continue,
            };

            match ext_conn.ping().await {
                Ok(()) => {
                    // 健康恢复，重置失败计数
                    self.inner.failure_counts.lock().unwrap().remove(&tenant_str);
                    tracing::trace!(
                        "DYNAMIC-TENANT: health check OK for tenant {}",
                        tenant_str
                    );
                }
                Err(e) => {
                    let mut counts = self.inner.failure_counts.lock().unwrap();
                    let count = counts.entry(tenant_str.clone()).or_insert(0);
                    *count += 1;

                    if *count >= threshold && auto_remove {
                        tracing::warn!(
                            "DYNAMIC-TENANT: tenant {} failed health check {}/{} times, removing from cache: {}",
                            tenant_str, count, threshold, e
                        );
                        drop(counts);

                        let _ = self.inner.store.remove(&tenant_id);
                        self.inner.failure_counts.lock().unwrap().remove(&tenant_str);
                    } else {
                        tracing::warn!(
                            "DYNAMIC-TENANT: tenant {} health check failed ({}/{}): {}",
                            tenant_str, count, threshold, e
                        );
                    }
                }
            }
        }
    }

    /// 根据配置创建数据库连接并缓存到 ConnectionStore
    async fn connect_and_cache(&self, cfg: &TenantConnectionConfig) -> Result<(), DbErr> {
        let mut opt = ConnectOptions::new(cfg.db_url.clone());
        opt.max_connections(cfg.max_connections.unwrap_or(50))
            .min_connections(cfg.min_connections.unwrap_or(5))
            .connect_timeout(Duration::from_secs(
                cfg.connect_timeout_secs.unwrap_or(30),
            ))
            .acquire_timeout(Duration::from_secs(
                cfg.acquire_timeout_secs.unwrap_or(30),
            ))
            .idle_timeout(Duration::from_secs(
                cfg.idle_timeout_secs.unwrap_or(600),
            ))
            .sqlx_logging(cfg.enable_logging.unwrap_or(false));

        let conn = sea_orm::Database::connect(opt).await?;
        let ext_conn = SeaOrmExtConnection::new(conn);

        self.inner.store.insert_ext(
            Value::String(Some(cfg.tenant_id.clone())),
            ext_conn,
        )?;

        Ok(())
    }
}

// ============================================================================
// DynamicTenantConfig
// ============================================================================

/// 动态租户管理配置
///
/// 对应 TOML 配置 `[sea-orm-ext-dynamic-tenant]` 段。
///
/// ```toml
/// [sea-orm-ext-dynamic-tenant]
/// enabled = true
/// health_check_interval_secs = 60
/// health_check_failure_threshold = 3
/// auto_remove_on_failure = true
/// ```
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DynamicTenantConfig {
    /// 是否启用动态租户管理
    #[serde(default)]
    pub enabled: bool,
    /// 健康检查间隔（秒，默认 60）
    #[serde(default)]
    pub health_check_interval_secs: Option<u64>,
    /// 连续失败阈值（默认 3，达到后自动移除）
    #[serde(default)]
    pub health_check_failure_threshold: Option<u32>,
    /// 达到阈值后是否自动从缓存移除（默认 true）
    #[serde(default)]
    pub auto_remove_on_failure: Option<bool>,
}
