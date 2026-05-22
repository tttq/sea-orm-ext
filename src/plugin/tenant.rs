use crate::{
    set_tenant_config, set_tenant_database_provider, set_tenant_id_provider, set_tenant_store,
    ConnectionStore, HashMapConnectionStore, SeaOrmExtConnection, TenantConfig, TenantDatabaseProvider, TenantIdProvider, TenantMode,
};
#[cfg(feature = "summer-web")]
use crate::{
    clear_tenant_context, get_tenant_database, get_tenant_mode,
    is_tenant_enabled, set_tenant_context,
};
use sea_orm::DatabaseConnection;
use sea_query::Value;
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::Arc;
use schemars::JsonSchema;
use summer::async_trait;
use summer::config::{Configurable, ConfigRegistry};
use summer::plugin::{ComponentRegistry, MutableComponentRegistry, Plugin};
use summer::app::AppBuilder;

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

fn deserialize_string_or_int_opt<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct StringOrIntOptVisitor;

    impl<'de> Visitor<'de> for StringOrIntOptVisitor {
        type Value = Option<String>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string, an integer, or null")
        }

        fn visit_none<E>(self) -> Result<Option<String>, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Option<String>, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D2>(self, deserializer: D2) -> Result<Option<String>, D2::Error>
        where
            D2: Deserializer<'de>,
        {
            deserialize_string_or_int(deserializer).map(Some)
        }

        fn visit_str<E>(self, v: &str) -> Result<Option<String>, E>
        where
            E: de::Error,
        {
            Ok(Some(v.to_owned()))
        }

        fn visit_i64<E>(self, v: i64) -> Result<Option<String>, E>
        where
            E: de::Error,
        {
            Ok(Some(v.to_string()))
        }

        fn visit_u64<E>(self, v: u64) -> Result<Option<String>, E>
        where
            E: de::Error,
        {
            Ok(Some(v.to_string()))
        }
    }

    deserializer.deserialize_option(StringOrIntOptVisitor)
}

#[derive(Debug, Clone, Serialize,JsonSchema, Deserialize)]
pub struct TenantDatabaseEntryConfig {
    pub url: String,
    pub max_connections: Option<u32>,
    pub min_connections: Option<u32>,
    pub connect_timeout_secs: Option<u64>,
    pub acquire_timeout_secs: Option<u64>,
}

impl Default for TenantDatabaseEntryConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            max_connections: Some(10),
            min_connections: Some(1),
            connect_timeout_secs: Some(30),
            acquire_timeout_secs: Some(30),
        }
    }
}

#[derive(Debug, Clone, Serialize,JsonSchema, Deserialize)]
pub struct TenantDatabaseEntry {
    #[serde(deserialize_with = "deserialize_string_or_int")]
    pub tenant_id: String,
    pub database: TenantDatabaseEntryConfig,
}

#[derive(Clone)]
pub struct SaTokenLayerMarker;

#[derive(Clone)]
pub struct TenantIdProviderComponent {
    pub provider: Arc<dyn TenantIdProvider>,
}

impl TenantIdProviderComponent {
    pub fn new(provider: Arc<dyn TenantIdProvider>) -> Self {
        Self { provider }
    }
}

#[derive(Clone)]
pub struct TenantDatabaseProviderComponent {
    pub provider: Arc<dyn TenantDatabaseProvider>,
}

impl TenantDatabaseProviderComponent {
    pub fn new(provider: Arc<dyn TenantDatabaseProvider>) -> Self {
        Self { provider }
    }
}
#[derive(Clone, Serialize, Deserialize,JsonSchema, Configurable)]
#[config_prefix = "sea-orm-ext-tenant"]
pub struct TenantPluginConfig {
    pub enabled: bool,
    pub mode: String,
    pub database_source: Option<String>,
    #[serde(deserialize_with = "deserialize_string_or_int_opt")]
    pub default_tenant_id: Option<String>,
    pub databases: Option<Vec<TenantDatabaseEntry>>,
    pub default_database: Option<TenantDatabaseEntryConfig>,
    #[serde(skip)]
    pub tenant_id_provider: Option<Arc<dyn TenantIdProvider>>,
    #[serde(skip)]
    pub tenant_database_provider: Option<Arc<dyn TenantDatabaseProvider>>,
}

