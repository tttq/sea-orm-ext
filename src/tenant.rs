use sea_orm::{ColumnTrait, ConnectOptions, DatabaseConnection, DbErr, EntityTrait, QueryFilter};
use sea_query::{DeleteStatement, UpdateStatement, Value};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock, RwLock};

use crate::summer_sea_orm_ext_connection::SeaOrmExtConnection;

#[cfg(feature = "runtime-tokio")]
use std::future::Future;

use crate::tenant_store::ConnectionStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantMode {
    Table,
    Database,
}

impl Default for TenantMode {
    fn default() -> Self {
        Self::Table
    }
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
    let mut guard = store.write().unwrap();
    *guard = Some(Arc::new(config));
}

type SharedTenantIdProvider = Arc<RwLock<Option<Arc<dyn TenantIdProvider>>>>;

static TENANT_ID_PROVIDER: OnceLock<SharedTenantIdProvider> = OnceLock::new();

fn tenant_id_provider_store() -> &'static SharedTenantIdProvider {
    TENANT_ID_PROVIDER.get_or_init(|| Arc::new(RwLock::new(None)))
}

pub fn set_tenant_id_provider(provider: Arc<dyn TenantIdProvider>) {
    let store = tenant_id_provider_store();
    let mut guard = store.write().unwrap();
    *guard = Some(provider);
}

pub fn get_tenant_id_provider() -> Option<Arc<dyn TenantIdProvider>> {
    let store = tenant_id_provider_store();
    let guard = store.read().unwrap();
    guard.clone()
}

type SharedTenantDatabaseProvider = Arc<RwLock<Option<Arc<dyn TenantDatabaseProvider>>>>;

static TENANT_DATABASE_PROVIDER: OnceLock<SharedTenantDatabaseProvider> = OnceLock::new();

fn tenant_database_provider_store() -> &'static SharedTenantDatabaseProvider {
    TENANT_DATABASE_PROVIDER.get_or_init(|| Arc::new(RwLock::new(None)))
}

pub fn set_tenant_database_provider(provider: Arc<dyn TenantDatabaseProvider>) {
    let store = tenant_database_provider_store();
    let mut guard = store.write().unwrap();
    *guard = Some(provider);
}

pub fn get_tenant_database_provider() -> Option<Arc<dyn TenantDatabaseProvider>> {
    let store = tenant_database_provider_store();
    let guard = store.read().unwrap();
    guard.clone()
}

pub trait TenantIdProvider: Send + Sync + 'static {
    fn get_tenant_id(&self) -> Option<Value>;
}

pub trait TenantDatabaseProvider: Send + Sync + 'static {
    fn provide(&self) -> HashMap<Value, ConnectOptions>;
}

pub fn clear_tenant_config() {
    let store = tenant_config_store();
    let mut guard = store.write().unwrap();
    *guard = None;
}

pub fn get_tenant_config() -> Option<Arc<TenantConfig>> {
    let store = tenant_config_store();
    let guard = store.read().unwrap();
    guard.clone()
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
    static THREAD_TENANT_CONTEXT: RefCell<Option<TenantContext>> = RefCell::new(None);
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
    is_tenant_enabled() && get_tenant_mode() == Some(TenantMode::Table) && !is_tenant_filter_disabled()
}

thread_local! {
    static TENANT_FILTER_DISABLED: Cell<bool> = Cell::new(false);
}

fn is_tenant_filter_disabled() -> bool {
    TENANT_FILTER_DISABLED.with(|f| f.get())
}

pub struct TenantIgnoreGuard {
    _private: (),
}

impl TenantIgnoreGuard {
    pub fn new() -> Self {
        TENANT_FILTER_DISABLED.with(|f| f.set(true));
        Self { _private: () }
    }
}

