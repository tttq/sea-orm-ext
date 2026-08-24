use crate::{
    set_tenant_config, set_tenant_database_provider, set_tenant_id_provider, set_tenant_store,
    ConnectionStore, HashMapConnectionStore, SeaOrmExtConnection, TenantConfig, TenantDatabaseProvider, TenantIdProvider, TenantMode,
};
use std::collections::HashSet;
#[cfg(feature = "summer-web")]
use crate::{
    clear_tenant_context, get_tenant_database, get_tenant_mode,
    is_tenant_enabled, set_tenant_context,
};
use sea_orm::DatabaseConnection;
use sea_query::Value;
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::Arc;
use schemars::JsonSchema;
use summer::async_trait;
use summer::config::{Configurable, ConfigRegistry};
use summer::plugin::{ComponentRegistry, MutableComponentRegistry, Plugin};
use summer::app::AppBuilder;

fn deserialize_string_or_int<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct StringOrIntVisitor;

    impl<'de> Visitor<'de> for StringOrIntVisitor {
        type Value = String;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string or an integer")
        }

        fn visit_str<E>(self, v: &str) -> Result<String, E>
        where
            E: de::Error,
        {
            Ok(v.to_owned())
        }

        fn visit_i64<E>(self, v: i64) -> Result<String, E>
        where
            E: de::Error,
        {
            Ok(v.to_string())
        }

        fn visit_u64<E>(self, v: u64) -> Result<String, E>
        where
            E: de::Error,
        {
            Ok(v.to_string())
        }
    }

    deserializer.deserialize_any(StringOrIntVisitor)
}

fn deserialize_string_or_int_opt<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct StringOrIntOptVisitor;

    impl<'de> Visitor<'de> for StringOrIntOptVisitor {
        type Value = Option<String>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string, an integer, or null")
        }

        fn visit_none<E>(self) -> Result<Option<String>, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Option<String>, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D2>(self, deserializer: D2) -> Result<Option<String>, D2::Error>
        where
            D2: Deserializer<'de>,
        {
            deserialize_string_or_int(deserializer).map(Some)
        }

        fn visit_str<E>(self, v: &str) -> Result<Option<String>, E>
        where
            E: de::Error,
        {
            Ok(Some(v.to_owned()))
        }

        fn visit_i64<E>(self, v: i64) -> Result<Option<String>, E>
        where
            E: de::Error,
        {
            Ok(Some(v.to_string()))
        }

        fn visit_u64<E>(self, v: u64) -> Result<Option<String>, E>
        where
            E: de::Error,
        {
            Ok(Some(v.to_string()))
        }
    }

    deserializer.deserialize_option(StringOrIntOptVisitor)
}

#[derive(Debug, Clone, Serialize,JsonSchema, Deserialize)]
pub struct TenantDatabaseEntryConfig {
    pub url: String,
    pub max_connections: Option<u32>,
    pub min_connections: Option<u32>,
    pub connect_timeout_secs: Option<u64>,
    pub acquire_timeout_secs: Option<u64>,
    /// 连接池空闲连接超时（秒）。`None` 表示使用驱动默认值。
    #[serde(default)]
    pub idle_timeout_secs: Option<u64>,
    /// 是否开启 SQL 日志。
    #[serde(default)]
    pub enable_logging: bool,
}

impl Default for TenantDatabaseEntryConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            // 默认 50：与 DatabaseConfig 保持一致，适合高并发生产环境
            max_connections: Some(50),
            min_connections: Some(5),
            connect_timeout_secs: Some(30),
            acquire_timeout_secs: Some(30),
            idle_timeout_secs: Some(600),
            enable_logging: false,
        }
    }
}

#[derive(Debug, Clone, Serialize,JsonSchema, Deserialize)]
pub struct TenantDatabaseEntry {
    #[serde(deserialize_with = "deserialize_string_or_int")]
    pub tenant_id: String,
    pub database: TenantDatabaseEntryConfig,
}

#[derive(Clone)]
pub struct SaTokenLayerMarker;

#[derive(Clone)]
pub struct TenantIdProviderComponent {
    pub provider: Arc<dyn TenantIdProvider>,
}

impl TenantIdProviderComponent {
    pub fn new(provider: Arc<dyn TenantIdProvider>) -> Self {
        Self { provider }
    }
}

#[derive(Clone)]
pub struct TenantDatabaseProviderComponent {
    pub provider: Arc<dyn TenantDatabaseProvider>,
}

