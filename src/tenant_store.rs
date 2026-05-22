use async_trait::async_trait;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, DbErr, Statement};
use sea_query::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::SeaOrmExtConnection;

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

pub struct HashMapConnectionStore {
    connections: RwLock<HashMap<String, SeaOrmExtConnection>>,
}

impl HashMapConnectionStore {
    pub fn new() -> Self {
        Self {
            connections: RwLock::new(HashMap::new()),
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
        let connections = self.connections.read().ok()?;
        connections.get(&key).map(|c| c.inner().clone())
    }

    fn insert(&self, tenant_id: Value, conn: DatabaseConnection) -> Result<(), DbErr> {
        let key = value_to_string_err(&tenant_id)?;
        let mut connections = self.connections.write().map_err(|_| {
            DbErr::Custom("Lock poisoned".to_owned())
        })?;
        connections.insert(key, SeaOrmExtConnection::new(conn));
        Ok(())
    }

    fn remove(&self, tenant_id: &Value) -> Result<(), DbErr> {
        let key = value_to_string_err(tenant_id)?;
        let mut connections = self.connections.write().map_err(|_| {
            DbErr::Custom("Lock poisoned".to_owned())
        })?;
        connections.remove(&key);
        Ok(())
    }

    fn len(&self) -> usize {
        self.connections.read().map(|c| c.len()).unwrap_or(0)
    }

    fn is_empty(&self) -> bool {
        self.connections.read().map(|c| c.is_empty()).unwrap_or(true)
    }

    fn get_all_tenants(&self) -> Vec<Value> {
        self.connections
            .read()
            .map(|c| c.keys().map(|v| Value::String(Some(v.clone()))).collect())
            .unwrap_or_default()
    }

    fn get_ext(&self, tenant_id: &Value) -> Option<SeaOrmExtConnection> {
        let key = value_to_string(tenant_id)?;
        let connections = self.connections.read().ok()?;
        connections.get(&key).cloned()
    }

    fn insert_ext(&self, tenant_id: Value, conn: SeaOrmExtConnection) -> Result<(), DbErr> {
        let key = value_to_string_err(&tenant_id)?;
        let mut connections = self.connections.write().map_err(|_| {
            DbErr::Custom("Lock poisoned".to_owned())
        })?;
        connections.insert(key, conn);
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
    builder: Box<dyn Fn(&Value) -> Result<DatabaseConnection, DbErr> + Send + Sync>,
}

impl TenantDatabaseManager {
    pub fn new(
        store: Arc<dyn ConnectionStore>,
        builder: Box<dyn Fn(&Value) -> Result<DatabaseConnection, DbErr> + Send + Sync>,
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

#[cfg(feature = "runtime-tokio")]
mod tokio_support {
    use super::*;
    use sea_orm::{ConnectOptions, DatabaseConnection, DbErr};

    pub struct TokioConnectionStore {
        connections: std::sync::RwLock<HashMap<String, SeaOrmExtConnection>>,
    }

    impl TokioConnectionStore {
        pub fn new() -> Self {
            Self {
                connections: std::sync::RwLock::new(HashMap::new()),
            }
        }

        pub async fn connect(
            &self,
            tenant_id: String,
            url: &str,
        ) -> Result<DatabaseConnection, DbErr> {
            let mut opt = ConnectOptions::new(url.to_owned());
            opt.max_connections(10)
                .min_connections(1)
                .connect_timeout(std::time::Duration::from_secs(30))
                .acquire_timeout(std::time::Duration::from_secs(30))
                .sqlx_logging(false);

            let conn = sea_orm::Database::connect(opt).await?;
            let ext_conn = SeaOrmExtConnection::new(conn);
            let mut connections = self.connections.write().unwrap();
            connections.insert(tenant_id, ext_conn.clone());
            Ok(ext_conn.inner().clone())
        }

        pub async fn connect_with_options(
            &self,
            tenant_id: String,
            options: ConnectOptions,
        ) -> Result<DatabaseConnection, DbErr> {
            let conn = sea_orm::Database::connect(options).await?;
            let ext_conn = SeaOrmExtConnection::new(conn);
            let mut connections = self.connections.write().unwrap();
            connections.insert(tenant_id, ext_conn.clone());
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
            let connections = self.connections.read().ok()?;
            connections.get(&key).map(|c| c.inner().clone())
        }

        fn insert(&self, tenant_id: Value, conn: DatabaseConnection) -> Result<(), DbErr> {
            let key = value_to_string_err(&tenant_id)?;
            let mut connections = self.connections.write().map_err(|_| {
                DbErr::Custom("Lock poisoned".to_owned())
            })?;
            connections.insert(key, SeaOrmExtConnection::new(conn));
            Ok(())
        }

        fn remove(&self, tenant_id: &Value) -> Result<(), DbErr> {
            let key = value_to_string_err(tenant_id)?;
            let mut connections = self.connections.write().map_err(|_| {
                DbErr::Custom("Lock poisoned".to_owned())
            })?;
            connections.remove(&key);
            Ok(())
        }

        fn len(&self) -> usize {
            self.connections.read().map(|c| c.len()).unwrap_or(0)
        }

        fn is_empty(&self) -> bool {
            self.connections.read().map(|c| c.is_empty()).unwrap_or(true)
        }

        fn get_all_tenants(&self) -> Vec<Value> {
            self.connections
                .read()
                .map(|c| c.keys().map(|v| Value::String(Some(v.clone()))).collect())
                .unwrap_or_default()
        }

        fn get_ext(&self, tenant_id: &Value) -> Option<SeaOrmExtConnection> {
            let key = value_to_string(tenant_id)?;
            let connections = self.connections.read().ok()?;
            connections.get(&key).cloned()
        }

        fn insert_ext(&self, tenant_id: Value, conn: SeaOrmExtConnection) -> Result<(), DbErr> {
            let key = value_to_string_err(&tenant_id)?;
            let mut connections = self.connections.write().map_err(|_| {
                DbErr::Custom("Lock poisoned".to_owned())
            })?;
            connections.insert(key, conn);
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

    pub struct TokioAsyncConnectionStore {
        connections: RwLock<HashMap<String, SeaOrmExtConnection>>,
    }

    impl TokioAsyncConnectionStore {
        pub fn new() -> Self {
            Self {
                connections: RwLock::new(HashMap::new()),
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
