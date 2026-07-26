# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.0.3] - 2026-07-26

### Added

- **`SeaOrmExtConnection` 自动路由**：业务层零样板代码，框架在 SQL 执行时自动路由到正确的数据库连接
  - 新增 `effective_connection()` 方法：根据当前租户上下文自动选择底层 `DatabaseConnection`
  - 改写 `ConnectionTrait` / `StreamTrait` / `TransactionTrait` 实现，所有 SQL 操作自动走 `effective_connection()`
  - 行为矩阵：
    - `TenantIgnoreGuard` 生效 → 主库（用于查询全局表，如 `auth_sys_tenant`）
    - database 模式 + 已登录 → 租户专属库（不注入 WHERE tenant_id）
    - table 模式 / 未登录 → 主库 + WHERE tenant_id 注入
- **`TenantIdProvider::get_tenant_mode()` 运行时租户模式**：支持 JWT token 携带的 `tenantMode` 字段，运行时动态决定隔离模式
  - 优先级：`TenantIdProvider.get_tenant_mode()` > 全局 TOML 配置 `mode`
  - 默认返回 `None`，已有实现无需修改，完全向后兼容
  - 适用于混合模式项目：全局 `table` + 部分 `database` 租户
- **`get_effective_tenant_mode()` 函数**：统一获取当前生效的租户模式
  - provider 返回无效字符串时记录警告并回退到全局配置
  - 供 `is_tenant_enforced()` 和 `SeaOrmExtConnection::effective_connection()` 共同使用
- **`get_tenant_database_for_current()` 函数**：自动路由辅助函数
  - 检查 `TenantIgnoreGuard` → 检查 effective mode → 查询租户连接
  - 供 `SeaOrmExtConnection::effective_connection()` 内部调用
- **`get_database_for_tenant_unchecked()` 函数**：不检查 mode 的租户连接查询（内部使用）
  - 与 `get_database_for_tenant()` 区别：不检查全局 mode，仅按 tenant_id 查找连接缓存
  - 适用场景：运行时 provider 指定为 database 模式但全局配置为 table 模式时仍能正确路由
- **6 个自动路由测试**：覆盖 `get_effective_tenant_mode` 优先级、`is_tenant_enforced` 行为、`SeaOrmExtConnection` 自动路由、`TenantIgnoreGuard` 走主库、table 模式走主库等场景
- **2 个混合模式 fallback 测试**：覆盖 `get_database_for_tenant_unchecked` 在 store 未初始化时返回 `Ok(None)` 走 fallback、跳过 mode 检查等场景
- **`DynamicTenantPlugin` 支持混合模式启动**：不再强制要求全局 `mode = "database"`，全局 table 模式下插件也会启动并初始化 `ConnectionStore`，配合运行时 `TenantIdProvider.get_tenant_mode()` 实现"全局 table + 部分租户 database"的混合模式

### Changed

- 版本号从 `0.0.2` 升级到 `0.0.3`
- **`is_tenant_enforced()` 重写**：基于 `get_effective_tenant_mode()` 判断，支持运行时 provider 模式
  - database 模式租户的查询不注入 WHERE tenant_id（租户库已物理隔离）
  - 无 provider 时回退原逻辑（按全局配置）
- **`get_database_for_tenant_unchecked()` 容错策略**：store 未初始化时从返回 `Err` 改为返回 `Ok(None)` 走 fallback，与 `effective_connection` 的容错策略一致
  - 适用场景：`DynamicTenantPlugin` 未启用或启动失败、测试环境未注册 store
  - 与 `get_database_for_tenant()` 的区别：后者保留 `Err` 行为，因为那是显式调用方期望严格模式检查
- **`StreamTrait::stream_raw` 限制说明**：流式查询不自动路由到租户库（生命周期约束），需手动用 `tenant_db()` 获取连接
- **README 文档更新**：
  - 顶部 badge 版本号 0.0.2 → 0.0.3，测试数 154 → 162
  - 特性对比表新增"🔌 自动路由"项
  - 新增 4.1 章节"自动路由（v0.0.3+ 新增）"详细说明行为矩阵和混合模式最佳实践
  - 多租户场景示例中区分"自动路由"与"手动切换"两种方式

### Compatibility

- **完全向后兼容**：
  - `TenantIdProvider` trait 新增方法有默认实现 `None`
  - `is_tenant_enforced()` 在 provider 返回 `None` 时回退原逻辑
  - `SeaOrmExtConnection::effective_connection()` 在非租户场景或 table 模式下返回 `self.inner`，行为与原来一致
  - 手动切换模式（`tenant_db()` / `TenantIgnoreGuard`）保留不变

---

## [0.0.2] - 2026-07-22

### Added

- **多租户 WHERE 自动注入覆盖**：为 `Entity::update_many()` 和 `Entity::delete_many()` 添加宏覆盖实现，开启字段隔离多租户（`TenantMode::Table`）时自动在 WHERE 条件中叠加 `tenant_id = ?`，防止跨租户更新/删除
  - 新增 `Entity::update_many_without_tenant()` / `Entity::delete_many_without_tenant()` 方法用于跨租户运维场景
  - 移除 `expand_batch_update_method` / `expand_batch_delete_method` 中冗余的 `tenant_where_filter`（由覆盖后的 `update_many()` 统一注入）
  - 安全保障：租户上下文未设置时 `require_tenant_id()` 返回 `Value::Int(None)`（SQL NULL），`WHERE tenant_id = NULL` 永远为 false，保证安全失败
