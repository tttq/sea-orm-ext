
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, DbErr, ExecResult, QueryResult, Statement,
    StreamTrait, TransactionError, TransactionTrait,
};
use sea_query::{inject_parameters, MysqlQueryBuilder, PostgresQueryBuilder, SqliteQueryBuilder};
use std::ops::Deref;

#[derive(Debug, Clone)]
pub struct SeaOrmExtConnection {
    inner: DatabaseConnection,
}

impl SeaOrmExtConnection {
    pub fn new(conn: DatabaseConnection) -> Self {
        Self { inner: conn }
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
                "[sea-orm-ext SQL] {}\n[sea-orm-ext Params] [{}]",
                full_sql,
                params.join(", ")
            );
        }
        None => {
            tracing::info!("[sea-orm-ext SQL] {}", stmt.sql);
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
