
//! SQL 日志连接包装器
//!
//! 提供 [`LoggingConnection`]，包装 `DatabaseConnection`，
//! 在每次 SQL 执行前打印完整的 SQL 语句（含参数值注入）。
//!
//! # 核心原理
//!
//! sea-orm 的 [`Statement`] 实现了 `Display` trait，
//! 内部调用 `inject_parameters` 将参数值注入到 SQL 模板中，
//! 生成可直接执行的完整 SQL 字符串。
//!
//! # 使用方式
//!
//! ```ignore
//! use sea_orm_ext::LoggingConnection;
//!
//! let db = Database::connect("...").await?;
//! let logging_db = LoggingConnection::new(db);
//!
//! // 之后所有通过 logging_db 执行的 SQL 都会被记录
//! let result = Entity::find().all(&logging_db).await?;
//! ```

use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, DbErr, ExecResult, QueryResult, Statement,
    StreamTrait, TransactionError, TransactionTrait,
};
use std::ops::Deref;

/// 带有 SQL 日志功能的数据库连接包装器。
///
/// 包装 `DatabaseConnection`，在每次执行 SQL 语句前，
/// 检查全局 SQL 日志开关，若开启则通过 `tracing::info!` 打印完整 SQL。
///
/// 打印格式：`[sea-orm-ext SQL] <完整SQL语句>`
///
/// 完整 SQL 由 `Statement` 的 `Display` 实现生成，
/// 已将参数值注入到占位符位置，可直接复制到数据库客户端执行。
#[derive(Debug, Clone)]
pub struct LoggingConnection {
    inner: DatabaseConnection,
}

impl LoggingConnection {
    /// 创建带日志功能的数据库连接包装器。
    pub fn new(conn: DatabaseConnection) -> Self {
        Self { inner: conn }
    }

    /// 获取内部原始连接的引用。
    pub fn inner(&self) -> &DatabaseConnection {
        &self.inner
    }

    /// 消费包装器，返回内部原始连接。
    pub fn into_inner(self) -> DatabaseConnection {
        self.inner
    }

    pub async fn close(self) -> Result<(), DbErr> {
        self.inner.close().await
    }
}

impl Deref for LoggingConnection {
    type Target = DatabaseConnection;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// 当 SQL 日志开启时，打印完整 SQL 语句。
fn log_statement(stmt: &Statement) {
    if crate::log::is_sql_log_enabled() {
        let full_sql = format!("{}", stmt);
        tracing::info!("[sea-orm-ext SQL] {}", full_sql);
    }
}

#[async_trait::async_trait]
impl ConnectionTrait for LoggingConnection {
    fn get_database_backend(&self) -> DbBackend {
        self.inner.get_database_backend()
    }

    async fn execute_raw(&self, stmt: Statement) -> Result<ExecResult, DbErr> {
        log_statement(&stmt);
        self.inner.execute_raw(stmt).await
    }

    async fn execute_unprepared(&self, sql: &str) -> Result<ExecResult, DbErr> {
        if crate::log::is_sql_log_enabled() {
            tracing::info!("[sea-orm-ext SQL] {}", sql);
        }
        self.inner.execute_unprepared(sql).await
    }

    async fn query_one_raw(&self, stmt: Statement) -> Result<Option<QueryResult>, DbErr> {
        log_statement(&stmt);
        self.inner.query_one_raw(stmt).await
    }

    async fn query_all_raw(&self, stmt: Statement) -> Result<Vec<QueryResult>, DbErr> {
        log_statement(&stmt);
        self.inner.query_all_raw(stmt).await
    }
}

impl StreamTrait for LoggingConnection {
    type Stream<'a> = <DatabaseConnection as StreamTrait>::Stream<'a>;

    fn get_database_backend(&self) -> DbBackend {
        self.inner.get_database_backend()
    }

    fn stream_raw<'a>(
        &'a self,
        stmt: Statement,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Stream<'a>, DbErr>> + Send + 'a>,
    > {
        log_statement(&stmt);
        self.inner.stream_raw(stmt)
    }
}

#[async_trait::async_trait]
impl TransactionTrait for LoggingConnection {
    type Transaction = <DatabaseConnection as TransactionTrait>::Transaction;

    async fn begin(&self) -> Result<Self::Transaction, DbErr> {
        self.inner.begin().await
    }

    async fn begin_with_config(
        &self,
        isolation_level: Option<sea_orm::IsolationLevel>,
        access_mode: Option<sea_orm::AccessMode>,
    ) -> Result<Self::Transaction, DbErr> {
        self.inner.begin_with_config(isolation_level, access_mode).await
    }

    async fn begin_with_options(
        &self,
        options: sea_orm::TransactionOptions,
    ) -> Result<Self::Transaction, DbErr> {
        self.inner.begin_with_options(options).await
    }

    async fn transaction<F, T, E>(&self, callback: F) -> Result<T, TransactionError<E>>
    where
        F: for<'c> FnOnce(
                &'c Self::Transaction,
            ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, E>> + Send + 'c>>
            + Send,
        T: Send,
        E: std::fmt::Display + std::fmt::Debug + Send,
    {
        self.inner.transaction(callback).await
    }

    async fn transaction_with_config<F, T, E>(
        &self,
        callback: F,
        isolation_level: Option<sea_orm::IsolationLevel>,
        access_mode: Option<sea_orm::AccessMode>,
    ) -> Result<T, TransactionError<E>>
    where
        F: for<'c> FnOnce(
                &'c Self::Transaction,
            ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, E>> + Send + 'c>>
            + Send,
        T: Send,
        E: std::fmt::Display + std::fmt::Debug + Send,
    {
        self.inner
            .transaction_with_config(callback, isolation_level, access_mode)
            .await
    }
}