impl TenantDatabaseProviderComponent {
    pub fn new(provider: Arc<dyn TenantDatabaseProvider>) -> Self {
        Self { provider }
    }
}
#[derive(Clone, Serialize, Deserialize,JsonSchema, Configurable)]
#[config_prefix = "sea-orm-ext-tenant"]
pub struct TenantPluginConfig {
    pub enabled: bool,
    pub mode: String,
    pub database_source: Option<String>,
    #[serde(deserialize_with = "deserialize_string_or_int_opt")]
    pub default_tenant_id: Option<String>,
    pub databases: Option<Vec<TenantDatabaseEntry>>,
    /// 默认数据库列表（fallback 链）。
    ///
    /// TOML 中使用数组表语法，可定义多个：
    /// ```toml
    /// [[sea-orm-ext-tenant.default_databases]]
    /// url = "postgres://..."
    ///
    /// [[sea-orm-ext-tenant.default_databases]]
    /// url = "postgres://..."
    /// ```
    ///
    /// `#[serde(deserialize_with = "single_or_vec", alias = "default_database")]`
    /// 同时兼容旧的单数 `[sea-orm-ext-tenant.default_database]` 写法（会被解析为单元素列表）。
    /// 注意：TOML 标准不允许同一个 `[table]` 表头重复定义，若需配置多个，请使用 `default_databases` 数组表语法。
    #[serde(default, deserialize_with = "crate::config::single_or_vec", alias = "default_database")]
    pub default_databases: Option<Vec<TenantDatabaseEntryConfig>>,
    /// 忽略租户过滤的表名列表。
    ///
    /// 被列入此列表的表将跳过所有租户过滤（SELECT/INSERT/UPDATE/DELETE），
    /// 适用于全局共享表（如字典表、配置表、系统日志表等）。
    ///
    /// **注意**：被列入此列表的表存在跨租户数据泄漏风险，请谨慎配置。
    ///
    /// # TOML 用法
    ///
    /// ```toml
    /// [sea-orm-ext-tenant]
    /// ignored_tables = ["sys_dict", "sys_config", "sys_log"]
    /// ```
    #[serde(default)]
    pub ignored_tables: Option<Vec<String>>,
    /// 失败重试次数（仅针对连接类错误，默认 0 = 不重试）。
    ///
    /// 设置后，所有通过 `SeaOrmExtConnection` 执行的 SQL 操作在遇到连接类错误时
    /// 会自动重试，采用指数退避策略（100ms, 200ms, 400ms, ...）。
    ///
    /// # TOML 用法
    ///
    /// ```toml
    /// [sea-orm-ext-tenant]
    /// max_retries = 3
    /// ```
    #[serde(default)]
    pub max_retries: Option<u32>,
    /// 是否开启 SQL 日志（完整 SQL + 参数值注入）。
    ///
    /// 设置为 `true` 后，所有通过 `SeaOrmExtConnection` 执行的 SQL 语句都会被
    /// 拦截并打印完整 SQL（含参数值注入）和独立参数列表，方便调试。
    ///
    /// **与 `[sea-orm-ext] enable_sql_log` 等效**，两者控制同一个全局开关。
    /// 可以在任意一个配置块中设置，推荐在 `[sea-orm-ext-tenant]` 中统一配置。
    ///
    /// # TOML 用法
    ///
    /// ```toml
    /// [sea-orm-ext-tenant]
    /// enable_sql_log = true
    /// ```
    #[serde(default)]
    pub enable_sql_log: bool,
    /// 租户 ID 的 HTTP Header 名称（可选）
    ///
    /// 设置后，`TenantLayer` 中间件会从请求 header 中提取租户 ID。
    /// 优先级：Header > TenantIdProvider > default_tenant_id
    ///
    /// # TOML 用法
    ///
    /// ```toml
    /// [sea-orm-ext-tenant]
    /// tenant_id_header = "X-Tenant-Id"
    /// ```
    #[serde(default)]
    pub tenant_id_header: Option<String>,
    #[serde(skip)]
    pub tenant_id_provider: Option<Arc<dyn TenantIdProvider>>,
    #[serde(skip)]
    pub tenant_database_provider: Option<Arc<dyn TenantDatabaseProvider>>,
}

