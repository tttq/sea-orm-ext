# sea-orm-ext

<div align="center">

![logo](https://img.shields.io/badge/sea--orm--ext-Enterprise%20Extension-blue?style=for-the-badge)

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.81+-blue.svg?style=for-the-badge)](https://www.rust-lang.org)
[![crates.io](https://img.shields.io/badge/crates.io-v0.0.3-orange.svg?style=for-the-badge)](https://crates.io/crates/sea-orm-ext)
[![docs.rs](https://img.shields.io/badge/docs.rs-latest-blue.svg?style=for-the-badge)](https://docs.rs/sea-orm-ext)
[![Test Status](https://img.shields.io/badge/tests-109%20passed-green?style=for-the-badge)](#测试覆盖)

> ⚡ SeaORM 非侵入式企业级扩展 — 一行注解开启自动填充、软删除、多租户隔离

</div>

---

## ✨ 为什么选择 sea-orm-ext？

| 特性 | 说明 | 对比 MyBatis-Plus |
|------|------|-------------------|
| **🔄 自动填充** | INSERT/UPDATE 时自动填充 `created_by`、`updated_by` 等审计字段 | 对应 `MetaObjectHandler` |
| **🛡️ 软删除** | DELETE 自动转为逻辑删除，自动过滤已删除记录 | 对应 `@TableLogic` |
| **🏢 多租户** | Table/Database 两种隔离模式，CRUD 全自动租户过滤 | 对应 `@MultiTenant` |
| **⚡ Summer 集成** | 声明式配置 + 自动数据库切换，开箱即用 | 无直接对应 |
| **📝 SQL 日志** | 打印完整 SQL（参数值注入）+ 独立参数列表，调试无忧 | 对应 `log-impl:2.x` |
| **📄 分页查询** | Web 友好的分页扩展，自动从请求参数解析分页信息 | 对应 `PageHelper` |

> 🔑 **核心设计理念**：所有功能通过派生宏和 Trait 实现，**非侵入式**，不污染原有 SeaORM API

---

## 📦 安装

```toml
[dependencies]
# 默认：启用 runtime-tokio-native-tls（与 summer-sea-orm 一致）
sea-orm-ext = "0.0.3"

# 指定数据库驱动
sea-orm-ext = { version = "0.0.3", features = ["postgres"] }
sea-orm-ext = { version = "0.0.3", features = ["mysql"] }
sea-orm-ext = { version = "0.0.3", features = ["sqlite"] }

# 启用全部功能
sea-orm-ext = { version = "0.0.3", features = ["full"] }
```

### Feature Flags

与 `summer-sea-orm` 完全对齐的 features：

| Feature | 说明 | 依赖 |
|---------|------|------|
| `default` | `sea-orm/runtime-tokio-native-tls` | sea-orm 内置 |
| `mysql` | MySQL 驱动 | sea-orm/sqlx-mysql |
| `postgres` | PostgreSQL 驱动 | sea-orm/sqlx-postgres |
| `sqlite` | SQLite 驱动 | sea-orm/sqlx-sqlite |
| `rustls` | 使用 rustls 替代 native-tls | sea-orm/runtime-tokio-rustls |
| `with-web` | Summer Web 集成（别名） | summer-web, tower |
| `with-web-openapi` | OpenAPI 分页文档（别名） | summer-web/openapi |

sea-orm-ext 额外提供的 features：

| Feature | 说明 | 依赖 |
|---------|------|------|
| `summer` | Summer 框架插件集成 | summer |
| `summer-web` | axum 自动租户切换（TenantLayer + TenantDb） | summer-web, tower |
| `summer-web-openapi` | OpenAPI 分页参数文档支持 | summer-web/openapi |
| `runtime-tokio` | Tokio 异步运行时支持 | tokio |
| `full` | 启用全部功能 | summer, summer-web, runtime-tokio |

---

## 🚀 快速开始

### 1. 定义实体（3 行注解 = 全功能）

```rust
use sea_orm::entity::prelude::*;

// 一行注解 = 自动填充 + 软删除 + 多租户
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, DeriveAutoFillSoftDeleteTenant)]
#[sea_orm(table_name = "orders")]
pub struct Model {
    #[sea_orm(primary_key, auto_generate)]
    pub id: i64,

    pub product_name: String,
    pub quantity: i32,

    // 🔽 自动填充字段
    #[sea_orm_ext(insert)]
    pub created_by: Option<String>,
    #[sea_orm_ext(update)]
    pub updated_by: Option<String>,

    // 🔽 软删除字段
    #[soft_delete(default = 0, del = 1)]
    pub is_deleted: i32,

    // 🔽 租户隔离字段
    #[sea_orm_ext(TENANT)]
    pub tenant_id: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
```

### 2. 设置全局处理器（一次配置，永久生效）

```rust
use sea_orm_ext::*;

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
use sea_orm::{ActiveModelTrait, EntityTrait, Set};

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
use sea_orm_ext::plugin::*;
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

- 从 `[sea-orm]` 配置创建数据库连接
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
# sea-orm 配置（数据库连接 + SQL 日志 + 字段填充）
[sea-orm]
uri = "postgres://user:pass@localhost:5432/mydb"
enable_sql_log = true
min_connections = 1
max_connections = 10
connect_timeout = 30000
idle_timeout = 600000
acquire_timeout = 30000
default_user = "system"

# 多租户配置
[sea-orm-ext-tenant]
enabled = true
mode = "database"
database_source = "config"
default_tenant_id = "1"

[[sea-orm-ext-tenant.databases]]
tenant_id = "1876543210000000001"
[sea-orm-ext-tenant.databases.database]
url = "postgres://user:pass@localhost:5432/tenant_1"
max_connections = 20

[[sea-orm-ext-tenant.databases]]
tenant_id = "1876543210000000002"
[sea-orm-ext-tenant.databases.database]
url = "postgres://user:pass@localhost:5432/tenant_2"
max_connections = 20
```

### TenantIdProvider 实现

```rust
use sea_orm_ext::{TenantIdProvider, TenantDatabaseProvider};
use sea_query::Value;
use std::collections::HashMap;
use sea_orm::ConnectOptions;

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

## 📝 SQL 日志

### SeaOrmExtConnection

`SeaOrmExtConnection` 是 `DatabaseConnection` 的包装器，拦截所有 SQL 执行并打印**完整 SQL + 参数列表**：

- 禁用 sqlx 不完整日志（`sqlx_logging(false)`），sqlx 自带的日志只有占位符 `?`/`$1`，不含参数值
- 使用 `sea_query::inject_parameters` 将参数值注入 SQL，生成完整可执行语句
- 同时打印独立参数列表，方便调试
- `enable_sql_log` 控制开关，运行时可动态切换

**日志输出示例**：

```
[sea-orm-ext SQL] SELECT "order"."id", "order"."product_name", "order"."quantity" FROM "order" WHERE "order"."is_deleted" = 0 AND "order"."tenant_id" = '1'
[sea-orm-ext Params] [Int(Some(0)), String(Some("1"))]
```

```rust
use sea_orm_ext::{DbConn, enable_sql_log, disable_sql_log};

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
use sea_orm_ext::{Pagination, PaginationExt, Page};

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
[sea-orm-web]
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
| `insert_many_with_fill()` | 批量插入（单条 SQL） |
| `update_many_with_fill()` | 批量更新 |
| `delete_many_soft()` | 批量软删除（单条 SQL） |

---

## 🔧 ID 生成器

```rust
// 1. 默认自增 ID
sea_orm_ext::set_id_generator(Box::new(DefaultIdGenerator::default()));

// 2. UUID
sea_orm_ext::set_id_generator(Box::new(UuidIdGenerator::new()));

// 3. Snowflake 分布式 ID
sea_orm_ext::set_id_generator(Box::new(SnowflakeIdGenerator::new(1)));

// 4. 自定义
struct MyGenerator;
impl IdGenerator for MyGenerator {
    fn generate(&self) -> Value { ... }
    fn generate_for_type(&self, entity: &str, field: &str, type_: &str) -> Option<Value> {
        // 智能类型适配
    }
}
sea_orm_ext::set_id_generator(Box::new(MyGenerator));
```

---

## 📊 测试覆盖

| 测试套件 | 测试数 | 覆盖范围 |
|---------|--------|----------|
| `crud_tests.rs` | 13 | 单条 CRUD、自动填充、字符串主键、UUID/Snowflake ID |
| `macro_tests.rs` | 5 | 批量插入/更新/软删除 |
| `integration_tests.rs` | 27 | 多租户隔离、TenantIdProvider、SQL 日志、SeaOrmExtConnection |
| `unit_tests.rs` | 61 | 租户过滤、软删除、守卫模式、ConnectionStore |
| `lib.rs` | 3 | 分页逻辑 |

**总计：109 个测试，100% 通过**

```bash
cargo test --workspace --features full
```

---

## 📚 文档

- [中文文档 (WIP)](https://summer-rs.github.io)
- [API 文档](https://docs.rs/sea-orm-ext)
- [Summer 框架](https://summer-rs.github.io/zh/)

---

## 🤝 贡献

欢迎提交 Issue 和 PR！

---

## 📄 License

MIT