impl std::fmt::Debug for TenantPluginConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TenantPluginConfig")
            .field("enabled", &self.enabled)
            .field("mode", &self.mode)
            .field("database_source", &self.database_source)
            .field("default_tenant_id", &self.default_tenant_id)
            .field("databases", &self.databases)
            .field("default_database", &self.default_database)
            .field("tenant_id_provider", &self.tenant_id_provider.as_ref().map(|_| "..."))
            .field("tenant_database_provider", &self.tenant_database_provider.as_ref().map(|_| "..."))
            .finish()
    }
}

impl Default for TenantPluginConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: "table".to_string(),
            database_source: Some("config".to_string()),
            default_tenant_id: None,
            databases: None,
            default_database: None,
            tenant_id_provider: None,
            tenant_database_provider: None,
        }
    }
}

pub struct TenantPlugin;

impl TenantPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TenantPlugin {
    fn default() -> Self {
        Self::new()
    }
}

async fn connect_from_entry(entry: &TenantDatabaseEntry) -> Result<DatabaseConnection, sea_orm::DbErr> {
    let mut opt = sea_orm::ConnectOptions::new(&entry.database.url);
    if let Some(max) = entry.database.max_connections {
        opt.max_connections(max);
    }
    if let Some(min) = entry.database.min_connections {
        opt.min_connections(min);
    }
    if let Some(timeout) = entry.database.connect_timeout_secs {
        opt.connect_timeout(std::time::Duration::from_secs(timeout));
    }
    if let Some(timeout) = entry.database.acquire_timeout_secs {
        opt.acquire_timeout(std::time::Duration::from_secs(timeout));
    }
    opt.sqlx_logging(false);
    sea_orm::Database::connect(opt).await
}

async fn connect_from_options(tenant_id: &Value, opt: &sea_orm::ConnectOptions) -> Result<DatabaseConnection, sea_orm::DbErr> {
    let mut cloned_opt = opt.clone();
    cloned_opt.sqlx_logging(false);
    let conn = sea_orm::Database::connect(cloned_opt).await;
    if conn.is_ok() {
        tracing::info!("Connected to database for tenant {:?}", tenant_id);
    } else {
        tracing::error!("Failed to connect to database for tenant {:?}: {:?}", tenant_id, conn.as_ref().err());
    }
    conn
}

