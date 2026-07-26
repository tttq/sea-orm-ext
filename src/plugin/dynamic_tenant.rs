//! # 动态租户管理 Summer 插件
//!
//! 在 [`TenantPlugin`]（基础多租户）之上提供基于主库查询的动态租户管理能力：
//!
//! - 启动时通过用户实现的 [`DynamicTenantConfigProvider`] 从主库加载所有租户配置
//! - 自动建立租户连接并缓存到 `ConnectionStore`
//! - 后台定时健康检查，连续失败的租户自动从缓存移除
//! - 运行时通过 [`TenantManager`] 动态增删改租户，自动同步缓存
//!
//! ## TOML 配置
//!
//! ```toml
//! [summer-sea-orm-ext-dynamic-tenant]
//! enabled = true
//! health_check_interval_secs = 60
//! health_check_failure_threshold = 3
//! auto_remove_on_failure = true
//! ```
//!
//! ## 用户使用示例
//!
//! ```ignore
//! use async_trait::async_trait;
//! use summer_sea_orm_ext::plugin::dynamic_tenant::{
//!     DynamicTenantPlugin, DynamicTenantConfigProviderComponent,
//! };
//! use summer_sea_orm_ext::{DynamicTenantConfigProvider, TenantConnectionConfig};
//! use summer::App;
//! use summer::plugin::MutableComponentRegistry;
//!
//! struct AppTenantConfigProvider;
//!
//! #[async_trait]
//! impl DynamicTenantConfigProvider for AppTenantConfigProvider {
//!     async fn load_all(&self, main_db: &sea_orm::DatabaseConnection)
//!         -> Result<Vec<TenantConnectionConfig>, sea_orm::DbErr>
//!     {
//!         // 用户实现：从主库租户表查询所有启用的租户
//!         Ok(Vec::new())
//!     }
//!
//!     async fn load_one(&self, main_db: &sea_orm::DatabaseConnection, tenant_id: &str)
//!         -> Result<Option<TenantConnectionConfig>, sea_orm::DbErr>
//!     {
//!         Ok(None)
//!     }
//! }
//!
//! fn build_app() -> summer::app::AppBuilder {
//!     let mut app = App::new();
//!     app.add_plugin(DynamicTenantPlugin::new())
//!         .add_component(DynamicTenantConfigProviderComponent::new(
//!             std::sync::Arc::new(AppTenantConfigProvider),
//!         ));
//!     app
//! }
//! ```

use crate::{
    dynamic_tenant::{DynamicTenantConfig, DynamicTenantConfigProvider, TenantManager},
    get_tenant_mode, HashMapConnectionStore, TenantMode,
};
use schemars::JsonSchema;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use summer::async_trait;
use summer::config::{Configurable, ConfigRegistry};
use summer::plugin::{ComponentRegistry, MutableComponentRegistry, Plugin};
use summer::app::AppBuilder;

/// 包装用户的 [`DynamicTenantConfigProvider`] 实现，注册到组件容器。
#[derive(Clone)]
pub struct DynamicTenantConfigProviderComponent {
    pub provider: Arc<dyn DynamicTenantConfigProvider>,
}

impl DynamicTenantConfigProviderComponent {
    pub fn new(provider: Arc<dyn DynamicTenantConfigProvider>) -> Self {
        Self { provider }
    }
}

/// 包装 [`TenantManager`]，注册到组件容器供业务层依赖注入。
#[derive(Clone)]
pub struct TenantManagerComponent {
    pub manager: TenantManager,
}

impl TenantManagerComponent {
    pub fn new(manager: TenantManager) -> Self {
        Self { manager }
    }

    /// 获取内部 `TenantManager` 的克隆（Arc 内部状态共享）。
    pub fn manager(&self) -> TenantManager {
        self.manager.clone()
    }
}

summer::submit_config_schema!("summer-sea-orm-ext-dynamic-tenant", DynamicTenantPluginConfig);

/// 动态租户插件配置
///
/// 对应 TOML 配置 `[summer-sea-orm-ext-dynamic-tenant]` 段。
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, Configurable)]
#[config_prefix = "summer-sea-orm-ext-dynamic-tenant"]
pub struct DynamicTenantPluginConfig {
    /// 是否启用动态租户管理（默认 false）
    #[serde(default)]
    pub enabled: bool,
    /// 健康检查间隔（秒，默认 60）
    #[serde(default)]
    pub health_check_interval_secs: Option<u64>,
    /// 连续失败阈值（默认 3，达到后自动从缓存移除）
    #[serde(default)]
    pub health_check_failure_threshold: Option<u32>,
    /// 达到阈值后是否自动从缓存移除（默认 true）
    #[serde(default)]
    pub auto_remove_on_failure: Option<bool>,
}

