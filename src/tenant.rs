use sea_orm::{ColumnTrait, ConnectOptions, DatabaseConnection, DbErr, EntityTrait, QueryFilter};
use sea_query::Value;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock, RwLock};

use crate::sea_orm_ext_connection::SeaOrmExtConnection;

#[cfg(feature = "runtime-tokio")]
use std::future::Future;

use crate::tenant_store::ConnectionStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TenantMode {
    #[default]
    Table,
    Database,
}

#[derive(Debug, Clone)]
pub struct TenantConfig {
    pub enabled: bool,
    pub mode: TenantMode,
    pub default_tenant_id: Option<Value>,
    pub ignored_tables: HashSet<String>,
}

impl Default for TenantConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: TenantMode::Table,
            default_tenant_id: None,
            ignored_tables: HashSet::new(),
        }
    }
}

type SharedTenantConfig = Arc<RwLock<Option<Arc<TenantConfig>>>>;

static TENANT_CONFIG: OnceLock<SharedTenantConfig> = OnceLock::new();

fn tenant_config_store() -> &'static SharedTenantConfig {
    TENANT_CONFIG.get_or_init(|| Arc::new(RwLock::new(None)))
}

pub fn set_tenant_config(config: TenantConfig) {
    let store = tenant_config_store();
    match store.write() {
        Ok(mut guard) => *guard = Some(Arc::new(config)),
        Err(_) => tracing::error!("TENANT-CONFIG: lock poisoned, set_tenant_config ignored"),
    }
}

type SharedTenantIdProvider = Arc<RwLock<Option<Arc<dyn TenantIdProvider>>>>;

static TENANT_ID_PROVIDER: OnceLock<SharedTenantIdProvider> = OnceLock::new();

fn tenant_id_provider_store() -> &'static SharedTenantIdProvider {
    TENANT_ID_PROVIDER.get_or_init(|| Arc::new(RwLock::new(None)))
}

pub fn set_tenant_id_provider(provider: Arc<dyn TenantIdProvider>) {
    let store = tenant_id_provider_store();
    match store.write() {
        Ok(mut guard) => *guard = Some(provider),
        Err(_) => tracing::error!("TENANT-ID-PROVIDER: lock poisoned, set_tenant_id_provider ignored"),
    }
}

pub fn get_tenant_id_provider() -> Option<Arc<dyn TenantIdProvider>> {
    let store = tenant_id_provider_store();
    match store.read() {
        Ok(guard) => guard.clone(),
        Err(_) => {
            tracing::error!("TENANT-ID-PROVIDER: lock poisoned, get_tenant_id_provider returning None");
            None
        }
    }
}

type SharedTenantDatabaseProvider = Arc<RwLock<Option<Arc<dyn TenantDatabaseProvider>>>>;

static TENANT_DATABASE_PROVIDER: OnceLock<SharedTenantDatabaseProvider> = OnceLock::new();

fn tenant_database_provider_store() -> &'static SharedTenantDatabaseProvider {
    TENANT_DATABASE_PROVIDER.get_or_init(|| Arc::new(RwLock::new(None)))
}

pub fn set_tenant_database_provider(provider: Arc<dyn TenantDatabaseProvider>) {
    let store = tenant_database_provider_store();
    match store.write() {
        Ok(mut guard) => *guard = Some(provider),
        Err(_) => tracing::error!("TENANT-DATABASE-PROVIDER: lock poisoned, set_tenant_database_provider ignored"),
    }
}

pub fn get_tenant_database_provider() -> Option<Arc<dyn TenantDatabaseProvider>> {
    let store = tenant_database_provider_store();
    match store.read() {
        Ok(guard) => guard.clone(),
        Err(_) => {
            tracing::error!("TENANT-DATABASE-PROVIDER: lock poisoned, get_tenant_database_provider returning None");
            None
        }
    }
}

pub trait TenantIdProvider: Send + Sync + 'static {
    fn get_tenant_id(&self) -> Option<Value>;

    /// 运行时租户模式（"table" / "database"）。
    ///
    /// 用于在全局 `TenantConfig::mode` 之外，按当前请求/令牌动态决定租户隔离模式。
    /// 例如：全局配置为 `table`，但 JWT token 标识当前用户属于 `database` 模式租户时，
    /// `is_tenant_enforced()` 应跳过 `WHERE tenant_id = ?` 注入，
    /// `SeaOrmExtConnection` 应自动路由到租户专属数据库。
    ///
    /// 默认返回 `None`，表示沿用全局配置，保持已有实现兼容。
    fn get_tenant_mode(&self) -> Option<String> {
        None
    }
}

pub trait TenantDatabaseProvider: Send + Sync + 'static {
    fn provide(&self) -> HashMap<Value, ConnectOptions>;
}

