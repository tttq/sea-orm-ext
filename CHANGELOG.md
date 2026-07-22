# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.0.1] - 2026-05-22

### Added

- **项目重命名**：从 `sea-orm-ext` 重命名为 `summer-sea-orm-ext`，更清晰地表达与 Summer 框架的深度集成关系
- **Re-export 机制**：重新导出 `sea_orm`、`sea_query`、`summer`（cfg feature）、`summer_web`（cfg feature）、`tower`（cfg feature），用户只需引入 `summer-sea-orm-ext` 一个依赖
- **Feature 透传**：完整透传 `sea-orm` 和 `summer-web` 的所有 features，通过配置 `summer-sea-orm-ext` 的 features 即可控制所有依赖
  - sea-orm 类型支持：`with-json`、`with-chrono`、`with-rust_decimal`、`with-uuid`、`with-time`、`with-bigdecimal`、`with-ipnetwork`、`with-mac_address`、`with-arrow`
  - sea-orm PostgreSQL 扩展：`postgres-array`、`postgres-use-serial-pk`、`postgres-vector`、`json-array`
  - sea-orm SQLite/MariaDB 扩展：`sqlite-use-returning-for-3_35`、`sqlite-no-row-value-before-3_15`、`mariadb-use-returning`
  - sea-orm 其他：`debug-print`、`proxy`、`rbac`、`schema-sync`、`seaography`、`tracing-spans`、`entity-registry`
  - summer-web：`http2`、`multipart`、`openapi`、`openapi-redoc`、`openapi-scalar`、`openapi-swagger`、`socket-io`、`ws`
- **SeaOrmExtConnection**：`DatabaseConnection` 的包装器，实现 `ConnectionTrait`、`StreamTrait`、`TransactionTrait`、`Deref<Target=DatabaseConnection>`，拦截所有 SQL 执行并打印完整语句（含参数值注入 + 参数列表）
- **SeaOrmExtConnection::close()**：支持关闭内部数据库连接
- **ConnectionStore::get_ext() / ConnectionStore::insert_ext()**：直接操作 `SeaOrmExtConnection` 的新方法，公开 API `get()` / `insert()` 保持 `DatabaseConnection` 签名向后兼容
- **分页模块 `pagination`**：
  - `Pagination` 结构体：`page`、`size`、`one_indexed`
  - `Page<T>` 结构体：`content`、`total_elements`、`total_pages`、`is_first()`、`is_last()`、`map()`
  - `PaginationExt` trait：`Select` 和 `Selector` 的分页查询扩展
  - `OrmError` / `PageResult<T>` 错误类型
  - `SeaOrmWebConfig`（summer-web feature）：从请求参数自动解析分页
  - `summer-web-openapi` feature：OpenAPI 分页参数文档支持
- **合并插件**：将 `SeaOrmPlugin`（数据库连接）和 `SeaOrmExtPlugin`（字段填充/ID生成/SQL日志）合并为统一的 `SeaOrmPlugin`
- **SQL 日志增强**：`log_statement` 使用 `sea_query::inject_parameters` 将参数值注入 SQL 生成完整可执行语句，同时打印独立参数列表 `[summer-sea-orm-ext Params]`
- **派生宏**：
  - `DeriveAutoFill`、`DeriveSoftDelete`、`DeriveAutoFillSoftDelete`
  - `DeriveTenant`、`DeriveAutoFillTenant`、`DeriveAutoFillSoftDeleteTenant`
  - 宏属性从 `#[sea_orm_ext(...)]` 更名为 `#[summer_sea_orm_ext(...)]`
- **自动字段填充 (Auto-Fill)**：INSERT/UPDATE 时自动填充审计字段，通过 `FieldFillHandler` trait 外部实现
- **软删除 (Soft-Delete)**：DELETE 操作拦截为逻辑删除，`find()` 自动过滤已删除记录
- **多租户 (Multi-Tenant)**：Table/Database 两种隔离模式
  - `TenantIdProvider` trait：框架无关的租户 ID 提供器
  - `TenantDatabaseProvider` trait：Database 模式下返回租户数据库配置
  - 租户 ID 三级优先级：TenantGuard → TenantIdProvider → default_tenant_id