/// 动态租户管理插件
///
/// **必须在 `TenantPlugin` 之后注册**，依赖以下前置条件：
/// - `TenantMode::Database` 模式
/// - 主库 `DatabaseConnection` 已通过 `SeaOrmPlugin` 注册
/// - （可选）`ConnectionStore` 已由 `TenantPlugin` 创建；否则本插件会自建一个
pub struct DynamicTenantPlugin;

impl DynamicTenantPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DynamicTenantPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for DynamicTenantPlugin {
    async fn build(&self, app: &mut AppBuilder) {
        let config = app
            .get_config::<DynamicTenantPluginConfig>()
            .expect("dynamic tenant plugin config load failed");

        if !config.enabled {
            tracing::info!("DYNAMIC-TENANT: plugin disabled, skipping initialization");
            return;
        }

        // 校验多租户已启用（mode 由全局配置或运行时 provider 决定）
        //
        // v0.0.3+ 支持混合模式：全局 `mode = "table"` + 部分租户运行时 database 模式。
        // 因此不再强制要求全局 mode 为 Database，只要 tenant 已启用，插件就启动：
        //   - 全局 Database 模式：所有租户走租户库（原行为）
        //   - 全局 Table 模式 + 运行时 provider 返回 "database"：混合模式，
        //     部分租户走租户库，其余走主库 + WHERE tenant_id
        //   - 全局 Table 模式 + provider 未实现 get_tenant_mode()：插件启动但 store
        //     无租户连接，所有查询走主库（与未启用插件等效）
        match get_tenant_mode() {
            Some(TenantMode::Database) => {
                tracing::info!("DYNAMIC-TENANT: global tenant mode is 'Database'");
            }
            Some(TenantMode::Table) => {
                tracing::info!(
                    "DYNAMIC-TENANT: global tenant mode is 'Table', plugin started for hybrid mode \
                     (tenants with runtime mode='database' will be routed to tenant DBs)"
                );
            }
            None => {
                tracing::warn!(
                    "DYNAMIC-TENANT: tenant config not found, please register TenantPlugin before DynamicTenantPlugin"
                );
                return;
            }
        }

        // 1. 获取主库连接
        let main_db = match app.get_component::<DatabaseConnection>() {
            Some(db) => db,
            None => {
                tracing::error!(
                    "DYNAMIC-TENANT: main DatabaseConnection not found in component registry, \
                     please register SeaOrmPlugin before DynamicTenantPlugin"
                );
                return;
            }
        };

        // 2. 获取 DynamicTenantConfigProvider（优先从 config 字段，其次从组件容器）
        let provider: Arc<dyn DynamicTenantConfigProvider> =
            match app.get_component::<DynamicTenantConfigProviderComponent>() {
                Some(c) => c.provider.clone(),
                None => {
                    tracing::error!(
                        "DYNAMIC-TENANT: DynamicTenantConfigProviderComponent not found, \
                         please register it via app.add_component(DynamicTenantConfigProviderComponent::new(...))"
                    );
                    return;
                }
            };

        // 3. 获取或创建 ConnectionStore
        //    若 TenantPlugin 已在 Database 模式下注册过 store，复用之；否则新建并注册
        let store = match crate::get_tenant_store() {
            Some(existing) => existing,
            None => {
                let new_store: Arc<dyn crate::ConnectionStore> =
                    Arc::new(HashMapConnectionStore::new());
                crate::set_tenant_store(new_store.clone());
                tracing::info!("DYNAMIC-TENANT: created new ConnectionStore (TenantPlugin did not create one)");
                new_store
            }
        };

        // 4. 构造 DynamicTenantConfig
        let dyn_config = DynamicTenantConfig {
            enabled: true,
            health_check_interval_secs: config.health_check_interval_secs,
            health_check_failure_threshold: config.health_check_failure_threshold,
            auto_remove_on_failure: config.auto_remove_on_failure,
        };

        // 5. 创建 TenantManager
        let manager = TenantManager::new(main_db, store, provider, dyn_config);

        // 6. 启动时初始化：从主库加载所有租户
        if let Err(e) = manager.initialize().await {
            tracing::error!(
                "DYNAMIC-TENANT: initialization failed: {} (some tenants may be unavailable)",
                e
            );
        }

        // 7. 启动后台健康检查任务
        #[cfg(feature = "runtime-tokio")]
        {
            manager.start_health_check();
            tracing::info!("DYNAMIC-TENANT: background health check task started");
        }

        // 8. 注册 TenantManagerComponent 供业务层依赖注入
        app.add_component(TenantManagerComponent::new(manager.clone()));
        tracing::info!("DYNAMIC-TENANT: plugin initialized, TenantManagerComponent registered");
    }

    fn name(&self) -> &'static str {
        "summer-sea-orm-ext-dynamic-tenant"
    }
}