/// 租户 ID 加解密器
///
/// 当应用需要在前端暴露加密后的租户 ID 时，实现此 trait 并通过
/// [`set_tenant_id_codec`] 注册。`TenantLayer` 中间件在获取到原始 tenant_id 后，
/// 会检查是否注册了此 trait：
/// - **已注册**：先调用 `decrypt()` 解密，再设置租户上下文
/// - **未注册**：直接使用原始 tenant_id，不做加解密
///
/// 典型场景：JWT token 中的 tenant_id 加密 / URL 路径参数中的加密 tenant_id
///
/// # 示例
///
/// ```ignore
/// use sea_orm_ext::{TenantIdCodec, set_tenant_id_codec};
/// use sea_orm::DbErr;
/// use sea_query::Value;
/// use std::sync::Arc;
///
/// struct MyCodec;
///
/// impl TenantIdCodec for MyCodec {
///     fn decrypt(&self, encrypted: &str) -> Result<Value, DbErr> {
///         // 解密逻辑
///         Ok(Value::String(Some(encrypted.to_string())))
///     }
///
///     fn encrypt(&self, tenant_id: &Value) -> Result<String, DbErr> {
///         // 加密逻辑
///         match tenant_id {
///             Value::String(Some(s)) => Ok(s.clone()),
///             _ => Err(DbErr::Custom("unsupported".into())),
///         }
///     }
/// }
///
/// set_tenant_id_codec(Arc::new(MyCodec));
/// ```
pub trait TenantIdCodec: Send + Sync + 'static {
    /// 解密：将前端传入的密文转为内部 tenant_id
    ///
    /// `TenantLayer` 中间件在获取到原始 tenant_id 后调用此方法。
    /// 解密失败返回 `Err`，中间件会记录日志并跳过该请求的租户上下文设置。
    fn decrypt(&self, encrypted: &str) -> Result<Value, DbErr>;

    /// 加密：将内部 tenant_id 转为前端可见的密文
    ///
    /// 用于生成返回给前端的 tenant_id
    fn encrypt(&self, tenant_id: &Value) -> Result<String, DbErr>;
}

// ============================================================================
// TenantIdCodec 全局注册
// ============================================================================

type SharedTenantIdCodec = Arc<RwLock<Option<Arc<dyn TenantIdCodec>>>>;

static TENANT_ID_CODEC: OnceLock<SharedTenantIdCodec> = OnceLock::new();

fn tenant_id_codec_store() -> &'static SharedTenantIdCodec {
    TENANT_ID_CODEC.get_or_init(|| Arc::new(RwLock::new(None)))
}

/// 注册租户 ID 加解密器
///
/// 注册后，`TenantLayer` 中间件会自动对 tenant_id 进行解密。
/// 详见 [`TenantIdCodec`] trait 文档。
pub fn set_tenant_id_codec(codec: Arc<dyn TenantIdCodec>) {
    let store = tenant_id_codec_store();
    match store.write() {
        Ok(mut guard) => *guard = Some(codec),
        Err(_) => tracing::error!("TENANT-CODEC: lock poisoned, set_tenant_id_codec ignored"),
    }
}

/// 获取已注册的租户 ID 加解密器
pub fn get_tenant_id_codec() -> Option<Arc<dyn TenantIdCodec>> {
    let store = tenant_id_codec_store();
    match store.read() {
        Ok(guard) => guard.clone(),
        Err(_) => {
            tracing::error!("TENANT-CODEC: lock poisoned, get_tenant_id_codec returning None");
            None
        }
    }
}

/// 清除已注册的租户 ID 加解密器
pub fn clear_tenant_id_codec() {
    let store = tenant_id_codec_store();
    match store.write() {
        Ok(mut guard) => *guard = None,
        Err(_) => tracing::error!("TENANT-CODEC: lock poisoned, clear_tenant_id_codec ignored"),
    }
}

pub fn clear_tenant_config() {
    let store = tenant_config_store();
    match store.write() {
        Ok(mut guard) => *guard = None,
        Err(_) => tracing::error!("TENANT-CONFIG: lock poisoned, clear_tenant_config ignored"),
    }
}