impl std::fmt::Debug for TenantPluginConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TenantPluginConfig")
            .field("enabled", &self.enabled)
            .field("mode", &self.mode)
            .field("database_source", &self.database_source)
            .field("default_tenant_id", &self.default_tenant_id)
            .field("databases", &self.databases)
            .field("default_databases", &self.default_databases)
            .field("ignored_tables", &self.ignored_tables)
            .field("max_retries", &self.max_retries)
            .field("enable_sql_log", &self.enable_sql_log)
            .field("tenant_id_header", &self.tenant_id_header)
            .field("tenant_id_provider", &self.tenant_id_provider.as_ref().map(|_| "..."))
            .field("tenant_database_provider", &self.tenant_database_provider.as_ref().map(|_| "..."))
            .finish()
    }
}

impl Default for TenantPluginConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: "table".to_string(),
            database_source: Some("config".to_string()),
            default_tenant_id: None,
            databases: None,
            default_databases: None,
            ignored_tables: None,
            max_retries: None,
            enable_sql_log: false,
            tenant_id_header: None,
            tenant_id_provider: None,
            tenant_database_provider: None,
        }
    }
}

pub struct TenantPlugin;

impl TenantPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TenantPlugin {
    fn default() -> Self {
        Self::new()
    }
}

async fn connect_from_entry(entry: &TenantDatabaseEntry) -> Result<DatabaseConnection, sea_orm::DbErr> {
    connect_from_entry_config(&entry.database).await
}

/// 从 `TenantDatabaseEntryConfig` 创建数据库连接
async fn connect_from_entry_config(entry: &TenantDatabaseEntryConfig) -> Result<DatabaseConnection, sea_orm::DbErr> {
    let mut opt = sea_orm::ConnectOptions::new(&entry.url);
    if let Some(max) = entry.max_connections {
        opt.max_connections(max);
    }
    if let Some(min) = entry.min_connections {
        opt.min_connections(min);
    }
    if let Some(timeout) = entry.connect_timeout_secs {
        opt.connect_timeout(std::time::Duration::from_secs(timeout));
    }
    if let Some(timeout) = entry.acquire_timeout_secs {
        opt.acquire_timeout(std::time::Duration::from_secs(timeout));
    }
    if let Some(timeout) = entry.idle_timeout_secs {
        opt.idle_timeout(std::time::Duration::from_secs(timeout));
    }
    opt.sqlx_logging(entry.enable_logging);
    sea_orm::Database::connect(opt).await
}

async fn connect_from_options(tenant_id: &Value, opt: &sea_orm::ConnectOptions) -> Result<DatabaseConnection, sea_orm::DbErr> {
    let mut cloned_opt = opt.clone();
    cloned_opt.sqlx_logging(false);
    let conn = sea_orm::Database::connect(cloned_opt).await;
    if conn.is_ok() {
        tracing::info!("Connected to database for tenant {:?}", tenant_id);
    } else {
        tracing::error!("Failed to connect to database for tenant {:?}: {:?}", tenant_id, conn.as_ref().err());
    }
    conn
}

/// 创建带重试配置的 `SeaOrmExtConnection`
fn build_ext_conn(conn: DatabaseConnection, max_retries: u32) -> SeaOrmExtConnection {
    let ext = SeaOrmExtConnection::new(conn);
    if max_retries > 0 {
        ext.set_max_retries(max_retries);
    }
    ext
}