#[async_trait]
impl Plugin for TenantPlugin {
    async fn build(&self, app: &mut AppBuilder) {
        let config = app.get_config::<TenantPluginConfig>()
            .expect("tenant plugin config load failed");

        if !config.enabled {
            tracing::info!("Tenant plugin is disabled, skipping initialization");
            return;
        }

        let mode = match config.mode.to_lowercase().as_str() {
            "database" => TenantMode::Database,
            "table" => TenantMode::Table,
            _ => {
                tracing::warn!(
                    "Unknown tenant mode '{}', falling back to 'table'",
                    config.mode
                );
                TenantMode::Table
            }
        };

        let default_tenant_id: Option<Value> =
            config.default_tenant_id.map(|id| Value::String(Some(id)));

        set_tenant_config(TenantConfig {
            enabled: true,
            mode,
            default_tenant_id,
            ignored_tables: Default::default(),
        });

        if let Some(db) = app.get_component::<DatabaseConnection>() {
            crate::set_default_database(db);
            tracing::info!("Default database connection registered for tenant module");
        }

        if let Some(provider) = &config.tenant_id_provider {
            app.add_component(TenantIdProviderComponent::new(provider.clone()));
            set_tenant_id_provider(provider.clone());
            tracing::info!("Tenant ID provider registered from plugin config");
        } else if let Some(component) = app.get_component::<TenantIdProviderComponent>() {
            set_tenant_id_provider(component.provider.clone());
            tracing::info!("Tenant ID provider registered from component registry");
        } else {
            tracing::warn!(
                "No TenantIdProvider registered. Tenant ID will only be available via \
                 default_tenant_id config or manual set_tenant_context() calls."
            );
        }

        if mode == TenantMode::Database {
            let store: Arc<dyn ConnectionStore> = Arc::new(HashMapConnectionStore::new());
            let source = config.database_source
                .as_deref()
                .unwrap_or("config")
                .to_lowercase();

            match source.as_str() {
                "config" => {
                    if let Some(databases) = &config.databases {
                        for entry in databases {
                            let tenant_id = &entry.tenant_id;
                            match connect_from_entry(entry).await {
                                Ok(conn) => {
                                    tracing::info!(
                                        "Connected to database for tenant {}: {}",
                                        tenant_id,
                                        entry.database.url
                                    );
                                    let _ = store.insert_ext(Value::String(Some(tenant_id.clone())), SeaOrmExtConnection::new(conn));
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "Failed to connect to database for tenant {}: {}",
                                        tenant_id,
                                        e
                                    );
                                }
                            }
                        }
                    }
                    tracing::info!(
                        "Tenant plugin initialized in Database mode (config source) with {} tenants",
                        config.databases.as_ref().map_or(0, |dbs| dbs.len())
                    );
                }
                "custom" => {
                    let provider = config.tenant_database_provider.as_ref()
                        .cloned()
                        .or_else(|| {
                            app.get_component::<TenantDatabaseProviderComponent>()
                                .map(|c| c.provider.clone())
                        });

                    if let Some(provider) = provider {
                        set_tenant_database_provider(provider.clone());
                        let databases = provider.provide();
                        let mut connected = 0usize;
                        for (tenant_id, opts) in &databases {
                            match connect_from_options(tenant_id, opts).await {
                                Ok(conn) => {
                                    let _ = store.insert_ext(tenant_id.clone(), SeaOrmExtConnection::new(conn));
                                    connected += 1;
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "Failed to connect to database for tenant {:?}: {}",
                                        tenant_id,
                                        e
                                    );
                                }
                            }
                        }
                        tracing::info!(
                            "Tenant plugin initialized in Database mode (custom source) with {}/{} tenants connected",
                            connected,
                            databases.len()
                        );
                    } else {
                        tracing::warn!(
                            "Database source is 'custom' but no TenantDatabaseProvider registered. \
                             Register via config.tenant_database_provider or app.add_component(TenantDatabaseProviderComponent)."
                        );
                    }
                }
                _ => {
                    tracing::warn!(
                        "Unknown database_source '{}', falling back to 'config'",
                        source
                    );
                    if let Some(databases) = &config.databases {
                        for entry in databases {
                            let tenant_id = &entry.tenant_id;
                            match connect_from_entry(entry).await {
                                Ok(conn) => {
                                    let _ = store.insert_ext(Value::String(Some(tenant_id.clone())), SeaOrmExtConnection::new(conn));
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "Failed to connect to database for tenant {}: {}",
                                        tenant_id,
                                        e
                                    );
                                }
                            }
                        }
                    }
                }
            }

            set_tenant_store(store);
        } else {
            tracing::info!("Tenant plugin initialized in Table mode");
        }

        #[cfg(feature = "summer-web")]
        {
            use summer_web::{RouterLayer, RouterLayers};
            let layer_fn: RouterLayer = std::sync::Arc::new(
                |router| router.layer(crate::plugin::tenant_layer::TenantLayer)
            );
            let insert_pos = if app.get_component::<SaTokenLayerMarker>().is_some() {
                let layers = app.get_component_ref::<RouterLayers>();
                let len = layers.as_ref().map_or(0, |l| l.len());
                if len > 0 {
                    tracing::info!(
                        "SaTokenLayer detected, TenantLayer will be placed after it (inner layer, lower priority)"
                    );
                    len - 1
                } else {
                    0
                }
            } else {
                tracing::info!(
                    "No SaTokenLayer detected, TenantLayer registered as innermost layer (lowest priority)"
                );
                0
            };
            if let Some(layers) = app.get_component_ref::<RouterLayers>() {
                unsafe {
                    let raw_ptr = layers.into_raw();
                    let layers = &mut *(raw_ptr as *mut RouterLayers);
                    layers.insert(insert_pos, layer_fn);
                }
            } else {
                app.add_component(vec![layer_fn] as RouterLayers);
            }
        }
    }

    fn name(&self) -> &'static str {
        "sea-orm-ext-tenant"
    }
}