- **动态租户管理（`DynamicTenantPlugin`）**：基于主库查询的动态租户连接管理，**仅在 `TenantMode::Database` 下生效**
  - 新增 `DynamicTenantConfigProvider` trait：用户实现 `load_all()` / `load_one()` 从主库查询租户连接配置，框架不关心租户表结构
  - 新增 `TenantManager`：启动时自动加载所有租户、建立连接并缓存；运行时支持 `add_tenant()` / `update_tenant()` / `remove_tenant()` / `refresh_cache()` 动态同步缓存
  - 新增 `TenantConnectionConfig`：框架定义的连接配置结构，与用户 Entity 解耦
  - 后台健康检查任务（`runtime-tokio` feature）：定时 ping 所有租户库，连续失败达阈值自动从缓存移除
  - 新增 `DynamicTenantPlugin` Summer 插件：自动初始化、注册 `TenantManagerComponent` 供业务层依赖注入
  - 新增 `[summer-sea-orm-ext-dynamic-tenant]` TOML 配置段：`enabled` / `health_check_interval_secs` / `health_check_failure_threshold` / `auto_remove_on_failure`
- **租户 ID 加解密（`TenantIdCodec` trait）**：支持前端传入加密后的 tenant_id，后端解密后再使用
  - 新增 `TenantIdCodec` trait：`decrypt()` / `encrypt()` 两个方法
  - 新增 `set_tenant_id_codec()` / `get_tenant_id_codec()` / `clear_tenant_id_codec()` 全局注册函数
  - `TenantLayer` 中间件集成：获取到原始 tenant_id 后，若注册了 codec 则先解密再设置上下文；解密失败时记录日志并跳过上下文设置（使用默认库）
  - 未注册 codec 时直接使用原始 tenant_id，不影响现有行为
- **`#[ignore_tenant]` 属性宏**：标记 async handler 函数，自动包入 `TenantIgnoreGuard`，跳过当前请求的租户 WHERE 过滤
  - 仅对 `async fn` 有效，非 async 函数编译报错
  - 函数返回时 guard 自动 drop，恢复租户过滤
  - 适用于跨租户聚合查询、系统配置读取、健康检查等场景
- **`TenantIgnoreGuard` 计数器模式**：支持嵌套使用
  - 将 `TENANT_FILTER_DISABLED` 从 `bool` 改为 `u32` 计数器（`TENANT_FILTER_DISABLED_DEPTH`）
  - 嵌套场景：内层 guard drop 后外层 guard 仍保持禁用状态，所有 guard 全部 drop 后才恢复过滤
- **`TenantLayer` 中间件增强**：支持从 HTTP Header 提取租户 ID
  - 新增 `TenantPluginConfig.tenant_id_header` 配置字段：设置后中间件从指定 Header 提取 tenant_id
  - 优先级：HTTP Header > `TenantIdProvider` > `default_tenant_id`
  - 全局缓存 header 名称（`set_tenant_header_name()`），避免每次请求读取配置
- **TenantPluginConfig 新增 `enable_sql_log` 字段**：支持在 `[summer-sea-orm-ext-tenant]` 配置块中开启 SQL 日志，与 `[summer-sea-orm-ext] enable_sql_log` 等效，控制同一个全局开关
- **`default_databases` 存储升级为 `SeaOrmExtConnection`**：内部存储改为 `Vec<SeaOrmExtConnection>`，支持 SQL 日志拦截和重试配置
  - 新增 `set_default_databases_ext()` / `get_default_databases_ext()` / `get_available_default_database_ext()` ext 版本函数
  - 原 `set_default_databases()` / `get_default_databases()` / `get_available_default_database()` 保持向后兼容，自动包装/解包 `SeaOrmExtConnection`
- **组合场景测试**：新增 5 个组合测试覆盖动态租户 + ignore_tenant + 加解密的完整链路
  - `test_combo_tenant_id_codec_with_guard_and_insert`：加解密 + TenantGuard + 数据插入
  - `test_combo_dynamic_tenant_with_ignore_tenant_macro`：动态租户 + #[ignore_tenant] 跨租户查询
  - `test_combo_tenant_id_codec_decrypt_failure_skips_context`：解密失败回退到默认库
  - `test_combo_dynamic_tenant_data_isolation`：动态租户多租户数据隔离
  - `test_combo_manual_guard_with_ignore_tenant_macro`：手动 guard + 宏 guard 嵌套共存

### Changed

- 版本号从 `0.0.1` 升级到 `0.0.2`
- **`is_tenant_enforced()` 行为明确化**：仅在 `TenantMode::Table` 且未禁用过滤时返回 true，`Database` 模式下始终返回 false，避免两种隔离模式互相干扰

### Removed

- **删除 `apply_tenant_condition` / `apply_tenant_delete_condition`**：原 `src/tenant.rs` 中的死代码（从未被调用），功能已由宏覆盖后的 `Entity::update_many()` / `Entity::delete_many()` 统一实现
- **删除 `Entity::find_active()`**：原宏生成的 deprecated 方法，功能已由 `Entity::find()`（自动叠加软删除过滤）完全替代
- 清理 `src/tenant.rs` 中未使用的 `use sea_query::{DeleteStatement, UpdateStatement}` 导入

---

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
