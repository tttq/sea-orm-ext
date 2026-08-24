#![allow(unused_imports, dead_code)]

use sea_orm::entity::prelude::*;
use sea_orm::ConnectOptions;
use sea_query::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

pub use sea_orm_ext::{
    IdGenerator, FieldFillHandler, FieldFillOperation,
    enable_sql_log, disable_sql_log, set_sql_log_enabled, is_sql_log_enabled,
    set_id_generator, set_field_fill_handler, clear_field_fill_handler, clear_id_generator,
    set_tenant_config, clear_tenant_config,
    get_tenant_context, clear_tenant_context, get_current_tenant_id,
    set_tenant_store, clear_tenant_store, get_tenant_database, get_database_for_tenant,
    TenantConfig, TenantMode, TenantGuard,
    TenantIdProvider, TenantDatabaseProvider,
    set_tenant_id_provider, get_tenant_id_provider,
    set_tenant_database_provider,
    tenant_db, tenant_db_for,
    HashMapConnectionStore, ConnectionStore,
    SeaOrmExtConnection,
    UuidIdGenerator, TypedIdGenerator,
    SoftDeleteTrait,
    DatabaseConfig, TenantDatabaseConfig, TenantDatabaseConfigFile,
    SeaOrmExtError,
    TenantSelectExt, TenantEntity, TenantEntityExt,
    is_tenant_enabled, is_tenant_enforced, is_table_tenant_ignored,
    get_tenant_mode, try_get_tenant_id, require_tenant_id,
    set_tenant_context, TenantContext,
    get_tenant_config,
    get_id_generator, get_field_fill_handler,
    get_effective_tenant_mode,
    get_tenant_database_for_current, get_database_for_tenant_unchecked,
    TenantIgnoreGuard,
};

pub static LOG_INIT: OnceLock<()> = OnceLock::new();

pub fn init_logging() {
    LOG_INIT.get_or_init(|| {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_test_writer()
            .try_init();
    });
}

pub fn reset_global_state() {
    clear_tenant_config();
    clear_tenant_store();
    clear_id_generator();
    clear_field_fill_handler();
    set_tenant_id_provider(Arc::new(TestTenantIdProvider::new()));
    disable_sql_log();
}

/// 重置全局状态并返回 TestTenantIdProvider 的 handle，
/// 便于测试动态设置 tenant_id 和 tenant_mode。
pub fn reset_global_state_with_provider() -> Arc<TestTenantIdProvider> {
    clear_tenant_config();
    clear_tenant_store();
    clear_id_generator();
    clear_field_fill_handler();
    let provider = Arc::new(TestTenantIdProvider::new());
    let handle: Arc<dyn TenantIdProvider> = provider.handle();
    set_tenant_id_provider(handle);
    disable_sql_log();
    provider
}

mod product_entity {
    use sea_orm::entity::prelude::*;
    use sea_orm_ext::DeriveAutoFillSoftDeleteTenant;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel, DeriveAutoFillSoftDeleteTenant)]
    #[sea_orm(table_name = "products")]
    pub struct Model {
        #[sea_orm(primary_key, auto_generate)]
        pub id: i64,
        pub name: String,
        pub price: Option<f64>,
        #[sea_orm_ext(insert)]
        pub created_by: Option<String>,
        #[sea_orm_ext(update)]
        pub updated_by: Option<String>,
        #[sea_orm_ext(insert_update)]
        pub version: i32,
        #[soft_delete(default = 0, del = 1)]
        pub is_deleted: i32,
        #[sea_orm_ext(TENANT)]
        pub tenant_id: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
}

pub use product_entity::{Entity as Product, Model as ProductModel, ActiveModel as ProductActiveModel, Column as ProductColumn};

mod config_entity {
    use sea_orm::entity::prelude::*;
    use sea_orm_ext::DeriveAutoFillSoftDelete;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel, DeriveAutoFillSoftDelete)]
    #[sea_orm(table_name = "sys_config")]
    pub struct Model {
        #[sea_orm(primary_key, auto_generate)]
        pub id: i64,
        pub key: String,
        pub value: String,
        #[sea_orm_ext(insert)]
        pub created_by: Option<String>,
        #[sea_orm_ext(update)]
        pub updated_by: Option<String>,
        #[soft_delete(default = 0, del = 1)]
        pub is_deleted: i32,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
}

