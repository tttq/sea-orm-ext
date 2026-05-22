# SeaORM Ext Summer 插件使用指南

## 快速开始

### 1. 引入依赖

```toml
[dependencies]
sea-orm-ext = { version = "0.2", features = ["full"] }
summer = "0.6.0"
summer-web = "0.6.0"
```

Feature 说明：
- `summer` — 启用 Summer Plugin 集成（TenantPlugin, SeaOrmExtPlugin）
- `summer-web` — 启用 axum 自动租户切换（TenantLayer + TenantDb extractor）
- `runtime-tokio` — 启用 tokio 异步运行时支持
- `full` — 启用所有功能

### 2. 核心 Trait

#### TenantIdProvider — 租户 ID 提供器

用户实现此 trait，提供租户 ID 的获取逻辑：

```rust
use sea_orm_ext::TenantIdProvider;
use sea_query::Value;

struct MyTenantIdProvider;

impl TenantIdProvider for MyTenantIdProvider {
    fn get_tenant_id(&self) -> Option<Value> {
        // 从线程局部存储、JWT、请求上下文等来源获取租户 ID
        Some(Value::String(Some("1".to_string())))
    }
}
```

#### TenantDatabaseProvider — 租户数据库提供器

Database 模式下，用户实现此 trait 返回租户数据库配置：

```rust
use sea_orm_ext::TenantDatabaseProvider;
use sea_orm::ConnectOptions;
use sea_query::Value;
use std::collections::HashMap;

struct MyTenantDatabaseProvider;

impl TenantDatabaseProvider for MyTenantDatabaseProvider {
    fn provide(&self) -> HashMap<Value, ConnectOptions> {
        let mut map = HashMap::new();
        map.insert(Value::String(Some("1".to_string())), ConnectOptions::new("postgres://.../tenant_1"));
        map.insert(Value::String(Some("2".to_string())), ConnectOptions::new("postgres://.../tenant_2"));
        map
    }
}
```

#### FieldFillHandler — 字段自动填充处理器

用户实现此 trait 提供自定义填充逻辑：

```rust
use sea_orm_ext::{FieldFillHandler, FieldFillOperation};
use sea_query::Value;

struct MyFieldFillHandler;

impl FieldFillHandler for MyFieldFillHandler {
    fn fill(&self, _entity_name: &str, field_name: &str, operation: FieldFillOperation) -> Option<Value> {
        match (field_name, operation) {
            ("created_by", FieldFillOperation::Insert) => Some("current_user".into()),
            ("updated_by", FieldFillOperation::Update) => Some("current_user".into()),
            _ => None,
        }
    }
}
```

### 3. 在 Summer 应用中使用

```rust
use sea_orm_ext::plugin::sea_orm_ext::{FieldFillHandlerComponent, SeaOrmExtPlugin, SnowflakeIdGenerator};
use sea_orm_ext::plugin::tenant::{TenantDatabaseProviderComponent, TenantIdProviderComponent, TenantPlugin};
use sea_orm_ext::{FieldFillHandler, FieldFillOperation, TenantDatabaseProvider, TenantIdProvider};
use sea_orm_ext::set_id_generator;
use sea_orm::ConnectOptions;
use sea_query::Value;
use std::collections::HashMap;
use std::sync::Arc;
use summer::plugin::MutableComponentRegistry;
use summer::App;

#[tokio::main]
async fn main() {
    set_id_generator(Box::new(SnowflakeIdGenerator::new(1)));

    let mut app = App::new();
    app.add_plugin(TenantPlugin::new())
        .add_plugin(SeaOrmExtPlugin::new())
        .add_component(TenantIdProviderComponent::new(Arc::new(MyTenantIdProvider)))
        .add_component(TenantDatabaseProviderComponent::new(Arc::new(MyTenantDatabaseProvider)))
        .add_component(FieldFillHandlerComponent::new(Arc::new(MyFieldFillHandler)));

    app.run().await;
}
```

### 4. 自动数据库切换（summer-web feature）

启用 `summer-web` feature 后，`TenantPlugin` 会自动注册 axum 中间件，在每个请求中：
1. 调用 `TenantIdProvider` 获取租户 ID
2. 设置租户上下文
3. Database 模式下自动查找并注入租户数据库连接
4. 请求结束后自动清除租户上下文

Handler 中使用 `TenantDb` extractor 自动获取正确的数据库连接：

```rust
use sea_orm_ext::plugin::tenant_layer::TenantDb;

async fn list_products(TenantDb(db): TenantDb) -> Result<Json<Vec<Product>>, StatusCode> {
    // db 已经是当前租户对应的数据库连接，无需手动切换
    let products = ProductEntity::find().all(&db).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(products))
}
```

## 配置详解

### TOML 配置

```toml
[sea-orm-ext-tenant]
enabled = true
mode = "database"              # "table" 或 "database"
database_source = "config"     # "config" 或 "custom"
default_tenant_id = "1"

# database_source = "config" 时配置
[[sea-orm-ext-tenant.databases]]
tenant_id = "1"
[sea-orm-ext-tenant.databases.database]
url = "postgres://user:pass@localhost:5432/tenant_1_db"
max_connections = 20
min_connections = 5
connect_timeout_secs = 30
acquire_timeout_secs = 30

[sea-orm-ext]
default_user = "system"
enable_sql_log = false
```

### database_source 说明

| 值 | 说明 |
|---|---|
| `config` | 从 TOML 配置文件加载租户数据库连接 |
| `custom` | 通过 `TenantDatabaseProvider` trait 外部实现返回数据库 map |

### 租户 ID 获取优先级

1. **TenantGuard / set_tenant_context()** — 手动设置的租户上下文（最高优先级）
2. **TenantIdProvider** — 通过 trait 外部实现获取
3. **default_tenant_id** — 配置文件中的默认值

## 表隔离 vs 数据库隔离

### 表隔离模式

所有租户共享同一数据库，通过 `tenant_id` 字段区分。宏自动注入租户 ID 和过滤条件。

```toml
[sea-orm-ext-tenant]
enabled = true
mode = "table"
```

### 数据库隔离模式

每个租户拥有独立数据库。通过 `TenantIdProvider` 获取当前租户 ID，自动切换连接。

```toml
[sea-orm-ext-tenant]
enabled = true
mode = "database"
database_source = "custom"
```

## 三个 Summer 插件

| 插件 | 配置前缀 | 功能 |
|---|---|---|
| `TenantPlugin` | `sea-orm-ext-tenant` | 多租户配置 + 租户 ID/数据库提供器注册 + 自动 axum layer |
| `SeaOrmExtPlugin` | `sea-orm-ext` | 字段自动填充 + ID 生成 + SQL 日志 |
| `SoftDeletePlugin` | — | 不需要，使用宏即自动开启 |
