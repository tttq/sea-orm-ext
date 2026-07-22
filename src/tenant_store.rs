use async_trait::async_trait;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, DbErr, Statement};
use sea_query::Value;
use std::sync::Arc;

use crate::SeaOrmExtConnection;

/// 租户数据库连接构建器函数类型。
pub type TenantDatabaseBuilder =
    Box<dyn Fn(&Value) -> Result<DatabaseConnection, DbErr> + Send + Sync>;

pub trait ConnectionStore: Send + Sync + 'static {
    fn get(&self, tenant_id: &Value) -> Option<DatabaseConnection>;

    fn insert(&self, tenant_id: Value, conn: DatabaseConnection) -> Result<(), DbErr>;

    fn remove(&self, tenant_id: &Value) -> Result<(), DbErr>;

    fn len(&self) -> usize;

    fn is_empty(&self) -> bool;

    fn get_all_tenants(&self) -> Vec<Value>;

    fn get_ext(&self, tenant_id: &Value) -> Option<SeaOrmExtConnection>;

    fn insert_ext(&self, tenant_id: Value, conn: SeaOrmExtConnection) -> Result<(), DbErr>;
}

fn value_to_string(tenant_id: &Value) -> Option<String> {
    match tenant_id {
        Value::String(Some(s)) => Some(s.clone()),
        Value::Int(Some(i)) => Some(i.to_string()),
        Value::BigInt(Some(i)) => Some(i.to_string()),
        Value::SmallInt(Some(i)) => Some(i.to_string()),
        Value::TinyInt(Some(i)) => Some(i.to_string()),
        Value::TinyUnsigned(Some(i)) => Some(i.to_string()),
        Value::SmallUnsigned(Some(i)) => Some(i.to_string()),
        Value::Unsigned(Some(i)) => Some(i.to_string()),
        Value::BigUnsigned(Some(i)) => Some(i.to_string()),
        _ => None,
    }
}

fn value_to_string_err(tenant_id: &Value) -> Result<String, DbErr> {
    value_to_string(tenant_id).ok_or_else(|| DbErr::Type("Invalid tenant ID type, expected String or integer".to_owned()))
}

/// 基于 `dashmap` 的并发连接存储。
///
/// 使用 DashMap 替代 `RwLock<HashMap>`，提供更细粒度的锁：
/// - DashMap 内部分片（默认 16 个 shard），每个 shard 独立加锁
/// - 读多写少场景下性能显著优于 `RwLock<HashMap>`
/// - 不会因为单个租户的写操作阻塞其他租户的读操作
pub struct HashMapConnectionStore {
    connections: dashmap::DashMap<String, SeaOrmExtConnection>,
}

impl HashMapConnectionStore {
    pub fn new() -> Self {
        Self {
            connections: dashmap::DashMap::new(),
        }
    }
}

impl Default for HashMapConnectionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionStore for HashMapConnectionStore {
    fn get(&self, tenant_id: &Value) -> Option<DatabaseConnection> {
        let key = value_to_string(tenant_id)?;
        self.connections.get(&key).map(|c| c.inner().clone())
    }

    fn insert(&self, tenant_id: Value, conn: DatabaseConnection) -> Result<(), DbErr> {
        let key = value_to_string_err(&tenant_id)?;
        self.connections.insert(key, SeaOrmExtConnection::new(conn));
        Ok(())
    }

    fn remove(&self, tenant_id: &Value) -> Result<(), DbErr> {
        let key = value_to_string_err(tenant_id)?;
        self.connections.remove(&key);
        Ok(())
    }

    fn len(&self) -> usize {
        self.connections.len()
    }

    fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }

    fn get_all_tenants(&self) -> Vec<Value> {
        self.connections
            .iter()
            .map(|r| Value::String(Some(r.key().clone())))
            .collect()
    }

    fn get_ext(&self, tenant_id: &Value) -> Option<SeaOrmExtConnection> {
        let key = value_to_string(tenant_id)?;
        self.connections.get(&key).map(|c| c.clone())
    }

    fn insert_ext(&self, tenant_id: Value, conn: SeaOrmExtConnection) -> Result<(), DbErr> {
        let key = value_to_string_err(&tenant_id)?;
        self.connections.insert(key, conn);
        Ok(())
    }
}

pub struct TenantConnectionWrapper<C: ConnectionTrait> {
    inner: C,
    store: Arc<dyn ConnectionStore>,
}

impl<C: ConnectionTrait> TenantConnectionWrapper<C> {
    pub fn new(inner: C, store: Arc<dyn ConnectionStore>) -> Self {
        Self { inner, store }
    }

