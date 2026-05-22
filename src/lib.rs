//! `sea-orm-ext` — SeaORM 扩展库，为 SeaORM 提供多租户、软删除、字段自动填充、批量操作等高级特性。
//!
//! # 核心模块
//!
//! - `config` — 数据库配置(单库 & 多租户库)，支持 TOML 文件解析和异步连接创建
//! - `errors` — 统一的错误类型 `SeaOrmExtError` 和 `Result` 别名
//! - `fill` — 字段自动填充(插入/更新时)，以及可拔插的 ID 生成器
//! - `log` — SQL 日志模块，提供可配置的 SQL 语句打印功能
//! - `sea_orm_ext_connection` — SQL 日志连接包装器，拦截所有 SQL 执行并打印完整语句
//! - `soft_delete` — 软删除 trait，提供默认的未删除/已删除标记值
//! - `tenant` — 多租户核心: 租户模式、上下文、实体 trait、操作扩展、租户守卫
//! - `tenant_store` — 租户数据库连接存储: `ConnectionStore` trait 及其实现
//!
//! # Re-exports
//!
//! 本 crate 重新导出了所有依赖 crate，使用时只需引入 `sea-orm-ext` 一个依赖：
//!
//! - `sea_orm` — SeaORM 核心（始终可用）
//! - `sea_query` — SeaQuery 核心（始终可用）
//! - `summer` — Summer 框架（需要 `summer` feature）
//! - `summer_web` — Summer Web 框架（需要 `summer-web` feature）

mod config;
mod errors;
mod fill;
mod log;
mod sea_orm_ext_connection;
mod pagination;
mod soft_delete;
mod tenant;
mod tenant_store;

pub mod plugin;

pub use config::*;
pub use errors::*;
pub use fill::*;
pub use log::*;
pub use sea_orm_ext_connection::*;
pub use pagination::*;
pub use soft_delete::*;
pub use tenant::*;
pub use tenant_store::*;

pub use sea_orm_ext_macros::*;

pub use sea_orm;
pub use sea_query;

#[cfg(feature = "summer")]
pub use summer;

#[cfg(feature = "summer-web")]
pub use summer_web;

#[cfg(feature = "summer-web")]
pub use tower;

pub type DbConn = SeaOrmExtConnection;