#[async_trait]
impl Plugin for TenantPlugin {
    async fn build(&self, app: &mut AppBuilder) {
        let config = app.get_config::<TenantPluginConfig>()
            .expect("tenant plugin config load failed");

        if !config.enabled {
            tracing::info!("Tenant plugin is disabled, skipping initialization");
            return;
        }

        let mode = match config.mode.to_lowercase().as_str() {
            "database" => TenantMode::Database,
            "table" => TenantMode::Table,
            _ => {
                tracing::warn!(
                    "Unknown tenant mode '{}', falling back to 'table'",
                    config.mode
                );
                TenantMode::Table
            }
        };

        let default_tenant_id: Option<Value> =
            config.default_tenant_id.map(|id| Value::String(Some(id)));

        let ignored_tables: HashSet<String> = config.ignored_tables
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect();

        set_tenant_config(TenantConfig {
            enabled: true,
            mode,
            default_tenant_id,
            ignored_tables,
        });

        // 注册 tenant_id_header（全局缓存，供 TenantLayer 中间件读取）
        #[cfg(feature = "summer-web")]
        {
            crate::plugin::tenant_layer::set_tenant_header_name(config.tenant_id_header.clone());
            if let Some(h) = &config.tenant_id_header {
                tracing::info!(
                    "TENANT-LAYER: tenant_id_header = '{}' (will extract tenant ID from HTTP header)",
                    h
                );
            }
        }

        // 失败重试次数：从配置读取，默认 0（不重试）
        let max_retries = config.max_retries.unwrap_or(0);
        if max_retries > 0 {
            tracing::info!(
                "Database retry enabled: max_retries = {} (exponential backoff)",
                max_retries
            );
        }

        // SQL 日志开关：与 [sea-orm-ext] enable_sql_log 等效，控制同一个全局开关
        if config.enable_sql_log {
            crate::set_sql_log_enabled(true);
            tracing::info!("[sea-orm-ext-tenant] SQL log enabled (complete SQL with parameters)");
        } else {
            tracing::debug!("[sea-orm-ext-tenant] SQL log disabled (set enable_sql_log = true to enable)");
        }

        if let Some(db) = app.get_component::<DatabaseConnection>() {
            crate::set_default_database(db);
            tracing::info!("Default database connection registered for tenant module");
        }

        // 注册默认数据库 fallback 链
        // 使用 ext 版本存储，保留 SeaOrmExtConnection 包装（支持 SQL 日志和重试配置）
        if let Some(default_dbs) = &config.default_databases {
            let mut ext_connections = Vec::new();
            for entry in default_dbs {
                match connect_from_entry_config(entry).await {
                    Ok(conn) => {
                        tracing::info!(
                            "Connected to default database (fallback chain): {}",
                            entry.url
                        );
                        ext_connections.push(build_ext_conn(conn, max_retries));
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to connect to default database {}: {} (skip, continue with remaining)",
                            entry.url, e
                        );
                    }
                }
            }
            if !ext_connections.is_empty() {
                crate::set_default_databases_ext(ext_connections);
                tracing::info!(
                    "Default database fallback chain registered with {} databases (with SQL log & retry support)",
                    config.default_databases.as_ref().map(|d| d.len()).unwrap_or(0)
                );
            }
        }

        if let Some(provider) = &config.tenant_id_provider {
            app.add_component(TenantIdProviderComponent::new(provider.clone()));
            set_tenant_id_provider(provider.clone());
            tracing::info!("Tenant ID provider registered from plugin config");
        } else if let Some(component) = app.get_component::<TenantIdProviderComponent>() {
            set_tenant_id_provider(component.provider.clone());
            tracing::info!("Tenant ID provider registered from component registry");
        } else {
            tracing::warn!(
                "No TenantIdProvider registered. Tenant ID will only be available via \
                 default_tenant_id config or manual set_tenant_context() calls."
            );
        }