    pub fn get_connection_for_tenant(&self, tenant_id: &Value) -> Result<DatabaseConnection, DbErr> {
        self.store.get(tenant_id).ok_or_else(|| {
            DbErr::Custom(format!("No connection found for tenant: {:?}", tenant_id))
        })
    }
}

#[async_trait]
impl<C: ConnectionTrait + Send + Sync> ConnectionTrait for TenantConnectionWrapper<C> {
    fn get_database_backend(&self) -> DbBackend {
        self.inner.get_database_backend()
    }

    async fn execute_raw(&self, stmt: Statement) -> Result<sea_orm::ExecResult, DbErr> {
        self.inner.execute_raw(stmt).await
    }

    async fn execute_unprepared(&self, sql: &str) -> Result<sea_orm::ExecResult, DbErr> {
        self.inner.execute_unprepared(sql).await
    }

    async fn query_one_raw(&self, stmt: Statement) -> Result<Option<sea_orm::QueryResult>, DbErr> {
        self.inner.query_one_raw(stmt).await
    }

    async fn query_all_raw(&self, stmt: Statement) -> Result<Vec<sea_orm::QueryResult>, DbErr> {
        self.inner.query_all_raw(stmt).await
    }
}

pub struct TenantDatabaseManager {
    store: Arc<dyn ConnectionStore>,
    builder: TenantDatabaseBuilder,
}

impl TenantDatabaseManager {
    pub fn new(
        store: Arc<dyn ConnectionStore>,
        builder: TenantDatabaseBuilder,
    ) -> Self {
        Self { store, builder }
    }

    pub fn initialize_tenants(&self, tenant_ids: Vec<Value>) -> Result<(), DbErr> {
        for tenant_id in tenant_ids {
            self.get_or_create_connection(&tenant_id)?;
        }
        Ok(())
    }

    pub fn get_or_create_connection(&self, tenant_id: &Value) -> Result<DatabaseConnection, DbErr> {
        if let Some(conn) = self.store.get(tenant_id) {
            return Ok(conn);
        }

        let conn = (self.builder)(tenant_id)?;
        self.store.insert(tenant_id.clone(), conn.clone())?;
        Ok(conn)
    }

    pub fn get_connection(&self, tenant_id: &Value) -> Result<DatabaseConnection, DbErr> {
        self.store.get(tenant_id).ok_or_else(|| {
            DbErr::Custom(format!("No connection found for tenant: {:?}", tenant_id))
        })
    }

    pub fn remove_tenant(&self, tenant_id: &Value) -> Result<(), DbErr> {
        self.store.remove(tenant_id)
    }

    pub fn store(&self) -> Arc<dyn ConnectionStore> {
        self.store.clone()
    }
}

// ============================================================================
// Tokio 异步支持模块
// ============================================================================

#[cfg(feature = "runtime-tokio")]
mod tokio_support {
    use super::*;
    use sea_orm::{ConnectOptions, DatabaseConnection, DbErr};

    /// 基于 `dashmap` 的 Tokio 异步连接存储。
    ///
    /// 使用 DashMap 作为底层存储（无锁读、分片写），
    /// 避免在 async 上下文中持有 `std::sync::RwLock` 跨 `.await`。
    ///
    /// 对于需要在 async 上下文中创建连接的场景，使用 `connect()` 方法。
    pub struct TokioConnectionStore {
        connections: dashmap::DashMap<String, SeaOrmExtConnection>,
    }

    impl TokioConnectionStore {
        pub fn new() -> Self {
            Self {
                connections: dashmap::DashMap::new(),
            }
        }

        /// 异步创建数据库连接并缓存。
        ///
        /// 此方法使用传入的 `ConnectOptions` 而非硬编码参数，
        /// 允许调用方完全控制连接池配置。
        pub async fn connect(
            &self,
            tenant_id: String,
            url: &str,
        ) -> Result<DatabaseConnection, DbErr> {
            let mut opt = ConnectOptions::new(url.to_owned());
            opt.max_connections(50)
                .min_connections(5)
                .connect_timeout(std::time::Duration::from_secs(30))
                .acquire_timeout(std::time::Duration::from_secs(30))
                .idle_timeout(std::time::Duration::from_secs(600))
                .sqlx_logging(false);

            let conn = sea_orm::Database::connect(opt).await?;
            let ext_conn = SeaOrmExtConnection::new(conn);
            self.connections.insert(tenant_id, ext_conn.clone());
            Ok(ext_conn.inner().clone())
        }

        /// 异步创建数据库连接（使用传入的 ConnectOptions）并缓存。
        pub async fn connect_with_options(
            &self,
            tenant_id: String,
            options: ConnectOptions,
        ) -> Result<DatabaseConnection, DbErr> {
            let conn = sea_orm::Database::connect(options).await?;
            let ext_conn = SeaOrmExtConnection::new(conn);
            self.connections.insert(tenant_id, ext_conn.clone());
            Ok(ext_conn.inner().clone())
        }
    }