- **Summer 插件集成**：
  - `SeaOrmPlugin`：数据库连接 + SQL 日志 + 字段填充 + ID 生成
  - `TenantPlugin`：多租户配置 + 租户 ID/数据库提供器注册 + 自动 axum layer
- **自动数据库切换（summer-web feature）**：
  - `TenantLayer`：axum tower 中间件，自动设置租户上下文
  - `TenantDb`：axum extractor，自动提取正确的数据库连接
- **ID 生成器**：`DefaultIdGenerator`、`UuidIdGenerator`、`TypedIdGenerator`、`SnowflakeIdGenerator`
- **`anyhow` 依赖**：与 summer-sea-orm 一致
- **`DatabaseConfig` 新增字段**：`enable_logging`、`idle_timeout_secs`
- **多租户 WHERE 自动注入覆盖**：为 `Entity::update_many()` 和 `Entity::delete_many()` 添加宏覆盖实现，开启字段隔离多租户（`TenantMode::Table`）时自动在 WHERE 条件中叠加 `tenant_id = ?`，防止跨租户更新/删除
  - 新增 `Entity::update_many_without_tenant()` / `Entity::delete_many_without_tenant()` 方法用于跨租户运维场景
  - 移除 `expand_batch_update_method` / `expand_batch_delete_method` 中冗余的 `tenant_where_filter`（由覆盖后的 `update_many()` 统一注入）
  - 标记 `apply_tenant_condition` / `apply_tenant_delete_condition` 为 `#[deprecated]`
  - 安全保障：租户上下文未设置时 `require_tenant_id()` 返回 `Value::Int(None)`（SQL NULL），`WHERE tenant_id = NULL` 永远为 false，保证安全失败

### Changed

- **项目名称**：`sea-orm-ext` → `summer-sea-orm-ext`
- **Rust 模块名**：`sea_orm_ext` → `summer_sea_orm_ext`
- **过程宏 crate**：`sea-orm-ext-macros` → `summer-sea-orm-ext-macros`
- **宏属性**：`#[sea_orm_ext(...)]` → `#[summer_sea_orm_ext(...)]`
- **配置前缀**：`[sea-orm-ext]` → `[summer-sea-orm-ext]`，`[sea-orm-ext-tenant]` → `[summer-sea-orm-ext-tenant]`
- **SQL 日志前缀**：`[sea-orm-ext SQL]` → `[summer-sea-orm-ext SQL]`
- **`DbConn` 类型**：`DbConn` = `SeaOrmExtConnection`，注入即支持完整 SQL 日志
- **统一配置结构**：`SeaOrmConfig` 整合 `uri`、`enable_sql_log`、`min/max_connections`、`connect/idle/acquire_timeout`、`default_user`、`fill_rules`
- **租户连接存储升级**：`ConnectionStore` 内部存储从 `DatabaseConnection` 改为 `SeaOrmExtConnection`
- **禁用 sqlx 不完整日志**：所有连接创建时统一设置 `sqlx_logging(false)`
- **Feature flags 对齐**：`default` 改为 `sea-orm/runtime-tokio-native-tls`，完整透传 sea-orm 和 summer-web 的所有 features

### Removed

- **`SeaOrmExtPlugin`**：已合并到 `SeaOrmPlugin`
- **`plugin/sea_orm.rs`**：独立插件文件已删除
- **`SeaOrmConfig`（config.rs 中的旧版本）**：已迁移到 `plugin/summer_sea_orm_ext.rs`
- **`summer_connection` 模块**：连接创建逻辑已整合到 `SeaOrmPlugin::connect()`
- **`LoggingConnection`**：已重命名为 `SeaOrmExtConnection`

### Fixed

- **SQL 日志不打印**：修复 `ConnectOptions` 未调用 `sqlx_logging()` 导致 SQLx 层面日志完全禁用的问题
- **SQL 日志不完整**：sqlx 自带的 SQL 日志不包含参数值（只有占位符 `?`/`$1`），现在通过 `sea_query::inject_parameters` 将参数值注入 SQL，打印完整可执行语句

### Dependencies

- sea-orm 2.0.0-rc.37
- sea-query 1.0.0-rc.33
- summer 0.5.0-rc.1 (optional)
- summer-web 0.5.0-rc.1 (optional)
- tower 0.5 (optional)
