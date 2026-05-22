# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.0.3] - 2026-05-22

### Changed

- **合并插件**：将 `SeaOrmPlugin`（数据库连接）和 `SeaOrmExtPlugin`（字段填充/ID生成/SQL日志）合并为统一的 `SeaOrmPlugin`，一个插件完成所有初始化
- **重命名 `LoggingConnection` → `SeaOrmExtConnection`**：更清晰地表达扩展连接的语义
- **`DbConn` 类型变更**：`DbConn` 从 `sea_orm::DatabaseConnection` 改为 `SeaOrmExtConnection`，注入即支持完整 SQL 日志
- **统一配置结构**：`SeaOrmConfig` 整合 `uri`、`enable_sql_log`、`min/max_connections`、`connect/idle/acquire_timeout`、`default_user`、`fill_rules`，配置前缀为 `[sea-orm]`
- **租户连接存储升级**：`ConnectionStore` 内部存储从 `DatabaseConnection` 改为 `SeaOrmExtConnection`，每个租户拥有独立的 `SeaOrmExtConnection` 实例，SQL 日志自动生效
- **禁用 sqlx 不完整日志**：所有连接创建时统一设置 `sqlx_logging(false)`，避免打印不含参数值的 SQL
- **SQL 日志增强**：`log_statement` 使用 `sea_query::inject_parameters` 将参数值注入 SQL 生成完整可执行语句，同时打印独立参数列表 `[sea-orm-ext Params]`
- **Feature flags 对齐 summer-sea-orm**：`default` 改为 `sea-orm/runtime-tokio-native-tls`，新增 `mysql`/`postgres`/`sqlite`/`rustls`/`with-web`/`with-web-openapi` features

### Added

- **`SeaOrmExtConnection`**：`DatabaseConnection` 的包装器，实现 `ConnectionTrait`、`StreamTrait`、`TransactionTrait`、`Deref<Target=DatabaseConnection>`，拦截所有 SQL 执行并打印完整语句（含参数值注入 + 参数列表）
- **`SeaOrmExtConnection::close()`**：支持关闭内部数据库连接
- **`ConnectionStore::get_ext()` / `ConnectionStore::insert_ext()`**：直接操作 `SeaOrmExtConnection` 的新方法，公开 API `get()` / `insert()` 保持 `DatabaseConnection` 签名向后兼容
- **分页模块 `pagination`**：
  - `Pagination` 结构体：`page`、`size`、`one_indexed`
  - `Page<T>` 结构体：`content`、`total_elements`、`total_pages`、`is_first()`、`is_last()`、`map()`
  - `PaginationExt` trait：`Select` 和 `Selector` 的分页查询扩展
  - `OrmError` / `PageResult<T>` 错误类型
  - `SeaOrmWebConfig`（summer-web feature）：从请求参数自动解析分页，支持 `one_indexed`、`max_page_size`、`default_page_size`
  - `summer-web-openapi` feature：OpenAPI 分页参数文档支持
- **`anyhow` 依赖**：与 summer-sea-orm 一致
- **`DatabaseConfig` 新增字段**：`enable_logging`、`idle_timeout_secs`
- **Feature flags**：`mysql`、`postgres`、`sqlite`、`rustls`、`with-web`、`with-web-openapi`（与 summer-sea-orm 完全对齐）

### Removed

- **`SeaOrmExtPlugin`**：已合并到 `SeaOrmPlugin`
- **`plugin/sea_orm.rs`**：独立插件文件已删除
- **`SeaOrmConfig`（config.rs 中的旧版本）**：已迁移到 `plugin/sea_orm_ext.rs`
- **`summer_connection` 模块**：连接创建逻辑已整合到 `SeaOrmPlugin::connect()`

### Fixed

- **SQL 日志不打印**：修复 `ConnectOptions` 未调用 `sqlx_logging()` 导致 SQLx 层面日志完全禁用的问题
- **SQL 日志不完整**：sqlx 自带的 SQL 日志不包含参数值（只有占位符 `?`/`$1`），现在通过 `sea_query::inject_parameters` 将参数值注入 SQL，打印完整可执行语句，同时打印独立参数列表

