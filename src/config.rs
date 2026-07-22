use serde::{Deserialize, Deserializer, Serialize};

fn deserialize_string_or_int<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct StringOrIntVisitor;

    impl<'de> Visitor<'de> for StringOrIntVisitor {
        type Value = String;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string or an integer")
        }

        fn visit_str<E>(self, v: &str) -> Result<String, E>
        where
            E: de::Error,
        {
            Ok(v.to_owned())
        }

        fn visit_i64<E>(self, v: i64) -> Result<String, E>
        where
            E: de::Error,
        {
            Ok(v.to_string())
        }

        fn visit_u64<E>(self, v: u64) -> Result<String, E>
        where
            E: de::Error,
        {
            Ok(v.to_string())
        }
    }

    deserializer.deserialize_any(StringOrIntVisitor)
}

/// 反序列化"单值或数组"为 `Vec<T>`。
///
/// 用于兼容 TOML 中的两种写法：
/// ```toml
/// # 单值（旧写法，会被转成单元素列表）
/// [default_database]
/// url = "..."
///
/// # 数组（新写法，推荐）
/// [[default_databases]]
/// url = "..."
/// ```
pub(crate) fn single_or_vec<'de, T, D>(deserializer: D) -> Result<Option<Vec<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SingleOrVec<T> {
        Vec(Vec<T>),
        Single(T),
    }

    impl<T> From<SingleOrVec<T>> for Vec<T> {
        fn from(v: SingleOrVec<T>) -> Vec<T> {
            match v {
                SingleOrVec::Vec(v) => v,
                SingleOrVec::Single(x) => vec![x],
            }
        }
    }

    let opt: Option<SingleOrVec<T>> = Option::deserialize(deserializer)?;
    Ok(opt.map(Into::into))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: Option<u32>,
    pub min_connections: Option<u32>,
    pub connect_timeout_secs: Option<u64>,
    pub acquire_timeout_secs: Option<u64>,
    #[serde(default)]
    pub enable_logging: bool,
    pub idle_timeout_secs: Option<u64>,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            // 默认 50：在高并发场景下提供足够的连接数，
            // 同时不会过度占用数据库资源（典型生产环境的合理值）。
            max_connections: Some(50),
            min_connections: Some(5),
            connect_timeout_secs: Some(30),
            acquire_timeout_secs: Some(30),
            enable_logging: false,
            idle_timeout_secs: Some(600),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantDatabaseConfig {
    #[serde(deserialize_with = "deserialize_string_or_int")]
    pub tenant_id: String,
    pub database: DatabaseConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TenantDatabaseConfigFile {
    pub tenants: Vec<TenantDatabaseConfig>,
    /// 默认数据库列表（fallback 链），TOML 中使用 `[[default_databases]]` 数组表语法。
    ///
    /// `#[serde(deserialize_with = "single_or_vec", alias = "default_database")]`
    /// 同时兼容旧的单数 `[default_database]` 写法（会被解析为单元素列表）。
    #[serde(default, deserialize_with = "single_or_vec", alias = "default_database")]
    pub default_databases: Option<Vec<DatabaseConfig>>,
}

impl TenantDatabaseConfigFile {
    pub fn from_file<P: AsRef<std::path::Path>>(
        path: P,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = std::fs::read_to_string(path)?;
        let config: TenantDatabaseConfigFile = toml::from_str(&contents)?;
        Ok(config)
    }

    pub fn from_toml(toml_str: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let config: TenantDatabaseConfigFile = toml::from_str(toml_str)?;
        Ok(config)
    }

    pub fn get_database_config(&self, tenant_id: &str) -> Option<&DatabaseConfig> {
        self.tenants
            .iter()
            .find(|t| t.tenant_id == tenant_id)
            .map(|t| &t.database)
    }

    pub fn get_all_tenant_ids(&self) -> Vec<String> {
        self.tenants.iter().map(|t| t.tenant_id.clone()).collect()
    }
}

#[cfg(feature = "runtime-tokio")]
pub mod async_support {
    use super::*;
    use crate::SeaOrmExtConnection;
    use sea_orm::{ConnectOptions, DatabaseConnection, DbErr};
    use sea_query::Value;

    pub async fn create_connection(config: &DatabaseConfig) -> Result<DatabaseConnection, DbErr> {
        let mut opt = ConnectOptions::new(&config.url);

        if let Some(max) = config.max_connections {
            opt.max_connections(max);
        }
        if let Some(min) = config.min_connections {
            opt.min_connections(min);
        }
        if let Some(timeout) = config.connect_timeout_secs {
            opt.connect_timeout(std::time::Duration::from_secs(timeout));
        }
        if let Some(timeout) = config.acquire_timeout_secs {
            opt.acquire_timeout(std::time::Duration::from_secs(timeout));
        }
        if let Some(timeout) = config.idle_timeout_secs {
            opt.idle_timeout(std::time::Duration::from_secs(timeout));
        }

        opt.sqlx_logging(false);

        sea_orm::Database::connect(opt).await
    }

    pub async fn initialize_from_config(
        store: &std::sync::Arc<dyn crate::ConnectionStore>,
        config_file: &TenantDatabaseConfigFile,
    ) -> Result<(), DbErr> {
        for tenant_config in &config_file.tenants {
            let conn = create_connection(&tenant_config.database).await?;
            let ext_conn = SeaOrmExtConnection::new(conn);
            store.insert_ext(Value::String(Some(tenant_config.tenant_id.clone())), ext_conn)?;
        }
        Ok(())
    }
}
