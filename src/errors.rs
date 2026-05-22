use thiserror::Error;

/// `sea-orm-ext` 的统一错误类型。
///
/// 覆盖多租户数据库连接、配置、字段填充、ID 生成等所有模块可能产生的错误。
#[derive(Debug, Error)]
pub enum SeaOrmExtError {
    /// 根据租户标识未找到对应租户
    #[error("Tenant not found: {0}")]
    TenantNotFound(String),

    /// 租户 ID 为必填项但未提供
    #[error("Tenant ID is required but not provided")]
    TenantIdRequired,

    /// 为指定租户建立数据库连接失败，内层包含 SeaORM 的 `DbErr`
    #[error("Database connection failed for tenant {tenant_id}: {source}")]
    DatabaseConnectionFailed {
        tenant_id: String,
        #[source]
        source: sea_orm::DbErr,
    },

    /// 连接存储(ConnectionStore)尚未初始化
    #[error("Connection store not initialized")]
    ConnectionStoreNotInitialized,

    /// 租户存储尚未初始化
    #[error("Tenant store not initialized")]
    TenantStoreNotInitialized,

    /// 租户配置未设置
    #[error("Tenant config not set")]
    TenantConfigNotSet,

    /// 非法的租户模式字符串
    #[error("Invalid tenant mode: {0}")]
    InvalidTenantMode(String),

    /// 通用数据库错误，通过 `#[from]` 属性可从 `sea_orm::DbErr` 自动转换
    #[error("Database error: {0}")]
    Database(#[from] sea_orm::DbErr),

    /// 字段自动填充处理器未找到
    #[error("Field fill handler not found")]
    FieldFillHandlerNotFound,

    /// ID 生成器尚未初始化
    #[error("ID generator not initialized")]
    IdGeneratorNotInitialized,

    /// 通用配置错误
    #[error("Configuration error: {0}")]
    Config(String),
}

/// `sea-orm-ext` 的标准 Result 类型别名。
pub type Result<T> = std::result::Result<T, SeaOrmExtError>;