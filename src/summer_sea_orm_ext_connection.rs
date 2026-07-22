
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, DbErr, ExecResult, QueryResult, Statement,
    StreamTrait, TransactionError, TransactionTrait,
};
use sea_query::{inject_parameters, MysqlQueryBuilder, PostgresQueryBuilder, SqliteQueryBuilder};
use std::ops::Deref;
use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug, Clone)]
pub struct SeaOrmExtConnection {
    inner: DatabaseConnection,
    /// 重试配置：失败时的最大重试次数（0 = 不重试）
    max_retries: std::sync::Arc<AtomicU32>,
}

impl SeaOrmExtConnection {
    pub fn new(conn: DatabaseConnection) -> Self {
        Self {
            inner: conn,
            max_retries: std::sync::Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn inner(&self) -> &DatabaseConnection {
        &self.inner
    }

    pub fn into_inner(self) -> DatabaseConnection {
        self.inner
    }

    pub async fn close(self) -> Result<(), DbErr> {
        self.inner.close().await
    }

    /// 设置失败重试次数（仅针对连接类错误，默认 0 = 不重试）
    ///
    /// 重试仅对 `execute_raw` / `query_one_raw` / `query_all_raw` 生效，
    /// 事务操作不重试（事务失败需要业务侧自行处理）。
    pub fn set_max_retries(&self, max: u32) {
        self.max_retries.store(max, Ordering::Relaxed);
    }

    /// 获取当前重试次数配置
    pub fn get_max_retries(&self) -> u32 {
        self.max_retries.load(Ordering::Relaxed)
    }

    /// 判断错误是否为可重试的连接类错误
    fn is_retryable_error(err: &DbErr) -> bool {
        let msg = err.to_string().to_lowercase();
        // 常见的连接类错误关键词
        msg.contains("connection")
            || msg.contains("broken pipe")
            || msg.contains("connection reset")
            || msg.contains("connection refused")
            || msg.contains("timed out")
            || msg.contains("timeout")
            || msg.contains("pool")
            || msg.contains("server has gone away")
            || msg.contains(" eof ")
    }

    /// 带重试的执行包装
    async fn with_retry<F, Fut, T>(&self, op: F) -> Result<T, DbErr>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, DbErr>>,
    {
        let max = self.get_max_retries();
        let mut attempt = 0u32;
        loop {
            match op().await {
                Ok(v) => return Ok(v),
                Err(e) => {
                    if attempt >= max || !Self::is_retryable_error(&e) {
                        return Err(e);
                    }
                    attempt += 1;
                    // 指数退避：100ms, 200ms, 400ms, ...
                    let backoff_ms = 100u64 * (1 << attempt.min(6));
                    tracing::warn!(
                        "DB operation failed (attempt {}/{}), retrying in {}ms: {}",
                        attempt, max, backoff_ms, e
                    );
                    #[cfg(feature = "runtime-tokio")]
                    tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                    #[cfg(not(feature = "runtime-tokio"))]
                    std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
                }
            }
        }
    }
}

impl Deref for SeaOrmExtConnection {
    type Target = DatabaseConnection;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

fn log_statement(stmt: &Statement) {
    if !crate::log::is_sql_log_enabled() {
        return;
    }

    match &stmt.values {
        Some(values) => {
            let full_sql = match stmt.db_backend {
                DbBackend::MySql => inject_parameters(&stmt.sql, &values.0, &MysqlQueryBuilder),
                DbBackend::Postgres => inject_parameters(&stmt.sql, &values.0, &PostgresQueryBuilder),
                DbBackend::Sqlite => inject_parameters(&stmt.sql, &values.0, &SqliteQueryBuilder),
                _ => inject_parameters(&stmt.sql, &values.0, &SqliteQueryBuilder),
            };
            let params: Vec<String> = values.0.iter().map(|v| format!("{:?}", v)).collect();
            tracing::info!(
                "[summer-sea-orm-ext SQL] {}\n[summer-sea-orm-ext Params] [{}]",
                full_sql,
                params.join(", ")
            );
        }
        None => {
            tracing::info!("[summer-sea-orm-ext SQL] {}", stmt.sql);
        }
    }
}

#[async_trait::async_trait]
impl ConnectionTrait for SeaOrmExtConnection {
    fn get_database_backend(&self) -> DbBackend {
        self.inner.get_database_backend()
    }

    async fn execute_raw(&self, stmt: Statement) -> Result<ExecResult, DbErr> {
        log_statement(&stmt);
        let stmt_clone = stmt.clone();
        self.with_retry(|| {
            let stmt = stmt_clone.clone();
            self.inner.execute_raw(stmt)
        })
        .await
    }

    async fn execute_unprepared(&self, sql: &str) -> Result<ExecResult, DbErr> {
        if crate::log::is_sql_log_enabled() {
            tracing::info!("[summer-sea-orm-ext SQL] {}", sql);
        }
        let sql_owned = sql.to_string();
        self.with_retry(|| {
            self.inner.execute_unprepared(&sql_owned)
        })
        .await
    }

    async fn query_one_raw(&self, stmt: Statement) -> Result<Option<QueryResult>, DbErr> {
        log_statement(&stmt);
        let stmt_clone = stmt.clone();
        self.with_retry(|| {
            let stmt = stmt_clone.clone();
            self.inner.query_one_raw(stmt)
        })
        .await
    }

    async fn query_all_raw(&self, stmt: Statement) -> Result<Vec<QueryResult>, DbErr> {
        log_statement(&stmt);
        let stmt_clone = stmt.clone();
        self.with_retry(|| {
            let stmt = stmt_clone.clone();
            self.inner.query_all_raw(stmt)
        })
        .await
    }
}

impl StreamTrait for SeaOrmExtConnection {
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
impl TransactionTrait for SeaOrmExtConnection {
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