pub use config_entity::{Entity as SysConfig, ActiveModel as SysConfigActiveModel};

mod document_entity {
    use sea_orm::entity::prelude::*;
    use sea_orm_ext::DeriveAutoFillSoftDelete;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel, DeriveAutoFillSoftDelete)]
    #[sea_orm(table_name = "documents")]
    pub struct Model {
        #[sea_orm(primary_key, auto_generate)]
        pub id: String,
        pub title: String,
        pub content: Option<String>,
        #[sea_orm_ext(insert)]
        pub created_by: Option<String>,
        #[sea_orm_ext(update)]
        pub updated_by: Option<String>,
        #[soft_delete(default = 0, del = 1)]
        pub is_deleted: i32,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
}

pub use document_entity::{Entity as Document, ActiveModel as DocumentActiveModel};

mod order_entity {
    use sea_orm::entity::prelude::*;
    use sea_orm_ext::DeriveAutoFillTenant;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel, DeriveAutoFillTenant)]
    #[sea_orm(table_name = "orders")]
    pub struct Model {
        #[sea_orm(primary_key, auto_generate)]
        pub id: i64,
        pub product_name: String,
        pub quantity: i32,
        #[sea_orm_ext(insert)]
        pub created_by: Option<String>,
        #[sea_orm_ext(update)]
        pub updated_by: Option<String>,
        #[sea_orm_ext(TENANT)]
        pub tenant_id: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
}

pub use order_entity::{Entity as Order, ActiveModel as OrderActiveModel, Column as OrderColumn};

pub struct TestIdGenerator {
    counter: AtomicI64,
}

impl TestIdGenerator {
    pub fn new() -> Self {
        Self { counter: AtomicI64::new(1000) }
    }
}

impl Default for TestIdGenerator {
    fn default() -> Self { Self::new() }
}

impl IdGenerator for TestIdGenerator {
    fn generate(&self) -> sea_query::Value {
        self.counter.fetch_add(1, Ordering::SeqCst).into()
    }

    fn generate_for_type(&self, _entity_name: &str, _field_name: &str, field_type: &str) -> Option<sea_query::Value> {
        match field_type {
            "String" | "Option<String>" => {
                let v = self.counter.fetch_add(1, Ordering::SeqCst);
                Some(v.to_string().into())
            }
            _ => None,
        }
    }
}

pub struct TestFillHandler {
    pub default_user: String,
}

impl TestFillHandler {
    pub fn new(user: &str) -> Self {
        Self { default_user: user.to_string() }
    }
}

impl FieldFillHandler for TestFillHandler {
    fn fill(&self, _entity: &str, field: &str, op: FieldFillOperation) -> Option<sea_query::Value> {
        match (field, op) {
            ("created_by", FieldFillOperation::Insert) => Some(self.default_user.clone().into()),
            ("updated_by", FieldFillOperation::Update) => Some(self.default_user.clone().into()),
            ("version", FieldFillOperation::Insert) => Some(1i32.into()),
            ("version", FieldFillOperation::Update) => Some(1i32.into()),
            _ => None,
        }
    }
}

pub struct TestTenantIdProvider {
    tenant_id: Arc<RwLock<Option<Value>>>,
    tenant_mode: Arc<RwLock<Option<String>>>,
}

impl TestTenantIdProvider {
    pub fn new() -> Self {
        Self {
            tenant_id: Arc::new(RwLock::new(None)),
            tenant_mode: Arc::new(RwLock::new(None)),
        }
    }

    pub fn set(&self, id: Value) {
        let mut guard = self.tenant_id.write().unwrap();
        *guard = Some(id);
    }

    /// 设置运行时租户模式（模拟 JWT token 中的 tenantMode 字段）
    pub fn set_mode(&self, mode: Option<&str>) {
        let mut guard = self.tenant_mode.write().unwrap();
        *guard = mode.map(|s| s.to_string());
    }

    pub fn handle(&self) -> Arc<dyn TenantIdProvider> {
        Arc::new(Self {
            tenant_id: self.tenant_id.clone(),
            tenant_mode: self.tenant_mode.clone(),
        })
    }
}