        if mode == TenantMode::Database {
            let store: Arc<dyn ConnectionStore> = Arc::new(HashMapConnectionStore::new());
            let source = config.database_source
                .as_deref()
                .unwrap_or("config")
                .to_lowercase();

            match source.as_str() {
                "config" => {
                    if let Some(databases) = &config.databases {
                        for entry in databases {
                            let tenant_id = &entry.tenant_id;
                            match connect_from_entry(entry).await {
                                Ok(conn) => {
                                    tracing::info!(
                                        "Connected to database for tenant {}: {}",
                                        tenant_id,
                                        entry.database.url
                                    );
                                    let _ = store.insert_ext(
                                        Value::String(Some(tenant_id.clone())),
                                        build_ext_conn(conn, max_retries),
                                    );
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "Failed to connect to database for tenant {}: {}",
                                        tenant_id,
                                        e
                                    );
                                }
                            }
                        }
                    }
                    tracing::info!(
                        "Tenant plugin initialized in Database mode (config source) with {} tenants",
                        config.databases.as_ref().map_or(0, |dbs| dbs.len())
                    );
                }
                "custom" => {
                    let provider = config.tenant_database_provider.as_ref()
                        .cloned()
                        .or_else(|| {
                            app.get_component::<TenantDatabaseProviderComponent>()
                                .map(|c| c.provider.clone())
                        });

                    if let Some(provider) = provider {
                        set_tenant_database_provider(provider.clone());
                        let databases = provider.provide();
                        let mut connected = 0usize;
                        for (tenant_id, opts) in &databases {
                            match connect_from_options(tenant_id, opts).await {
                                Ok(conn) => {
                                    let _ = store.insert_ext(
                                        tenant_id.clone(),
                                        build_ext_conn(conn, max_retries),
                                    );
                                    connected += 1;
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "Failed to connect to database for tenant {:?}: {}",
                                        tenant_id,
                                        e
                                    );
                                }
                            }
                        }
                        tracing::info!(
                            "Tenant plugin initialized in Database mode (custom source) with {}/{} tenants connected",
                            connected,
                            databases.len()
                        );
                    } else {
                        tracing::warn!(
                            "Database source is 'custom' but no TenantDatabaseProvider registered. \
                             Register via config.tenant_database_provider or app.add_component(TenantDatabaseProviderComponent)."
                        );
                    }
                }
                _ => {
                    tracing::warn!(
                        "Unknown database_source '{}', falling back to 'config'",
                        source
                    );
                    if let Some(databases) = &config.databases {
                        for entry in databases {
                            let tenant_id = &entry.tenant_id;
                            match connect_from_entry(entry).await {
                                Ok(conn) => {
                                    let _ = store.insert_ext(
                                        Value::String(Some(tenant_id.clone())),
                                        build_ext_conn(conn, max_retries),
                                    );
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "Failed to connect to database for tenant {}: {}",
                                        tenant_id,
                                        e
                                    );
                                }
                            }
                        }
                    }
                }
            }

            set_tenant_store(store);
        } else {
            tracing::info!("Tenant plugin initialized in Table mode");
        }

        #[cfg(feature = "summer-web")]
        {
            use summer_web::{RouterLayer, RouterLayers};
            let layer_fn: RouterLayer = std::sync::Arc::new(
                |router| router.layer(crate::plugin::tenant_layer::TenantLayer)
            );
            let insert_pos = if app.get_component::<SaTokenLayerMarker>().is_some() {
                let layers = app.get_component_ref::<RouterLayers>();
                let len = layers.as_ref().map_or(0, |l| l.len());
                if len > 0 {
                    tracing::info!(
                        "SaTokenLayer detected, TenantLayer will be placed after it (inner layer, lower priority)"
                    );
                    len - 1
                } else {
                    0
                }
            } else {
                tracing::info!(
                    "No SaTokenLayer detected, TenantLayer registered as innermost layer (lowest priority)"
                );
                0
            };
            if let Some(layers) = app.get_component_ref::<RouterLayers>() {
                unsafe {
                    let raw_ptr = layers.into_raw();
                    let layers = &mut *(raw_ptr as *mut RouterLayers);
                    layers.insert(insert_pos, layer_fn);
                }
            } else {
                app.add_component(vec![layer_fn] as RouterLayers);
            }
        }
    }

    fn name(&self) -> &'static str {
        "sea-orm-ext-tenant"
    }

    /// 依赖 `SeaOrmPlugin`：本插件在 build 时会读取主库 `DatabaseConnection` 组件
    /// 用作默认数据库 fallback。Summer 框架按依赖拓扑排序构建插件，未声明依赖时
    /// 构建顺序由 DashMap 哈希决定，无法保证 SeaOrmPlugin 先构建。
    fn dependencies(&self) -> Vec<&str> {
        vec!["sea-orm-ext"]
    }
}

#[cfg(feature = "summer-web")]
pub mod tenant_layer {
    use super::*;
    use crate::{get_tenant_id_codec, get_tenant_id_provider, SeaOrmExtConnection, TenantContext};
    use summer_web::axum;
    use summer_web::axum::http::Request;
    use summer_web::axum::response::{IntoResponse, Response};
    use std::task::{Context, Poll};
    use tower::Layer;
    use tower::Service;

    #[derive(Clone)]
    pub struct TenantLayer;

    impl<S> Layer<S> for TenantLayer {
        type Service = TenantMiddleware<S>;

        fn layer(&self, inner: S) -> Self::Service {
            TenantMiddleware { inner }
        }
    }

    #[derive(Clone)]
    pub struct TenantMiddleware<S> {
        inner: S,
    }

    impl<S, B> Service<Request<B>> for TenantMiddleware<S>
    where
        S: Service<Request<B>, Response = Response> + Send + Clone + 'static,
        S::Future: Send + 'static,
        S::Error: Send + 'static,
        B: Send + 'static,
    {
        type Response = <S as Service<Request<B>>>::Response;
        type Error = <S as Service<Request<B>>>::Error;
        type Future = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

        fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            self.inner.poll_ready(cx)
        }

