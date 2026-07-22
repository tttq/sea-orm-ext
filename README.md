# summer-sea-orm-ext

<div align="center">

![logo](https://img.shields.io/badge/summer--sea--orm--ext-Enterprise%20Extension-blue?style=for-the-badge)

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.81+-blue.svg?style=for-the-badge)](https://www.rust-lang.org)
[![crates.io](https://img.shields.io/badge/crates.io-v0.0.1-orange.svg?style=for-the-badge)](https://crates.io/crates/summer-sea-orm-ext)
[![docs.rs](https://img.shields.io/badge/docs.rs-latest-blue.svg?style=for-the-badge)](https://docs.rs/summer-sea-orm-ext)
[![Test Status](https://img.shields.io/badge/tests-114%20passed-green?style=for-the-badge)](#测试覆盖)

> ⚡ SeaORM 非侵入式企业级扩展 — 一行注解开启自动填充、软删除、多租户隔离，深度集成 Summer 框架

</div>

---

## ✨ 为什么选择 summer-sea-orm-ext？

| 特性 | 说明 | 对比 MyBatis-Plus |
|------|------|-------------------|
| **🔄 自动填充** | INSERT/UPDATE 时自动填充 `created_by`、`updated_by` 等审计字段 | 对应 `MetaObjectHandler` |
| **🛡️ 软删除** | DELETE 自动转为逻辑删除，自动过滤已删除记录 | 对应 `@TableLogic` |
| **🏢 多租户** | Table/Database 两种隔离模式，SELECT/UPDATE/DELETE 全自动租户过滤 | 对应 `@MultiTenant` |
| **⚡ Summer 集成** | 声明式配置 + 自动数据库切换，开箱即用 | 无直接对应 |
| **📝 SQL 日志** | 打印完整 SQL（参数值注入）+ 独立参数列表，调试无忧 | 对应 `log-impl:2.x` |
| **📄 分页查询** | Web 友好的分页扩展，自动从请求参数解析分页信息 | 对应 `PageHelper` |
| **🔗 单依赖引入** | Re-export sea-orm/sea-query/summer/summer-web，一个依赖全搞定 | 无直接对应 |

> 🔑 **核心设计理念**：所有功能通过派生宏和 Trait 实现，**非侵入式**，不污染原有 SeaORM API

---

## 📦 安装

```toml
[dependencies]
# 默认：启用 runtime-tokio-native-tls
summer-sea-orm-ext = "0.0.1"

# 指定数据库驱动
summer-sea-orm-ext = { version = "0.0.1", features = ["postgres"] }
summer-sea-orm-ext = { version = "0.0.1", features = ["mysql"] }
summer-sea-orm-ext = { version = "0.0.1", features = ["sqlite"] }

# Summer + Web + PostgreSQL + Rustls + Chrono + OpenAPI
summer-sea-orm-ext = { version = "0.0.1", features = [
    "summer", "summer-web", "postgres", "rustls",
    "with-chrono", "with-uuid", "openapi"
] }

# 启用全部功能
summer-sea-orm-ext = { version = "0.0.1", features = ["full"] }
```

> 💡 **无需单独引入** `sea-orm`、`sea-query`、`summer`、`summer-web`，`summer-sea-orm-ext` 已 re-export 所有依赖。

### Feature Flags

#### 数据库驱动 & TLS

| Feature | 说明 | 透传到 |
|---------|------|--------|
| `default` | `sea-orm/runtime-tokio-native-tls` | sea-orm 内置 |
| `mysql` | MySQL 驱动 | sea-orm/sqlx-mysql |
| `postgres` | PostgreSQL 驱动 | sea-orm/sqlx-postgres |
| `sqlite` | SQLite 驱动 | sea-orm/sqlx-sqlite |
| `sqlx-all` | 一次性启用 MySQL + PostgreSQL + SQLite | sea-orm/sqlx-all |
| `rustls` | 使用 rustls 替代 native-tls（别名） | runtime-tokio-rustls |
| `runtime-tokio-native-tls` | Tokio + native-tls | sea-orm/runtime-tokio-native-tls |
| `runtime-tokio-rustls` | Tokio + rustls | sea-orm/runtime-tokio-rustls |
| `runtime-async-std-native-tls` | async-std + native-tls（需 `--no-default-features`） | sea-orm/runtime-async-std-native-tls |
| `runtime-async-std-rustls` | async-std + rustls（需 `--no-default-features`） | sea-orm/runtime-async-std-rustls |

> ⚠️ **运行时互斥**：`runtime-tokio-*` 与 `runtime-async-std-*` 互斥。如需使用 async-std 运行时，请使用 `--no-default-features` 关闭默认的 tokio 运行时后再启用对应 async-std 特性。

#### 流式

| Feature | 说明 | 透传到 |
|---------|------|--------|
| `stream` | 流式查询（sea-orm 默认开启） | sea-orm/stream |

#### sea-orm 类型支持

| Feature | 说明 |
|---------|------|
| `with-json` | JSON 类型 (serde_json) |
| `with-chrono` | Chrono 时间类型 |
| `with-rust_decimal` | Decimal 类型 |
| `with-uuid` | UUID 类型 |
| `with-time` | Time 类型 |
| `with-bigdecimal` | BigDecimal 类型 |
| `with-ipnetwork` | IP 网络类型 |
| `with-mac_address` | MAC 地址类型 |
| `with-arrow` | Arrow 类型 |

#### sea-orm 扩展

| Feature | 说明 |
|---------|------|
| `postgres-array` | PostgreSQL 数组 |
| `postgres-use-serial-pk` | PostgreSQL 串行主键 |
| `postgres-vector` | pgvector 支持 |
| `json-array` | JSON 数组 |
| `sqlite-use-returning-for-3_35` | SQLite RETURNING |
| `sqlite-no-row-value-before-3_15` | SQLite 兼容 3.15 之前版本 |
| `mariadb-use-returning` | MariaDB RETURNING |
| `debug-print` | 调试打印 |
| `proxy` | 代理模式 |
| `rbac` | RBAC 权限控制 |
| `schema-sync` | Schema 同步 |
| `seaography` | Seaography GraphQL |
| `tracing-spans` | Tracing spans |
| `entity-registry` | 实体注册 |

#### Summer 框架 & Web

| Feature | 说明 | 透传到 |
|---------|------|--------|
| `summer` | Summer 框架插件集成 | dep:summer |
| `summer-web` | axum 自动租户切换 | summer-web, tower |
| `http2` | HTTP/2 支持 | summer-web?/http2 |
| `multipart` | 文件上传 | summer-web?/multipart |
| `openapi` | OpenAPI 文档 | summer-web?/openapi |
| `openapi-redoc` | ReDoc UI | summer-web?/openapi-redoc |
| `openapi-scalar` | Scalar UI | summer-web?/openapi-scalar |
| `openapi-swagger` | Swagger UI | summer-web?/openapi-swagger |
| `socket-io` | Socket.IO | summer-web?/socket_io |
| `ws` | WebSocket | summer-web?/ws |
| `with-web` | summer-web 别名 | summer-web |
| `with-web-openapi` | openapi 别名 | openapi |

#### Runtime

| Feature | 说明 |
|---------|------|
| `runtime-tokio` | Tokio 异步运行时 |
| `full` | 启用全部功能 (summer + summer-web + runtime-tokio) |

---

## 🚀 快速开始

### 1. 定义实体（3 行注解 = 全功能）

```rust
use summer_sea_orm_ext::sea_orm::entity::prelude::*;

// 一行注解 = 自动填充 + 软删除 + 多租户
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, DeriveAutoFillSoftDeleteTenant)]
#[sea_orm(table_name = "orders")]
pub struct Model {
    #[sea_orm(primary_key, auto_generate)]
    pub id: i64,

    pub product_name: String,
    pub quantity: i32,

    // 🔽 自动填充字段
    #[summer_sea_orm_ext(insert)]
    pub created_by: Option<String>,
    #[summer_sea_orm_ext(update)]
    pub updated_by: Option<String>,

    // 🔽 软删除字段
    #[soft_delete(default = 0, del = 1)]
    pub is_deleted: i32,

    // 🔽 租户隔离字段
    #[summer_sea_orm_ext(TENANT)]
    pub tenant_id: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
```

### 2. 设置全局处理器（一次配置，永久生效）

```rust
use summer_sea_orm_ext::*;

// ID 生成器：Snowflake 算法
set_id_generator(Box::new(SnowflakeIdGenerator::new(1)));

// 字段填充处理器：自动填充审计字段
struct MyFillHandler;
impl FieldFillHandler for MyFillHandler {
    fn fill(&self, _: &str, field: &str, op: FieldFillOperation) -> Option<Value> {
        match (field, op) {
            ("created_by", FieldFillOperation::Insert) => Some("system".into()),
            ("updated_by", FieldFillOperation::Update) => Some("system".into()),
            _ => None,
        }
    }
}
set_field_fill_handler(Box::new(MyFillHandler));

// 多租户配置
set_tenant_config(TenantConfig {
    enabled: true,
    mode: TenantMode::Table,
    default_tenant_id: Some(Value::String(Some("1".to_string()))),
    ignored_tables: Default::default(),
});

// SQL 日志开关
enable_sql_log();
```

### 3. CRUD 操作（零改动，自动生效）

```rust
use summer_sea_orm_ext::sea_orm::{ActiveModelTrait, EntityTrait, Set};

// ✅ INSERT：自动填充 created_by + tenant_id
let order = orders::ActiveModel {
    product_name: Set("Laptop".into()),
    quantity: Set(2),
    ..Default::default()
};
let inserted = order.insert(&db).await?;

// ✅ SELECT：自动过滤 is_deleted=0 AND tenant_id=?
let my_orders = orders::Entity::find().all(&db).await?;

// ✅ UPDATE：自动添加租户条件 + 填充 updated_by
let mut am: orders::ActiveModel = inserted.into();
am.quantity = Set(5);
let updated = am.update(&db).await?;

// ✅ DELETE：自动转为 UPDATE SET is_deleted=1
let result = am.delete(&db).await?; // ✅ 软删除成功
```

### 4. 多租户场景示例

```rust
// 🔷 Table 模式：自动在 SQL 中注入 tenant_id
{
    let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
    let orders = orders::Entity::find().all(&db).await?;
    // SQL: WHERE tenant_id = '1'
}

// 🔷 批量更新/删除：自动叠加租户 WHERE（防止跨租户操作）
{
    let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
    // 只更新租户 1 的记录，SQL: UPDATE ... WHERE tenant_id = '1'
    orders::Entity::update_many()
        .col_expr(Column::Quantity, Expr::value(0))
        .exec(&db).await?;

    // 只删除租户 1 的记录，SQL: DELETE ... WHERE tenant_id = '1'
    orders::Entity::delete_many().exec(&db).await?;
}

// 🔷 跨租户运维场景：显式绕过租户过滤
{
    // 需跨租户的数据迁移、清理等场景使用 _without_tenant() 方法
    orders::Entity::update_many_without_tenant()
        .col_expr(Column::Quantity, Expr::value(0))
        .exec(&db).await?; // 更新所有租户的记录
}

// 🔷 登录接口：临时禁用租户过滤
{
    let _guard = TenantIgnoreGuard::new();
    let user = User::find_without_tenant()
        .filter(Column::Email.eq("admin@example.com"))
        .one(&db).await?;
}

// 🔷 Database 模式：自动切换数据库连接
let tenant_db = tenant_db(&default_db)?; // 自动获取当前租户的连接
let orders = orders::Entity::find().all(&tenant_db).await?;
```

---

## 🏗️ Summer 框架集成

### 完整项目结构

```rust
// main.rs
use summer_sea_orm_ext::plugin::*;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut app = App::new();

    // 注册插件（SeaOrmPlugin 整合数据库连接 + SQL 日志 + 字段填充 + ID 生成）
    app.add_plugin(SeaOrmPlugin::new())
        .add_plugin(TenantPlugin::new())

        // 注册组件（提供自定义逻辑）
        .add_component(TenantIdProviderComponent::new(Arc::new(MyTenantIdProvider)))
        .add_component(TenantDatabaseProviderComponent::new(Arc::new(MyTenantDatabaseProvider)))
        .add_component(FieldFillHandlerComponent::new(Arc::new(MyFieldFillHandler)))

        .run().await?;

    Ok(())
}
```

### SeaOrmPlugin（统一插件）

`SeaOrmPlugin` 整合了数据库连接创建、SQL 日志、字段填充、ID 生成四大功能：

- 从 `[summer-sea-orm-ext]` 配置创建数据库连接
- 注册 `DatabaseConnection` 和 `SeaOrmExtConnection` 组件
- `enable_sql_log = true` 开启完整 SQL 日志（含参数值注入 + 参数列表）
- 自动注册字段填充处理器和 ID 生成器

### 自动数据库切换（summer-web）

`TenantPlugin` 自动注册中间件，每个请求自动设置租户上下文：

```rust
// Handler 中使用 TenantDb extractor，自动获取正确的数据库连接
async fn list_orders(TenantDb(db): TenantDb) -> Result<Json<Vec<Order>>, StatusCode> {
    // db 是 SeaOrmExtConnection，支持完整 SQL 日志
    let orders = OrderEntity::find()
        .all(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(orders))
}
```

### 配置示例

```toml
# 数据库配置（连接 + SQL 日志 + 字段填充）
[summer-sea-orm-ext]
uri = "postgres://user:pass@localhost:5432/mydb"
enable_sql_log = true
min_connections = 1
max_connections = 10
connect_timeout = 30000
idle_timeout = 600000
acquire_timeout = 30000
default_user = "system"

# 多租户配置
[summer-sea-orm-ext-tenant]
enabled = true
mode = "database"
database_source = "config"
default_tenant_id = "1"

[[summer-sea-orm-ext-tenant.databases]]
tenant_id = "1876543210000000001"
[summer-sea-orm-ext-tenant.databases.database]
url = "postgres://user:pass@localhost:5432/tenant_1"
max_connections = 20

[[summer-sea-orm-ext-tenant.databases]]
tenant_id = "1876543210000000002"
[summer-sea-orm-ext-tenant.databases.database]
url = "postgres://user:pass@localhost:5432/tenant_2"
max_connections = 20
```

### TenantIdProvider 实现

```rust
use summer_sea_orm_ext::{TenantIdProvider, TenantDatabaseProvider};
use summer_sea_orm_ext::sea_orm::ConnectOptions;
use summer_sea_orm_ext::sea_query::Value;
use std::collections::HashMap;

// 🔷 从请求上下文获取租户 ID（如 JWT、Header）
struct HttpTenantIdProvider;
impl TenantIdProvider for HttpTenantIdProvider {
    fn get_tenant_id(&self) -> Option<Value> {
        some_tenant_id_from_context()
    }
}

// 🔷 Database 模式下，提供租户数据库配置
struct ConfigTenantDatabaseProvider;
impl TenantDatabaseProvider for ConfigTenantDatabaseProvider {
    fn provide(&self) -> HashMap<Value, ConnectOptions> {
        let mut map = HashMap::new();
        map.insert(Value::String(Some("1".to_string())), ConnectOptions::new("postgres://.../tenant_1"));
        map.insert(Value::String(Some("2".to_string())), ConnectOptions::new("postgres://.../tenant_2"));
        map
    }
}
```

---

## 🔗 Re-export 机制

`summer-sea-orm-ext` 重新导出了所有依赖 crate，使用时只需引入一个依赖：

| Re-export | 条件 | 说明 |
|-----------|------|------|
| `sea_orm` | 始终可用 | SeaORM 核心 |
| `sea_query` | 始终可用 | SeaQuery 核心 |
| `summer` | `summer` feature | Summer 框架 |
| `summer_web` | `summer-web` feature | Summer Web 框架 |
| `tower` | `summer-web` feature | Tower 中间件 |

```rust
// 无需单独引入 sea-orm、summer-web 等
use summer_sea_orm_ext::sea_orm::EntityTrait;
use summer_sea_orm_ext::summer_web::SomeType;
use summer_sea_orm_ext::{DbConn, SeaOrmExtConnection, SoftDelete};
```

---

## 📝 SQL 日志

### SeaOrmExtConnection

`SeaOrmExtConnection` 是 `DatabaseConnection` 的包装器，拦截所有 SQL 执行并打印**完整 SQL + 参数列表**：

- 禁用 sqlx 不完整日志（`sqlx_logging(false)`），sqlx 自带的日志只有占位符 `?`/`$1`，不含参数值
- 使用 `sea_query::inject_parameters` 将参数值注入 SQL，生成完整可执行语句
- 同时打印独立参数列表，方便调试
- `enable_sql_log` 控制开关，运行时可动态切换

**日志输出示例**：

```
[summer-sea-orm-ext SQL] SELECT "order"."id", "order"."product_name", "order"."quantity" FROM "order" WHERE "order"."is_deleted" = 0 AND "order"."tenant_id" = '1'
[summer-sea-orm-ext Params] [Int(Some(0)), String(Some("1"))]
```

```rust
use summer_sea_orm_ext::{DbConn, enable_sql_log, disable_sql_log};

// DbConn = SeaOrmExtConnection
#[inject(component)]
db: DbConn,

// 动态开关
enable_sql_log();   // 开启
disable_sql_log();  // 关闭
```

### 租户连接与 SQL 日志

所有租户连接内部均以 `SeaOrmExtConnection` 存储，每个租户拥有独立的 `SeaOrmExtConnection` 实例，SQL 日志自动生效：

```rust
// ConnectionStore 内部存储 SeaOrmExtConnection
// 公开 API 返回 DatabaseConnection（向后兼容）
let conn: DatabaseConnection = store.get(&tenant_id).unwrap();

// 直接获取 SeaOrmExtConnection（支持完整 SQL 日志）
let ext_conn: SeaOrmExtConnection = store.get_ext(&tenant_id).unwrap();
```

---

## 📄 分页查询

```rust
use summer_sea_orm_ext::{Pagination, PaginationExt, Page};

// 基础分页
let pagination = Pagination { page: 0, size: 20, one_indexed: false };
let page: Page<MyModel> = MyEntity::find().page(&db, &pagination).await?;

// Web 自动分页（summer-web feature）
// 从查询参数自动解析 page/size
async fn list_orders(pagination: Pagination, TenantDb(db): TenantDb) -> Result<Json<Page<Order>>> {
    let page = OrderEntity::find().page(&db, &pagination).await?;
    Ok(Json(page))
}
```

### SeaOrmWebConfig

```toml
[summer-sea-orm-ext-web]
one_indexed = false
default_page_size = 20
max_page_size = 2000
```

---

## 🎯 派生宏参考

| 宏 | 功能 | 生成的字段 |
|----|------|-----------|
| `DeriveAutoFill` | 自动填充 | `created_by`, `updated_by` 等 |
| `DeriveSoftDelete` | 软删除 | `delete_many_soft()`, `find_with_deleted()` 等 |
| `DeriveTenant` | 多租户 | `find_without_tenant()`, 自动 `WHERE tenant_id=?` |
| `DeriveAutoFillSoftDeleteTenant` | 全功能 | 以上全部 |

### 生成的 CRUD 方法

| 方法 | 说明 |
|------|------|
| `insert()` | 自动填充字段 |
| `update()` | 自动填充 `updated_by` |
| `delete()` | 转为软删除 |
| `find()` | 自动过滤 `is_deleted=0` + `tenant_id=?` |
| `find_by_id()` | 按主键查询，自动双重过滤 |
| `find_with_deleted()` | 查询所有（含已删除） |
| `find_without_tenant()` | 跳过租户过滤（保留软删除） |
| `update_many()` | 批量更新，自动叠加 `WHERE tenant_id=?` |
| `delete_many()` | 批量删除，自动叠加 `WHERE tenant_id=?` |
| `update_many_without_tenant()` | 批量更新（跨租户，运维场景） |
| `delete_many_without_tenant()` | 批量删除（跨租户，运维场景） |
| `insert_many_with_fill()` | 批量插入（单条 SQL） |
| `update_many_with_fill()` | 批量更新（自动带租户 WHERE） |
| `delete_many_soft()` | 批量软删除（单条 SQL，自动带租户 WHERE） |

---

## 🔧 ID 生成器

```rust
// 1. 默认自增 ID
summer_sea_orm_ext::set_id_generator(Box::new(DefaultIdGenerator::default()));

// 2. UUID
summer_sea_orm_ext::set_id_generator(Box::new(UuidIdGenerator::new()));

// 3. Snowflake 分布式 ID
summer_sea_orm_ext::set_id_generator(Box::new(SnowflakeIdGenerator::new(1)));

// 4. 自定义
struct MyGenerator;
impl IdGenerator for MyGenerator {
    fn generate(&self) -> Value { ... }
    fn generate_for_type(&self, entity: &str, field: &str, type_: &str) -> Option<Value> {
        // 智能类型适配
    }
}
summer_sea_orm_ext::set_id_generator(Box::new(MyGenerator));
```

---

## 📊 测试覆盖

| 测试套件 | 测试数 | 覆盖范围 |
|---------|--------|----------|
| `crud_tests.rs` | 13 | 单条 CRUD、自动填充、字符串主键、UUID/Snowflake ID |
| `macro_tests.rs` | 5 | 批量插入/更新/软删除 |
| `integration_tests.rs` | 32 | 多租户隔离、TenantIdProvider、SQL 日志、SeaOrmExtConnection、update_many/delete_many 租户过滤 |
| `unit_tests.rs` | 61 | 租户过滤、软删除、守卫模式、ConnectionStore |
| `lib.rs` | 3 | 分页逻辑 |

**总计：114 个测试，100% 通过**

```bash
cargo test --workspace --features full
```

---

## 📚 文档

- [中文文档 (WIP)](https://summer-rs.github.io)
- [API 文档](https://docs.rs/summer-sea-orm-ext)
- [Summer 框架](https://summer-rs.github.io/zh/)

---

## 🤝 贡献

欢迎提交 Issue 和 PR！

---

## 📄 License

MIT