## [0.0.2] - 2026-05-20

### Changed

- 重构多租户 Database 模式：支持 `config`（配置文件）和 `custom`（TenantDatabaseProvider trait）两种数据源
- 重构自动填充插件：通过 `FieldFillHandler` trait 外部实现获取填充值，无需再实现 Summer Plugin
- 移除 SoftDeletePlugin，使用宏即自动开启软删除
- 统一 `TenantIdProvider` trait 定义到 `src/tenant.rs`，消除与 `plugin/tenant.rs` 的重复定义
- `TenantPlugin::build()` 中调用 `set_tenant_id_provider()` 注册到全局存储
- 新增 `summer-web` feature：TenantLayer 自动设置租户上下文 + TenantDb extractor 自动获取数据库连接
- 移除所有 Web 框架（axum/actix/rocket）独立集成模块

### Added

- `TenantDatabaseProvider` trait：Database 模式下返回租户数据库配置 map
- `TenantIdProviderComponent` / `TenantDatabaseProviderComponent` / `FieldFillHandlerComponent`：Summer 组件注册
- `tenant_db()` / `tenant_db_for()` 便捷函数
- `TenantLayer` + `TenantDb`（summer-web feature）：自动数据库切换
- 8 个新增测试覆盖 TenantIdProvider、tenant_db、TenantDatabaseProvider

## [0.0.1] - 2026-05-20

### Added

- **自动字段填充 (Auto-Fill)**：INSERT/UPDATE 时自动填充审计字段（created_by、updated_by 等），通过 `FieldFillHandler` trait 外部实现
- **软删除 (Soft-Delete)**：将 DELETE 操作拦截为逻辑删除，`find()` 自动过滤已删除记录，`find_with_deleted()` 查询全部
- **多租户 (Multi-Tenant)**：支持 Table 隔离和 Database 隔离两种模式
  - `TenantIdProvider` trait：框架无关的租户 ID 提供器，用户外部实现
  - `TenantDatabaseProvider` trait：Database 模式下返回租户数据库配置 map
  - `database_source` 配置：`config`（从配置文件加载）或 `custom`（通过 trait 外部实现）
  - `tenant_db()` / `tenant_db_for()` 便捷函数自动获取正确的数据库连接
  - 租户 ID 三级优先级：TenantGuard → TenantIdProvider → default_tenant_id
- **Summer 插件集成**：
  - `TenantPlugin`：多租户配置 + 租户 ID/数据库提供器注册
  - `SeaOrmExtPlugin`：字段自动填充 + ID 生成 + SQL 日志
  - `FieldFillHandlerComponent`：通过 `app.add_component()` 注册自定义填充处理器
  - `TenantIdProviderComponent`：通过 `app.add_component()` 注册租户 ID 提供器
  - `TenantDatabaseProviderComponent`：通过 `app.add_component()` 注册租户数据库提供器
- **自动数据库切换 (summer-web feature)**：
  - `TenantLayer`：axum tower 中间件，自动设置租户上下文并注入数据库连接
  - `TenantDb`：axum extractor，自动提取正确的数据库连接
  - 请求结束后自动清除租户上下文
- **SQL 日志**：`LoggingConnection` 包装器 + `enable_sql_log()`/`disable_sql_log()` 开关
- **ID 生成器**：`DefaultIdGenerator`、`UuidIdGenerator`、`TypedIdGenerator`、`SnowflakeIdGenerator`
- **派生宏**：
  - `DeriveAutoFill`、`DeriveSoftDelete`、`DeriveAutoFillSoftDelete`
  - `DeriveTenant`、`DeriveAutoFillTenant`、`DeriveAutoFillSoftDeleteTenant`
- **SeaORM 2.0 适配**：适配 sea-orm 2.0.0-rc.37 + sea-query 1.0.0-rc.33 的所有破坏性变更

### Dependencies

- sea-orm 2.0.0-rc.37
- sea-query 1.0.0-rc.33
- summer 0.5.0-rc.1 (optional)
- summer-web 0.5.0-rc.1 (optional)
- tower 0.5 (optional)
