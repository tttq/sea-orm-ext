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
            max_connections: Some(10),
            min_connections: Some(1),
            connect_timeout_secs: Some(30),
            acquire_timeout_secs: Some(30),
            enable_logging: false,
            idle_timeout_secs: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantDatabaseConfig {
    #[serde(deserialize_with = "deserialize_string_or_int")]
    pub tenant_id: String,
    pub database: DatabaseConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantDatabaseConfigFile {
    pub tenants: Vec<TenantDatabaseConfig>,
    pub default_database: Option<DatabaseConfig>,
}

impl Default for TenantDatabaseConfigFile {
    fn default() -> Self {
        Self {
            tenants: Vec::new(),
            default_database: None,
        }
    }
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