impl Drop for TenantIgnoreGuard {
    fn drop(&mut self) {
        TENANT_FILTER_DISABLED.with(|f| f.set(false));
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

pub fn require_tenant_id() -> Value {
    get_current_tenant_id().unwrap_or_else(|| {
        panic!(
            "TABLE-ISOLATION: tenant ID is required but unavailable. \
             Ensure tenant context has been set via set_tenant_context() or TenantGuard::set()."
        )
    })
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
        if is_table_tenant_ignored(table.as_ref()) {
            return self;
        }
        let tenant_id = require_tenant_id();
        self.filter(E::tenant_column().eq(tenant_id))
    }
}

pub fn apply_tenant_condition<E>(mut stmt: UpdateStatement) -> UpdateStatement
where
    E: TenantEntity,
{
    if !is_tenant_enforced() {
        return stmt;
    }
    if is_table_tenant_ignored(E::default().table_name().as_ref()) {
        return stmt;
    }
    let tenant_id = require_tenant_id();
    stmt.cond_where(E::tenant_column().eq(tenant_id));
    stmt
}

pub fn apply_tenant_delete_condition<E>(mut stmt: DeleteStatement) -> DeleteStatement
where
    E: TenantEntity,
{
    if !is_tenant_enforced() {
        return stmt;
    }
    if is_table_tenant_ignored(E::default().table_name().as_ref()) {
        return stmt;
    }
    let tenant_id = require_tenant_id();
    stmt.cond_where(E::tenant_column().eq(tenant_id));
    stmt
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
    let mut guard = inner.write().unwrap();
    *guard = Some(store);
}

pub fn clear_tenant_store() {
    let inner = tenant_store_inner();
    let mut guard = inner.write().unwrap();
    *guard = None;
}

pub fn get_tenant_store() -> Option<Arc<dyn ConnectionStore>> {
    let inner = tenant_store_inner();
    let guard = inner.read().unwrap();
    guard.clone()
}

type SharedDefaultDb = Arc<RwLock<Option<DatabaseConnection>>>;

static DEFAULT_DB: OnceLock<SharedDefaultDb> = OnceLock::new();

fn default_db_inner() -> &'static SharedDefaultDb {
    DEFAULT_DB.get_or_init(|| Arc::new(RwLock::new(None)))
}

pub fn set_default_database(db: DatabaseConnection) {
    let inner = default_db_inner();
    let mut guard = inner.write().unwrap();
    *guard = Some(db);
}

pub fn get_default_database() -> Option<DatabaseConnection> {
    let inner = default_db_inner();
    let guard = inner.read().unwrap();
    guard.clone()
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
        return Ok(get_default_database());
    }

    if get_tenant_mode() != Some(TenantMode::Database) {
        return Ok(get_default_database());
    }

    let Some(tenant_id) = get_current_tenant_id() else {
        return Ok(get_default_database());
    };

    get_database_for_tenant(&tenant_id)
}

pub fn get_database_for_tenant(tenant_id: &Value) -> Result<Option<DatabaseConnection>, DbErr> {
    if !is_tenant_enabled() {
        return Ok(get_default_database());
    }

    if get_tenant_mode() != Some(TenantMode::Database) {
        return Ok(get_default_database());
    }

    let Some(store) = get_tenant_store() else {
        return Err(DbErr::Custom(
            "DATABASE-ISOLATION: tenant connection store not initialized.".to_owned(),
        ));
    };

    if let Some(conn) = store.get(tenant_id) {
        return Ok(Some(conn));
    }

    Ok(None)
}

#[cfg(feature = "runtime-tokio")]
pub async fn get_database_for_tenant_async(tenant_id: &Value) -> Result<Option<DatabaseConnection>, DbErr> {
    if !is_tenant_enabled() {
        return Ok(get_default_database());
    }

    if get_tenant_mode() != Some(TenantMode::Database) {
        return Ok(get_default_database());
    }

    let Some(store) = get_tenant_store() else {
        return Err(DbErr::Custom(
            "DATABASE-ISOLATION: tenant connection store not initialized.".to_owned(),
        ));
    };

    if let Some(conn) = store.get(tenant_id) {
        return Ok(Some(conn));
    }

    let provider = get_tenant_database_provider();
    if let Some(provider) = provider {
        let databases = provider.provide();
        if let Some(opts) = databases.get(tenant_id) {
            let conn = connect_and_cache(&store, tenant_id, opts).await?;
            return Ok(Some(conn));
        }
    }

    Err(DbErr::Custom(format!(
        "DATABASE-ISOLATION: no database connection found for tenant {:?}, \
         and no TenantDatabaseProvider can provide one",
        tenant_id
    )))
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

pub trait TenantActiveModelExt: sea_orm::ActiveModelTrait {
    fn ensure_tenant_id(&mut self) where Self: Sized {}
}