impl Default for TestTenantIdProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TenantIdProvider for TestTenantIdProvider {
    fn get_tenant_id(&self) -> Option<Value> {
        let guard = self.tenant_id.read().unwrap();
        guard.clone()
    }

    fn get_tenant_mode(&self) -> Option<String> {
        let guard = self.tenant_mode.read().unwrap();
        guard.clone()
    }
}

pub struct TestTenantDatabaseProvider {
    databases: HashMap<Value, ConnectOptions>,
}

impl TestTenantDatabaseProvider {
    pub fn new(databases: HashMap<Value, ConnectOptions>) -> Self {
        Self { databases }
    }
}

impl TenantDatabaseProvider for TestTenantDatabaseProvider {
    fn provide(&self) -> HashMap<Value, ConnectOptions> {
        self.databases.clone()
    }
}

pub async fn create_sqlite_db() -> sea_orm::DatabaseConnection {
    sea_orm::Database::connect("sqlite::memory:").await.unwrap()
}

pub async fn create_logging_sqlite_db() -> SeaOrmExtConnection {
    let db = sea_orm::Database::connect("sqlite::memory:").await.unwrap();
    SeaOrmExtConnection::new(db)
}

pub async fn setup_product_table(db: &impl sea_orm::ConnectionTrait) {
    db.execute_unprepared(r#"
        CREATE TABLE IF NOT EXISTS products (
            id          INTEGER PRIMARY KEY,
            name        TEXT    NOT NULL,
            price       REAL,
            created_by  TEXT,
            updated_by  TEXT,
            version     INTEGER NOT NULL DEFAULT 0,
            is_deleted  INTEGER NOT NULL DEFAULT 0,
            tenant_id   TEXT
        )
    "#).await.unwrap();
}

pub async fn setup_sys_config_table(db: &impl sea_orm::ConnectionTrait) {
    db.execute_unprepared(r#"
        CREATE TABLE IF NOT EXISTS sys_config (
            id          INTEGER PRIMARY KEY,
            key         TEXT    NOT NULL,
            value       TEXT    NOT NULL,
            created_by  TEXT,
            updated_by  TEXT,
            is_deleted  INTEGER NOT NULL DEFAULT 0
        )
    "#).await.unwrap();
}

pub fn new_product(name: &str, price: Option<f64>) -> ProductActiveModel {
    ProductActiveModel {
        name: sea_orm::Set(name.to_string()),
        price: sea_orm::Set(price),
        ..Default::default()
    }
}

pub async fn find_active_products(db: &impl sea_orm::ConnectionTrait) -> Vec<ProductModel> {
    Product::find()
        .filter(ProductColumn::IsDeleted.eq(0))
        .all(db)
        .await
        .unwrap()
}

pub async fn setup_document_table(db: &impl sea_orm::ConnectionTrait) {
    db.execute_unprepared(r#"
        CREATE TABLE IF NOT EXISTS documents (
            id          TEXT PRIMARY KEY,
            title       TEXT    NOT NULL,
            content     TEXT,
            created_by  TEXT,
            updated_by  TEXT,
            is_deleted  INTEGER NOT NULL DEFAULT 0
        )
    "#).await.unwrap();
}

pub fn new_document(title: &str, content: Option<&str>) -> DocumentActiveModel {
    DocumentActiveModel {
        title: sea_orm::Set(title.to_string()),
        content: sea_orm::Set(content.map(|s| s.to_string())),
        ..Default::default()
    }
}

pub async fn setup_order_table(db: &impl sea_orm::ConnectionTrait) {
    db.execute_unprepared(r#"
        CREATE TABLE IF NOT EXISTS orders (
            id          INTEGER PRIMARY KEY,
            product_name TEXT   NOT NULL,
            quantity    INTEGER NOT NULL DEFAULT 1,
            created_by  TEXT,
            updated_by  TEXT,
            tenant_id   TEXT
        )
    "#).await.unwrap();
}

pub fn new_order(product_name: &str, quantity: i32) -> OrderActiveModel {
    OrderActiveModel {
        product_name: sea_orm::Set(product_name.to_string()),
        quantity: sea_orm::Set(quantity),
        ..Default::default()
    }
}