pub fn get_tenant_config() -> Option<Arc<TenantConfig>> {
    let store = tenant_config_store();
    match store.read() {
        Ok(guard) => guard.clone(),
        Err(_) => {
            tracing::error!("TENANT-CONFIG: lock poisoned, get_tenant_config returning None");
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct TenantContext {
    pub tenant_id: Value,
}

#[cfg(feature = "runtime-tokio")]
tokio::task_local! {
    static TASK_TENANT_CONTEXT: RefCell<Option<TenantContext>>;
}

thread_local! {
    static THREAD_TENANT_CONTEXT: RefCell<Option<TenantContext>> = const { RefCell::new(None) };
}

#[cfg(feature = "runtime-tokio")]
fn set_tenant_context_inner(ctx: TenantContext) {
    if let Ok(cell) = TASK_TENANT_CONTEXT.try_with(|c| c.clone()) {
        *cell.borrow_mut() = Some(ctx);
    } else {
        THREAD_TENANT_CONTEXT.with(|cell| {
            *cell.borrow_mut() = Some(ctx);
        });
    }
}

#[cfg(not(feature = "runtime-tokio"))]
fn set_tenant_context_inner(ctx: TenantContext) {
    THREAD_TENANT_CONTEXT.with(|cell| {
        *cell.borrow_mut() = Some(ctx);
    });
}

#[cfg(feature = "runtime-tokio")]
fn clear_tenant_context_inner() {
    if let Ok(cell) = TASK_TENANT_CONTEXT.try_with(|c| c.clone()) {
        *cell.borrow_mut() = None;
    } else {
        THREAD_TENANT_CONTEXT.with(|cell| {
            *cell.borrow_mut() = None;
        });
    }
}

#[cfg(not(feature = "runtime-tokio"))]
fn clear_tenant_context_inner() {
    THREAD_TENANT_CONTEXT.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

#[cfg(feature = "runtime-tokio")]
fn get_tenant_context_inner() -> Option<TenantContext> {
    if let Ok(cell) = TASK_TENANT_CONTEXT.try_with(|c| c.clone()) {
        cell.borrow().clone()
    } else {
        THREAD_TENANT_CONTEXT.with(|cell| {
            cell.borrow().clone()
        })
    }
}

#[cfg(not(feature = "runtime-tokio"))]
fn get_tenant_context_inner() -> Option<TenantContext> {
    THREAD_TENANT_CONTEXT.with(|cell| {
        cell.borrow().clone()
    })
}

pub fn set_tenant_context(ctx: TenantContext) {
    set_tenant_context_inner(ctx);
}

pub fn clear_tenant_context() {
    clear_tenant_context_inner();
}

pub fn get_tenant_context() -> Option<TenantContext> {
    get_tenant_context_inner()
}

#[cfg(feature = "runtime-tokio")]
pub async fn tenant_scope<F>(tenant_id: Value, f: F) -> F::Output
where
    F: Future,
{
    let ctx = TenantContext { tenant_id };
    TASK_TENANT_CONTEXT
        .scope(RefCell::new(Some(ctx)), f)
        .await
}

pub fn is_tenant_enabled() -> bool {
    get_tenant_config().map(|c| c.enabled).unwrap_or(false)
}

pub fn get_tenant_mode() -> Option<TenantMode> {
    get_tenant_config().map(|c| c.mode)
}

pub fn get_current_tenant_id() -> Option<Value> {
    if let Some(ctx) = get_tenant_context() {
        return Some(ctx.tenant_id);
    }

    if let Some(provider) = get_tenant_id_provider() {
        if let Some(tenant_id) = provider.get_tenant_id() {
            return Some(tenant_id);
        }
    }

    get_tenant_config().and_then(|c| c.default_tenant_id.clone())
}

pub fn is_tenant_enforced() -> bool {
    if !is_tenant_enabled() || is_tenant_filter_disabled() {
        return false;
    }

    // 优先读运行时 mode（来自 TenantIdProvider，如 JWT token 的 tenantMode 字段）。
    // database 模式租户的查询不注入 WHERE tenant_id = ?（租户库已物理隔离）。
    get_effective_tenant_mode() == Some(TenantMode::Table)
}

/// 获取当前生效的租户模式。
///
/// 优先级：运行时 provider（如 JWT token 的 tenantMode） > 全局配置。
/// 用于业务层判断当前请求应该走 table 还是 database 隔离，
/// 以及 `SeaOrmExtConnection` 的自动数据库路由。
///
/// # 回退策略
///
/// - provider 未实现 `get_tenant_mode()`（返回 `None`）：回退全局配置
/// - provider 返回 `Some("invalid")`（无效字符串）：记录警告并回退全局配置
/// - provider 返回 `Some("database")` / `Some("table")`：使用 provider 的值
pub fn get_effective_tenant_mode() -> Option<TenantMode> {
    if let Some(provider) = get_tenant_id_provider() {
        if let Some(mode) = provider.get_tenant_mode() {
            match mode.to_lowercase().as_str() {
                "database" => return Some(TenantMode::Database),
                "table" => return Some(TenantMode::Table),
                _ => {
                    tracing::warn!(
                        "TENANT-MODE: invalid provider mode '{}', falling back to global config",
                        mode
                    );
                    // 无效字符串不直接返回 None，继续回退到全局配置
                }
            }
        }
    }
    get_tenant_mode()
}

thread_local! {
    static TENANT_FILTER_DISABLED_DEPTH: Cell<u32> = const { Cell::new(0) };
}

pub(crate) fn is_tenant_filter_disabled() -> bool {
    TENANT_FILTER_DISABLED_DEPTH.with(|f| f.get() > 0)
}

pub struct TenantIgnoreGuard {
    _private: (),
}

impl Default for TenantIgnoreGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl TenantIgnoreGuard {
    pub fn new() -> Self {
        TENANT_FILTER_DISABLED_DEPTH.with(|f| f.set(f.get().saturating_add(1)));
        Self { _private: () }
    }
}

impl Drop for TenantIgnoreGuard {
    fn drop(&mut self) {
        TENANT_FILTER_DISABLED_DEPTH.with(|f| f.set(f.get().saturating_sub(1)));
    }
}

pub fn is_table_tenant_ignored(table_name: &str) -> bool {
    get_tenant_config()
        .map(|c| c.ignored_tables.contains(table_name))
        .unwrap_or(false)
}

pub fn try_get_tenant_id() -> Result<Value, DbErr> {
    get_current_tenant_id().ok_or_else(|| {
        DbErr::Custom(
            "TABLE-ISOLATION: tenant ID is required but unavailable. \
             Ensure tenant context has been set via set_tenant_context() or TenantGuard::set()."
                .to_owned(),
        )
    })
}

/// 获取当前租户 ID，若未设置则返回 `Value::Int(None)`（SQL NULL）并记录错误（**不会 panic**）。
///
/// 历史版本此函数会 panic，导致整个进程崩溃。现在改为 panic-free：
/// - 若未设置 tenant context，记录 `tracing::error` 并返回 `Value::Int(None)`（表示 SQL NULL）
/// - SQL `WHERE tenant_id = NULL` 永远为 false，查询返回空集（安全失败）
/// - 调用方应在调用前主动检查 `get_current_tenant_id()` 或使用 `try_get_tenant_id()?`
pub fn require_tenant_id() -> Value {
    match get_current_tenant_id() {
        Some(id) => id,
        None => {
            tracing::error!(
                "TABLE-ISOLATION: tenant ID is required but unavailable. \
                 Returning Value::Int(None) (SQL NULL, will match no rows). \
                 Ensure tenant context has been set via set_tenant_context() or TenantGuard::set()."
            );
            // sea-query 1.0 移除了 Value::Null 变体；使用 Value::Int(None) 表示 SQL NULL。
            // SQL 中 `WHERE col = NULL` 永远返回 false，保证安全失败。
            Value::Int(None)
        }
    }
}

pub trait TenantEntity: EntityTrait {
    type TenantColumn: ColumnTrait;
    fn tenant_column() -> Self::TenantColumn;
}

pub trait TenantSelectExt: Sized {
    fn apply_tenant_filter(self) -> Self;
}

impl<E> TenantSelectExt for sea_orm::Select<E>
where
    E: TenantEntity,
{
    fn apply_tenant_filter(self) -> Self {
        if !is_tenant_enforced() {
            return self;
        }
        let table = E::default().table_name();
        if is_table_tenant_ignored(table) {
            return self;
        }
        let tenant_id = require_tenant_id();
        self.filter(E::tenant_column().eq(tenant_id))
    }
}

pub struct TenantGuard;

impl TenantGuard {
    pub fn set(tenant_id: Value) -> Self {
        set_tenant_context(TenantContext { tenant_id });
        Self
    }
}

impl Drop for TenantGuard {
    fn drop(&mut self) {
        clear_tenant_context();
    }
}

type SharedTenantStore = Arc<RwLock<Option<Arc<dyn ConnectionStore>>>>;

static TENANT_STORE: OnceLock<SharedTenantStore> = OnceLock::new();

fn tenant_store_inner() -> &'static SharedTenantStore {
    TENANT_STORE.get_or_init(|| Arc::new(RwLock::new(None)))
}

pub fn set_tenant_store(store: Arc<dyn ConnectionStore>) {
    let inner = tenant_store_inner();
    match inner.write() {
        Ok(mut guard) => *guard = Some(store),
        Err(_) => tracing::error!("TENANT-STORE: lock poisoned, set_tenant_store ignored"),
    }
}

pub fn clear_tenant_store() {
    let inner = tenant_store_inner();
    match inner.write() {
        Ok(mut guard) => *guard = None,
        Err(_) => tracing::error!("TENANT-STORE: lock poisoned, clear_tenant_store ignored"),
    }
}

pub fn get_tenant_store() -> Option<Arc<dyn ConnectionStore>> {
    let inner = tenant_store_inner();
    match inner.read() {
        Ok(guard) => guard.clone(),
        Err(_) => {
            tracing::error!("TENANT-STORE: lock poisoned, get_tenant_store returning None");
            None
        }
    }
}

type SharedDefaultDb = Arc<RwLock<Option<DatabaseConnection>>>;

static DEFAULT_DB: OnceLock<SharedDefaultDb> = OnceLock::new();

fn default_db_inner() -> &'static SharedDefaultDb {
    DEFAULT_DB.get_or_init(|| Arc::new(RwLock::new(None)))
}

pub fn set_default_database(db: DatabaseConnection) {
    let inner = default_db_inner();
    match inner.write() {
        Ok(mut guard) => *guard = Some(db),
        Err(_) => tracing::error!("DEFAULT-DB: lock poisoned, set_default_database ignored"),
    }
}

pub fn get_default_database() -> Option<DatabaseConnection> {
    let inner = default_db_inner();
    match inner.read() {
        Ok(guard) => guard.clone(),
        Err(_) => {
            tracing::error!("DEFAULT-DB: lock poisoned, get_default_database returning None");
            None
        }
    }
}

// ============================================================================
// 默认数据库 fallback 链
// ============================================================================

/// 默认数据库 fallback 链：主库故障时按顺序尝试 fallback 库
///
/// 内部存储 `SeaOrmExtConnection` 以支持 SQL 日志拦截和重试配置。
/// 公开 API 返回 `DatabaseConnection` 以保持向后兼容。
type SharedDefaultDbs = Arc<RwLock<Vec<SeaOrmExtConnection>>>;

static DEFAULT_DBS: OnceLock<SharedDefaultDbs> = OnceLock::new();

fn default_dbs_inner() -> &'static SharedDefaultDbs {
    DEFAULT_DBS.get_or_init(|| Arc::new(RwLock::new(Vec::new())))
}

/// 设置默认数据库 fallback 链（按顺序，第一个为主库，后续为 fallback）
///
/// 传入的 `DatabaseConnection` 会被自动包装为 `SeaOrmExtConnection`，
/// 以支持 SQL 日志拦截和重试配置。
pub fn set_default_databases(dbs: Vec<DatabaseConnection>) {
    let ext_dbs: Vec<SeaOrmExtConnection> = dbs
        .into_iter()
        .map(SeaOrmExtConnection::new)
        .collect();
    set_default_databases_ext(ext_dbs);
}

/// 设置默认数据库 fallback 链（ext 版本，直接传入 `SeaOrmExtConnection`）
///
/// 与 [`set_default_databases`] 不同，此函数直接存储 `SeaOrmExtConnection`，
/// 保留原有的 `max_retries` 等 ext 配置。
pub fn set_default_databases_ext(dbs: Vec<SeaOrmExtConnection>) {
    let inner = default_dbs_inner();
    match inner.write() {
        Ok(mut guard) => *guard = dbs,
        Err(_) => tracing::error!("DEFAULT-DBS: lock poisoned, set_default_databases_ext ignored"),
    }
}

/// 获取默认数据库 fallback 链
pub fn get_default_databases() -> Vec<DatabaseConnection> {
    let inner = default_dbs_inner();
    match inner.read() {
        Ok(guard) => guard.iter().map(|ext| ext.inner().clone()).collect(),
        Err(_) => {
            tracing::error!("DEFAULT-DBS: lock poisoned, get_default_databases returning empty vec");
            Vec::new()
        }
    }
}

/// 获取默认数据库 fallback 链（ext 版本，返回 `SeaOrmExtConnection`）
///
/// 与 [`get_default_databases`] 不同，此函数返回 `SeaOrmExtConnection`，
/// 支持 SQL 日志拦截和重试配置。
pub fn get_default_databases_ext() -> Vec<SeaOrmExtConnection> {
    let inner = default_dbs_inner();
    match inner.read() {
        Ok(guard) => guard.clone(),
        Err(_) => {
            tracing::error!("DEFAULT-DBS: lock poisoned, get_default_databases_ext returning empty vec");
            Vec::new()
        }
    }
}

/// 从 fallback 链中获取第一个可用的数据库连接（同步版本，不进行健康检查）。
///
/// - 优先返回主库（链中第一个）
/// - 若 fallback 链为空，回退到单库模式
///
/// **注意**：sea-orm 2.0 移除了 `is_closed()` 方法，此同步版本无法进行健康检查。
/// 若需要健康检查，请使用 [`get_available_default_database_async`]。
/// 失败的连接会由 `SeaOrmExtConnection` 的重试机制处理。
pub fn get_available_default_database() -> Option<DatabaseConnection> {
    let dbs = get_default_databases_ext();
    if let Some(ext) = dbs.first() {
        return Some(ext.inner().clone());
    }
    // fallback 链为空，回退到单库模式
    get_default_database()
}

/// 从 fallback 链中获取第一个可用的数据库连接（ext 版本，返回 `SeaOrmExtConnection`）。
///
/// 与 [`get_available_default_database`] 不同，此函数返回 `SeaOrmExtConnection`，
/// 支持 SQL 日志拦截和重试配置。推荐在需要 SQL 日志的场景使用。
pub fn get_available_default_database_ext() -> Option<SeaOrmExtConnection> {
    let dbs = get_default_databases_ext();
    if let Some(ext) = dbs.first() {
        return Some(ext.clone());
    }
    // fallback 链为空，回退到单库模式
    // 注意：单库模式存储的是 DatabaseConnection，需要包装为 ext
    get_default_database().map(SeaOrmExtConnection::new)
}

/// 从 fallback 链中获取第一个可用的数据库连接（异步版本，带健康检查）。
///
/// 通过 `ping()` 探活，依次尝试 fallback 链中的每个数据库，返回第一个可用的。
#[cfg(feature = "runtime-tokio")]
pub async fn get_available_default_database_async() -> Option<DatabaseConnection> {
    let dbs = get_default_databases_ext();
    for ext in &dbs {
        match ext.ping().await {
            Ok(()) => return Some(ext.inner().clone()),
            Err(e) => {
                tracing::warn!(
                    "DATABASE-FAILOVER: database ping failed, trying next in fallback chain: {}",
                    e
                );
            }
        }
    }
    // fallback 链全部不可用或链为空，回退到单库模式
    get_default_database()
}

#[cfg(feature = "runtime-tokio")]
async fn connect_and_cache(
    store: &Arc<dyn ConnectionStore>,
    tenant_id: &Value,
    opts: &ConnectOptions,
) -> Result<DatabaseConnection, DbErr> {
    let mut cloned_opts = opts.clone();
    cloned_opts.sqlx_logging(false);
    let conn = sea_orm::Database::connect(cloned_opts).await?;
    let ext_conn = SeaOrmExtConnection::new(conn);
    let result = ext_conn.inner().clone();
    store.insert_ext(tenant_id.clone(), ext_conn)?;
    Ok(result)
}

pub fn get_tenant_database() -> Result<Option<DatabaseConnection>, DbErr> {
    if !is_tenant_enabled() {
        return Ok(get_available_default_database());
    }

    if get_tenant_mode() != Some(TenantMode::Database) {
        return Ok(get_available_default_database());
    }

    let Some(tenant_id) = get_current_tenant_id() else {
        return Ok(get_available_default_database());
    };

    get_database_for_tenant(&tenant_id)
}

pub fn get_database_for_tenant(tenant_id: &Value) -> Result<Option<DatabaseConnection>, DbErr> {
    if !is_tenant_enabled() {
        return Ok(get_available_default_database());
    }

    if get_tenant_mode() != Some(TenantMode::Database) {
        return Ok(get_available_default_database());
    }

    let Some(store) = get_tenant_store() else {
        return Err(DbErr::Custom(
            "DATABASE-ISOLATION: tenant connection store not initialized.".to_owned(),
        ));
    };

    if let Some(conn) = store.get(tenant_id) {
        // sea-orm 2.0 移除了 is_closed() 同步方法，直接返回连接。
        // 失败的连接会由 SeaOrmExtConnection 的重试机制处理。
        return Ok(Some(conn));
    }

    // 未找到该租户的连接，尝试 fallback 链
    tracing::warn!(
        "DATABASE-FAILOVER: no connection for tenant {:?}, trying fallback chain",
        tenant_id
    );
    Ok(get_available_default_database())
}

/// 获取指定租户的数据库连接（不检查 mode，仅按 tenant_id 查 ConnectionStore）。
///
/// 内部使用，供自动路由调用。mode 检查由调用方（如 `get_tenant_database_for_current`）负责。
///
/// 与 [`get_database_for_tenant`] 的区别：
/// - [`get_database_for_tenant`] 会检查全局 `get_tenant_mode()`，table 模式下直接返回主库
/// - 本函数不检查 mode，仅按 tenant_id 查找连接缓存
///
/// 适用场景：运行时 provider 指定为 database 模式但全局配置为 table 模式时，
/// 仍能正确路由到租户专属库。
pub fn get_database_for_tenant_unchecked(tenant_id: &Value) -> Result<Option<DatabaseConnection>, DbErr> {
    if !is_tenant_enabled() {
        return Ok(None);
    }

    // store 未初始化时返回 None 走 fallback（与 effective_connection 容错策略一致）
    //
    // 适用场景：
    //   - DynamicTenantPlugin 未启用或启动失败
    //   - 全局 table 模式但运行时 provider 指定某租户为 database 模式
    //   - 测试环境未注册 store
    //
    // 与 get_database_for_tenant() 的区别：后者在 store 未初始化时返回 Err，
    // 因为那是显式调用方期望严格模式检查；本函数用于自动路由，应容忍配置缺失。
    let Some(store) = get_tenant_store() else {
        tracing::warn!(
            "DATABASE-FAILOVER: tenant connection store not initialized, \
             falling back to main db for tenant {:?}",
            tenant_id
        );
        return Ok(get_available_default_database());
    };

    if let Some(conn) = store.get(tenant_id) {
        return Ok(Some(conn));
    }

    tracing::warn!(
        "DATABASE-FAILOVER: no connection for tenant {:?}, trying fallback chain",
        tenant_id
    );
    Ok(get_available_default_database())
}

/// 获取当前请求生效的数据库连接（基于运行时 tenant mode 自动路由）。
///
/// 优先级：运行时 provider mode > 全局配置 mode。
///
/// - database 模式 + 当前 tenant_id：返回租户专属 db
/// - table 模式 或 未登录：返回 `None`（用主库 self.inner）
///
/// 供 `SeaOrmExtConnection` 在执行 SQL 前自动调用，业务层无需手动选库。
///
/// # 自动路由逻辑
///
/// ```text
/// ┌─────────────────────────────────────────────────────────────┐
/// │ effective_connection() 调用链                               │
/// ├─────────────────────────────────────────────────────────────┤
/// │ 1. TenantIgnoreGuard 生效？  ──Yes──→  返回主库（None）     │
/// │                                  No                          │
/// │ 2. get_effective_tenant_mode() == Database？                │
/// │    Yes → 查租户连接（unchecked）                            │
/// │    No  → 返回 None（主库）                                  │
/// └─────────────────────────────────────────────────────────────┘
/// ```
pub fn get_tenant_database_for_current() -> Result<Option<DatabaseConnection>, DbErr> {
    if !is_tenant_enabled() {
        return Ok(None);
    }

    // TenantIgnoreGuard 生效时走主库（用于查询全局表）
    if is_tenant_filter_disabled() {
        return Ok(None);
    }

    // 读运行时 mode（provider）> 全局配置 mode
    if get_effective_tenant_mode() != Some(TenantMode::Database) {
        return Ok(None);
    }

    let Some(tenant_id) = get_current_tenant_id() else {
        return Ok(None);
    };

    get_database_for_tenant_unchecked(&tenant_id)
}

#[cfg(feature = "runtime-tokio")]
pub async fn get_database_for_tenant_async(tenant_id: &Value) -> Result<Option<DatabaseConnection>, DbErr> {
    if !is_tenant_enabled() {
        return Ok(get_available_default_database_async().await);
    }

    if get_tenant_mode() != Some(TenantMode::Database) {
        return Ok(get_available_default_database_async().await);
    }

    let Some(store) = get_tenant_store() else {
        return Err(DbErr::Custom(
            "DATABASE-ISOLATION: tenant connection store not initialized.".to_owned(),
        ));
    };

    if let Some(conn) = store.get(tenant_id) {
        // 使用 ping() 异步探活
        match conn.ping().await {
            Ok(()) => return Ok(Some(conn)),
            Err(e) => {
                tracing::warn!(
                    "DATABASE-FAILOVER: tenant {:?} connection ping failed ({}), trying fallback chain",
                    tenant_id, e
                );
                return Ok(get_available_default_database_async().await);
            }
        }
    }

    let provider = get_tenant_database_provider();
    if let Some(provider) = provider {
        let databases = provider.provide();
        if let Some(opts) = databases.get(tenant_id) {
            match connect_and_cache(&store, tenant_id, opts).await {
                Ok(conn) => return Ok(Some(conn)),
                Err(e) => {
                    tracing::warn!(
                        "DATABASE-FAILOVER: failed to connect for tenant {:?}: {}, trying fallback chain",
                        tenant_id, e
                    );
                    return Ok(get_available_default_database_async().await);
                }
            }
        }
    }

    // provider 也无法提供，尝试 fallback 链
    tracing::warn!(
        "DATABASE-FAILOVER: no provider can provide connection for tenant {:?}, trying fallback chain",
        tenant_id
    );
    Ok(get_available_default_database_async().await)
}

#[cfg(feature = "runtime-tokio")]
pub async fn get_tenant_database_async() -> Result<Option<DatabaseConnection>, DbErr> {
    if !is_tenant_enabled() {
        return Ok(get_default_database());
    }

    if get_tenant_mode() != Some(TenantMode::Database) {
        return Ok(get_default_database());
    }

    let Some(tenant_id) = get_current_tenant_id() else {
        return Ok(get_default_database());
    };

    get_database_for_tenant_async(&tenant_id).await
}

pub fn tenant_db(default_db: &DatabaseConnection) -> Result<DatabaseConnection, DbErr> {
    if !is_tenant_enabled() {
        return Ok(default_db.clone());
    }

    match get_tenant_mode() {
        Some(TenantMode::Database) => {
            let tenant_id = get_current_tenant_id().ok_or_else(|| {
                DbErr::Custom(
                    "DATABASE-ISOLATION: tenant ID is required but unavailable. \
                     Ensure tenant context has been set via set_tenant_context() or TenantIdProvider."
                        .to_owned(),
                )
            })?;

            let store = get_tenant_store().ok_or_else(|| {
                DbErr::Custom(
                    "DATABASE-ISOLATION: tenant connection store not initialized."
                        .to_owned(),
                )
            })?;

            if let Some(conn) = store.get(&tenant_id) {
                return Ok(conn);
            }

            Err(DbErr::Custom(format!(
                "DATABASE-ISOLATION: no database connection found for tenant {:?}. \
                 Use tenant_db_async() to auto-initialize from TenantDatabaseProvider",
                tenant_id
            )))
        }
        Some(TenantMode::Table) | None => Ok(default_db.clone()),
    }
}

pub fn tenant_db_for(default_db: &DatabaseConnection, tenant_id: &Value) -> Result<DatabaseConnection, DbErr> {
    if !is_tenant_enabled() {
        return Ok(default_db.clone());
    }

    match get_tenant_mode() {
        Some(TenantMode::Database) => {
            let store = get_tenant_store().ok_or_else(|| {
                DbErr::Custom(
                    "DATABASE-ISOLATION: tenant connection store not initialized."
                        .to_owned(),
                )
            })?;

            if let Some(conn) = store.get(tenant_id) {
                return Ok(conn);
            }

            Err(DbErr::Custom(format!(
                "DATABASE-ISOLATION: no database connection found for tenant {:?}. \
                 Use tenant_db_for_async() to auto-initialize from TenantDatabaseProvider",
                tenant_id
            )))
        }
        Some(TenantMode::Table) | None => Ok(default_db.clone()),
    }
}

#[cfg(feature = "runtime-tokio")]
pub async fn tenant_db_async(default_db: &DatabaseConnection) -> Result<DatabaseConnection, DbErr> {
    if !is_tenant_enabled() {
        return Ok(default_db.clone());
    }

    match get_tenant_mode() {
        Some(TenantMode::Database) => {
            let tenant_id = get_current_tenant_id().ok_or_else(|| {
                DbErr::Custom(
                    "DATABASE-ISOLATION: tenant ID is required but unavailable. \
                     Ensure tenant context has been set via set_tenant_context() or TenantIdProvider."
                        .to_owned(),
                )
            })?;

            let store = get_tenant_store().ok_or_else(|| {
                DbErr::Custom(
                    "DATABASE-ISOLATION: tenant connection store not initialized."
                        .to_owned(),
                )
            })?;

            if let Some(conn) = store.get(&tenant_id) {
                return Ok(conn);
            }

            let provider = get_tenant_database_provider();
            if let Some(provider) = provider {
                let databases = provider.provide();
                if let Some(opts) = databases.get(&tenant_id) {
                    let conn = connect_and_cache(&store, &tenant_id, opts).await?;
                    return Ok(conn);
                }
            }

            Err(DbErr::Custom(format!(
                "DATABASE-ISOLATION: no database connection found for tenant {:?}",
                tenant_id
            )))
        }
        Some(TenantMode::Table) | None => Ok(default_db.clone()),
    }
}

#[cfg(feature = "runtime-tokio")]
pub async fn tenant_db_for_async(default_db: &DatabaseConnection, tenant_id: &Value) -> Result<DatabaseConnection, DbErr> {
    if !is_tenant_enabled() {
        return Ok(default_db.clone());
    }

    match get_tenant_mode() {
        Some(TenantMode::Database) => {
            let store = get_tenant_store().ok_or_else(|| {
                DbErr::Custom(
                    "DATABASE-ISOLATION: tenant connection store not initialized."
                        .to_owned(),
                )
            })?;

            if let Some(conn) = store.get(tenant_id) {
                return Ok(conn);
            }

            let provider = get_tenant_database_provider();
            if let Some(provider) = provider {
                let databases = provider.provide();
                if let Some(opts) = databases.get(tenant_id) {
                    let conn = connect_and_cache(&store, tenant_id, opts).await?;
                    return Ok(conn);
                }
            }

            Err(DbErr::Custom(format!(
                "DATABASE-ISOLATION: no database connection found for tenant {:?}",
                tenant_id
            )))
        }
        Some(TenantMode::Table) | None => Ok(default_db.clone()),
    }
}

pub fn tenant_logging_db(default_db: &DatabaseConnection) -> Result<SeaOrmExtConnection, DbErr> {
    let db = tenant_db(default_db)?;
    Ok(SeaOrmExtConnection::new(db))
}

pub fn tenant_logging_db_for(default_db: &DatabaseConnection, tenant_id: &Value) -> Result<SeaOrmExtConnection, DbErr> {
    let db = tenant_db_for(default_db, tenant_id)?;
    Ok(SeaOrmExtConnection::new(db))
}

pub trait TenantEntityExt: TenantEntity {
    fn find_with_tenant() -> sea_orm::Select<Self> {
        <sea_orm::Select<Self> as TenantSelectExt>::apply_tenant_filter(
            <Self as EntityTrait>::find()
        )
    }

    fn update_tenant<A>(model: A) -> Result<sea_orm::ValidatedUpdateOne<A>, DbErr>
    where
        A: sea_orm::ActiveModelTrait<Entity = Self>,
    {
        let stmt = <Self as EntityTrait>::update(model).validate()?;
        if !is_tenant_enforced() {
            return Ok(stmt);
        }
        let tenant_id = try_get_tenant_id()?;
        Ok(stmt.filter(Self::tenant_column().eq(tenant_id)))
    }

    fn delete_tenant<A>(model: A) -> Result<sea_orm::ValidatedDeleteOne<Self>, DbErr>
    where
        A: sea_orm::ActiveModelTrait<Entity = Self>,
    {
        let stmt = <Self as EntityTrait>::delete(model).validate()?;
        if !is_tenant_enforced() {
            return Ok(stmt);
        }
        let tenant_id = try_get_tenant_id()?;
        Ok(stmt.filter(Self::tenant_column().eq(tenant_id)))
    }
}

impl<E> TenantEntityExt for E where E: TenantEntity {}