    impl Default for TokioConnectionStore {
        fn default() -> Self {
            Self::new()
        }
    }

    impl ConnectionStore for TokioConnectionStore {
        fn get(&self, tenant_id: &Value) -> Option<DatabaseConnection> {
            let key = value_to_string(tenant_id)?;
            self.connections.get(&key).map(|c| c.inner().clone())
        }

        fn insert(&self, tenant_id: Value, conn: DatabaseConnection) -> Result<(), DbErr> {
            let key = value_to_string_err(&tenant_id)?;
            self.connections.insert(key, SeaOrmExtConnection::new(conn));
            Ok(())
        }

        fn remove(&self, tenant_id: &Value) -> Result<(), DbErr> {
            let key = value_to_string_err(tenant_id)?;
            self.connections.remove(&key);
            Ok(())
        }

        fn len(&self) -> usize {
            self.connections.len()
        }

        fn is_empty(&self) -> bool {
            self.connections.is_empty()
        }

        fn get_all_tenants(&self) -> Vec<Value> {
            self.connections
                .iter()
                .map(|r| Value::String(Some(r.key().clone())))
                .collect()
        }

        fn get_ext(&self, tenant_id: &Value) -> Option<SeaOrmExtConnection> {
            let key = value_to_string(tenant_id)?;
            self.connections.get(&key).map(|c| c.clone())
        }

        fn insert_ext(&self, tenant_id: Value, conn: SeaOrmExtConnection) -> Result<(), DbErr> {
            let key = value_to_string_err(&tenant_id)?;
            self.connections.insert(key, conn);
            Ok(())
        }
    }
}

#[cfg(feature = "runtime-tokio")]
pub use tokio_support::TokioConnectionStore;

#[cfg(feature = "runtime-tokio")]
mod async_support {
    use super::*;
    use sea_orm::DatabaseConnection;
    use tokio::sync::RwLock;

    #[async_trait::async_trait]
    pub trait AsyncConnectionStore: Send + Sync + 'static {
        async fn get(&self, tenant_id: &Value) -> Option<DatabaseConnection>;
        async fn insert(&self, tenant_id: Value, conn: DatabaseConnection) -> Result<(), DbErr>;
        async fn remove(&self, tenant_id: &Value) -> Result<(), DbErr>;
        async fn len(&self) -> usize;
        async fn is_empty(&self) -> bool;
        async fn get_all_tenants(&self) -> Vec<Value>;
    }

    /// 基于 `tokio::sync::RwLock` 的异步连接存储。
    ///
    /// 适用于需要在 async 上下文中持有锁跨 `.await` 的场景。
    /// 对于一般场景，推荐使用 `HashMapConnectionStore`（基于 dashmap，无锁读）。
    pub struct TokioAsyncConnectionStore {
        connections: RwLock<std::collections::HashMap<String, SeaOrmExtConnection>>,
    }

    impl TokioAsyncConnectionStore {
        pub fn new() -> Self {
            Self {
                connections: RwLock::new(std::collections::HashMap::new()),
            }
        }
    }

    impl Default for TokioAsyncConnectionStore {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait::async_trait]
    impl AsyncConnectionStore for TokioAsyncConnectionStore {
        async fn get(&self, tenant_id: &Value) -> Option<DatabaseConnection> {
            let key = value_to_string(tenant_id)?;
            let connections = self.connections.read().await;
            connections.get(&key).map(|c| c.inner().clone())
        }

        async fn insert(&self, tenant_id: Value, conn: DatabaseConnection) -> Result<(), DbErr> {
            let key = value_to_string_err(&tenant_id)?;
            let mut connections = self.connections.write().await;
            connections.insert(key, SeaOrmExtConnection::new(conn));
            Ok(())
        }

        async fn remove(&self, tenant_id: &Value) -> Result<(), DbErr> {
            let key = value_to_string_err(tenant_id)?;
            let mut connections = self.connections.write().await;
            connections.remove(&key);
            Ok(())
        }

        async fn len(&self) -> usize {
            self.connections.read().await.len()
        }

        async fn is_empty(&self) -> bool {
            self.connections.read().await.is_empty()
        }

        async fn get_all_tenants(&self) -> Vec<Value> {
            self.connections
                .read()
                .await
                .keys()
                .map(|v| Value::String(Some(v.clone())))
                .collect()
        }
    }
}

#[cfg(feature = "runtime-tokio")]
pub use async_support::{AsyncConnectionStore, TokioAsyncConnectionStore};