#[cfg(feature = "summer-web")]
pub mod tenant_layer {
    use super::*;
    use crate::{get_tenant_id_provider, SeaOrmExtConnection, TenantContext};
    use summer_web::axum;
    use summer_web::axum::http::Request;
    use summer_web::axum::response::{IntoResponse, Response};
    use std::task::{Context, Poll};
    use tower::Layer;
    use tower::Service;

    #[derive(Clone)]
    pub struct TenantLayer;

    impl<S> Layer<S> for TenantLayer {
        type Service = TenantMiddleware<S>;

        fn layer(&self, inner: S) -> Self::Service {
            TenantMiddleware { inner }
        }
    }

    #[derive(Clone)]
    pub struct TenantMiddleware<S> {
        inner: S,
    }

    impl<S, B> Service<Request<B>> for TenantMiddleware<S>
    where
        S: Service<Request<B>, Response = Response> + Send + Clone + 'static,
        S::Future: Send + 'static,
        S::Error: Send + 'static,
        B: Send + 'static,
    {
        type Response = <S as Service<Request<B>>>::Response;
        type Error = <S as Service<Request<B>>>::Error;
        type Future = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

        fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            self.inner.poll_ready(cx)
        }

        fn call(&mut self, mut req: Request<B>) -> Self::Future {
            if is_tenant_enabled() {
                if let Some(tenant_id) = resolve_tenant_id() {
                    set_tenant_context(TenantContext {
                        tenant_id: tenant_id.clone(),
                    });

                    if get_tenant_mode() == Some(TenantMode::Database) {
                        if let Ok(Some(db)) = get_tenant_database() {
                            req.extensions_mut().insert::<DatabaseConnection>(db);
                        }
                    }
                }
            }

            let inner = self.inner.clone();
            let mut inner = std::mem::replace(&mut self.inner, inner);

            Box::pin(async move {
                let response = inner.call(req).await;
                clear_tenant_context();
                response
            })
        }
    }

    fn resolve_tenant_id() -> Option<Value> {
        if let Some(provider) = get_tenant_id_provider() {
            if let Some(id) = provider.get_tenant_id() {
                return Some(id);
            }
        }
        crate::get_tenant_config().and_then(|c| c.default_tenant_id.clone())
    }

    pub struct TenantDb(pub SeaOrmExtConnection);

    impl TenantDb {
        pub fn into_inner(self) -> DatabaseConnection {
            self.0.into_inner()
        }

        pub fn inner(&self) -> &DatabaseConnection {
            self.0.inner()
        }

        pub fn as_ext(&self) -> &SeaOrmExtConnection {
            &self.0
        }
    }

    impl<B> axum::extract::FromRequestParts<B> for TenantDb
    where
        B: Send + Sync,
    {
        type Rejection = axum::response::Response;

        async fn from_request_parts(
            parts: &mut axum::http::request::Parts,
            _state: &B,
        ) -> Result<Self, Self::Rejection> {
            if get_tenant_mode() == Some(TenantMode::Database) {
                if let Some(db) = parts.extensions.get::<DatabaseConnection>() {
                    return Ok(TenantDb(SeaOrmExtConnection::new(db.clone())));
                }

                if let Ok(Some(db)) = get_tenant_database() {
                    return Ok(TenantDb(SeaOrmExtConnection::new(db)));
                }

                if let Some(store) = crate::get_tenant_store() {
                    if let Some(tenant_id) = crate::get_current_tenant_id() {
                        if let Some(ext_conn) = store.get_ext(&tenant_id) {
                            return Ok(TenantDb(ext_conn));
                        }
                    }
                }
            }

            let app = parts.extensions.get::<summer_web::AppState>();
            if let Some(app_state) = app {
                if let Some(ext_conn) = app_state.app.get_component::<SeaOrmExtConnection>() {
                    return Ok(TenantDb(ext_conn));
                }
                if let Some(db) = app_state.app.get_component::<DatabaseConnection>() {
                    return Ok(TenantDb(SeaOrmExtConnection::new(db)));
                }
            }

            Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}