        fn call(&mut self, mut req: Request<B>) -> Self::Future {
            if is_tenant_enabled() {
                // 1. 优先从 HTTP Header 提取（若配置了 tenant_id_header）
                let header_tenant_id = extract_tenant_id_from_header(&req);

                // 2. Header > TenantIdProvider > default_tenant_id
                let raw_tenant_id = header_tenant_id
                    .or_else(|| {
                        get_tenant_id_provider().and_then(|p| p.get_tenant_id())
                    })
                    .or_else(|| {
                        crate::get_tenant_config().and_then(|c| c.default_tenant_id.clone())
                    });

                // 3. 若拿到 tenant_id，进行解密（若注册了 codec），然后设置上下文
                if let Some(tenant_id) = raw_tenant_id {
                    let resolved: Option<Value> = match &tenant_id {
                        Value::String(Some(s)) => {
                            // 字符串类型才需要解密；其他类型直接使用
                            if let Some(codec) = get_tenant_id_codec() {
                                match codec.decrypt(s) {
                                    Ok(decrypted) => Some(decrypted),
                                    Err(e) => {
                                        tracing::warn!(
                                            "TENANT-LAYER: failed to decrypt tenant_id from header: {} (skip setting tenant context)",
                                            e
                                        );
                                        // 跳过租户上下文设置，使用默认库
                                        None
                                    }
                                }
                            } else {
                                Some(tenant_id.clone())
                            }
                        }
                        _ => Some(tenant_id.clone()),
                    };

                    if let Some(final_id) = resolved {
                        set_tenant_context(TenantContext { tenant_id: final_id });

                        if get_tenant_mode() == Some(TenantMode::Database) {
                            if let Ok(Some(db)) = get_tenant_database() {
                                req.extensions_mut().insert::<DatabaseConnection>(db);
                            }
                        }
                    }
                }
            }

            let inner = self.inner.clone();
            let mut inner = std::mem::replace(&mut self.inner, inner);

            Box::pin(async move {
                let response = inner.call(req).await;
                clear_tenant_context();
                response
            })
        }
    }

    /// 从 HTTP header 提取租户 ID（若配置了 `tenant_id_header`）
    ///
    /// 优先级：Header > TenantIdProvider > default_tenant_id
    fn extract_tenant_id_from_header<B>(req: &Request<B>) -> Option<Value> {
        let header_name = get_tenant_header_name()?;

        let name = axum::http::HeaderName::from_bytes(header_name.as_bytes()).ok()?;
        req.headers().get(&name).and_then(|v| {
            v.to_str().ok().map(|s| Value::String(Some(s.to_owned())))
        })
    }

    /// 全局缓存的 tenant_id_header 名称（由 TenantPlugin::build 设置）
    static TENANT_HEADER_NAME: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

    /// 设置全局 tenant_id_header 名称（由 TenantPlugin::build 调用）
    pub fn set_tenant_header_name(name: Option<String>) {
        let _ = TENANT_HEADER_NAME.set(name);
    }

    /// 获取全局 tenant_id_header 名称
    fn get_tenant_header_name() -> Option<String> {
        TENANT_HEADER_NAME.get().cloned().flatten()
    }

    pub struct TenantDb(pub SeaOrmExtConnection);

    impl TenantDb {
        pub fn into_inner(self) -> DatabaseConnection {
            self.0.into_inner()
        }

        pub fn inner(&self) -> &DatabaseConnection {
            self.0.inner()
        }

        pub fn as_ext(&self) -> &SeaOrmExtConnection {
            &self.0
        }
    }

    impl<B> axum::extract::FromRequestParts<B> for TenantDb
    where
        B: Send + Sync,
    {
        type Rejection = axum::response::Response;

        async fn from_request_parts(
            parts: &mut axum::http::request::Parts,
            _state: &B,
        ) -> Result<Self, Self::Rejection> {
            if get_tenant_mode() == Some(TenantMode::Database) {
                if let Some(db) = parts.extensions.get::<DatabaseConnection>() {
                    return Ok(TenantDb(SeaOrmExtConnection::new(db.clone())));
                }

                if let Ok(Some(db)) = get_tenant_database() {
                    return Ok(TenantDb(SeaOrmExtConnection::new(db)));
                }

                if let Some(store) = crate::get_tenant_store() {
                    if let Some(tenant_id) = crate::get_current_tenant_id() {
                        if let Some(ext_conn) = store.get_ext(&tenant_id) {
                            return Ok(TenantDb(ext_conn));
                        }
                    }
                }
            }

            let app = parts.extensions.get::<summer_web::AppState>();
            if let Some(app_state) = app {
                if let Some(ext_conn) = app_state.app.get_component::<SeaOrmExtConnection>() {
                    return Ok(TenantDb(ext_conn));
                }
                if let Some(db) = app_state.app.get_component::<DatabaseConnection>() {
                    return Ok(TenantDb(SeaOrmExtConnection::new(db)));
                }
            }

            Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}
