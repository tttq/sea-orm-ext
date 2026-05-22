use crate::{set_field_fill_handler, set_id_generator, set_sql_log_enabled, FieldFillHandler, FieldFillOperation, IdGenerator, SeaOrmExtConnection};
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use sea_query::Value;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use schemars::JsonSchema;
use summer::async_trait;
use summer::config::{Configurable, ConfigRegistry};
use summer::plugin::{ComponentRegistry, MutableComponentRegistry, Plugin};
use summer::app::AppBuilder;
use summer::App;
use summer::error::Result;

summer::submit_config_schema!("sea-orm", SeaOrmConfig);

#[derive(Debug, Configurable, Clone, JsonSchema, Deserialize)]
#[config_prefix = "sea-orm"]
pub struct SeaOrmConfig {
    pub uri: String,
    #[serde(default)]
    pub enable_sql_log: bool,
    #[serde(default = "default_min_connections")]
    pub min_connections: u32,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    pub connect_timeout: Option<u64>,
    pub idle_timeout: Option<u64>,
    pub acquire_timeout: Option<u64>,
    #[serde(default = "default_default_user")]
    pub default_user: String,
    #[serde(default)]
    pub fill_rules: Vec<FillRule>,
}

fn default_min_connections() -> u32 {
    1
}

fn default_max_connections() -> u32 {
    10
}

fn default_default_user() -> String {
    "system".to_string()
}

impl Default for SeaOrmConfig {
    fn default() -> Self {
        Self {
            uri: String::new(),
            enable_sql_log: false,
            min_connections: 1,
            max_connections: 10,
            connect_timeout: None,
            idle_timeout: None,
            acquire_timeout: None,
            default_user: default_default_user(),
            fill_rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, JsonSchema, Deserialize)]
pub struct FillRule {
    pub entity: String,
    pub field: String,
    pub operation: String,
    pub value: String,
}

#[derive(Clone)]
pub struct FieldFillHandlerComponent {
    pub handler: Arc<dyn FieldFillHandler>,
}

impl FieldFillHandlerComponent {
    pub fn new(handler: Arc<dyn FieldFillHandler>) -> Self {
        Self { handler }
    }
}

pub struct SeaOrmPlugin;

impl SeaOrmPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SeaOrmPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl SeaOrmPlugin {
    pub async fn connect(config: &SeaOrmConfig) -> Result<DatabaseConnection> {
        let mut opt = ConnectOptions::new(&config.uri);
        opt.max_connections(config.max_connections)
            .min_connections(config.min_connections)
            .sqlx_logging(false);

        if let Some(connect_timeout) = config.connect_timeout {
            opt.connect_timeout(Duration::from_millis(connect_timeout));
        }
        if let Some(idle_timeout) = config.idle_timeout {
            opt.idle_timeout(Duration::from_millis(idle_timeout));
        }
        if let Some(acquire_timeout) = config.acquire_timeout {
            opt.acquire_timeout(Duration::from_millis(acquire_timeout));
        }

        Ok(Database::connect(opt)
            .await
            .map_err(|e| anyhow::anyhow!("sea-orm connection failed: {} - {}", &config.uri, e))?)
    }

    async fn close_db_connection(app: Arc<App>) -> Result<String> {
        let conn = app
            .get_component::<SeaOrmExtConnection>()
            .expect("sea-orm db connection not exists");
        conn.close()
            .await
            .map_err(|e| anyhow::anyhow!("sea-orm db connection close failed: {}", e))?;
        Ok("sea-orm db connection close successful!".into())
    }
}

#[async_trait]
impl Plugin for SeaOrmPlugin {
    async fn build(&self, app: &mut AppBuilder) {
        let config = app
            .get_config::<SeaOrmConfig>()
            .expect("sea-orm plugin config load failed");

        let conn = Self::connect(&config)
            .await
            .expect("sea-orm plugin load failed");

        app.add_component(conn.clone());

        let ext_conn = SeaOrmExtConnection::new(conn);
        app.add_component(ext_conn);

        set_sql_log_enabled(config.enable_sql_log);

        if config.enable_sql_log {
            tracing::info!("[sea-orm] SQL log enabled, complete SQL with parameters will be printed");
        } else {
            tracing::info!("[sea-orm] SQL log disabled");
        }

        if let Some(component) = app.get_component::<FieldFillHandlerComponent>() {
            set_field_fill_handler(Box::new(ArcFieldFillHandler(component.handler.clone())));
            tracing::info!("[sea-orm] Custom FieldFillHandler registered from component");
        } else {
            let handler = ConfigFieldFillHandler::new(&config);
            set_field_fill_handler(Box::new(handler));
            tracing::info!("[sea-orm] Config-based FieldFillHandler registered");
        }

        let id_gen = DefaultIdGenerator::default();
        set_id_generator(Box::new(id_gen));

        app.add_shutdown_hook(|app| Box::new(Self::close_db_connection(app)));
    }

    fn name(&self) -> &'static str {
        "sea-orm"
    }
}

struct ArcFieldFillHandler(Arc<dyn FieldFillHandler>);

impl FieldFillHandler for ArcFieldFillHandler {
    fn fill(
        &self,
        entity_name: &str,
        field_name: &str,
        operation: FieldFillOperation,
    ) -> Option<Value> {
        self.0.fill(entity_name, field_name, operation)
    }
}

struct ConfigFieldFillHandler {
    default_user: String,
    fill_rules: Vec<FillRule>,
}

impl ConfigFieldFillHandler {
    fn new(config: &SeaOrmConfig) -> Self {
        Self {
            default_user: config.default_user.clone(),
            fill_rules: config.fill_rules.clone(),
        }
    }
}

impl FieldFillHandler for ConfigFieldFillHandler {
    fn fill(
        &self,
        entity_name: &str,
        field_name: &str,
        operation: FieldFillOperation,
    ) -> Option<Value> {
        let op_str = match operation {
            FieldFillOperation::Insert => "insert",
            FieldFillOperation::Update => "update",
        };

        for rule in &self.fill_rules {
            if rule.entity == entity_name
                && rule.field == field_name
                && (rule.operation == op_str || rule.operation == "insert_update")
            {
                return Some(rule.value.clone().into());
            }
        }

        match (entity_name, field_name, operation) {
            (_, "created_by", FieldFillOperation::Insert) => {
                Some(self.default_user.clone().into())
            }
            (_, "updated_by", FieldFillOperation::Update) => {
                Some(self.default_user.clone().into())
            }
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct DefaultIdGenerator {
    counter: std::sync::atomic::AtomicI64,
}

impl Default for DefaultIdGenerator {
    fn default() -> Self {
        Self {
            counter: std::sync::atomic::AtomicI64::new(1),
        }
    }
}

impl IdGenerator for DefaultIdGenerator {
    fn generate(&self) -> Value {
        self.counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            .into()
    }

    fn generate_for_type(&self, _entity_name: &str, _field_name: &str, field_type: &str) -> Option<Value> {
        match field_type {
            "String" | "Option<String>" => {
                let v = self.counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Some(v.to_string().into())
            }
            "i32" | "Option<i32>" => {
                let v = self.counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Some((v as i32).into())
            }
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct SnowflakeIdGenerator {
    epoch: i64,
    worker_id: i64,
    last_timestamp: std::sync::atomic::AtomicI64,
    sequence: std::sync::atomic::AtomicI16,
}

const WORKER_ID_BITS: i64 = 10;
const SEQUENCE_BITS: i64 = 12;
const MAX_WORKER_ID: i64 = (1 << WORKER_ID_BITS) - 1;
const MAX_SEQUENCE: i16 = (1 << SEQUENCE_BITS) as i16;
const WORKER_ID_SHIFT: i64 = SEQUENCE_BITS;
const TIMESTAMP_SHIFT: i64 = SEQUENCE_BITS + WORKER_ID_BITS;

const DEFAULT_EPOCH: i64 = 1704067200000;

impl SnowflakeIdGenerator {
    pub fn new(worker_id: i64) -> Self {
        Self::with_epoch(worker_id, DEFAULT_EPOCH)
    }

    pub fn with_epoch(worker_id: i64, epoch: i64) -> Self {
        assert!(
            worker_id >= 0 && worker_id <= MAX_WORKER_ID,
            "worker_id must be in range [0, {}], got {}",
            MAX_WORKER_ID,
            worker_id
        );
        Self {
            epoch,
            worker_id,
            last_timestamp: std::sync::atomic::AtomicI64::new(-1),
            sequence: std::sync::atomic::AtomicI16::new(0),
        }
    }

    fn current_timestamp(&self) -> i64 {
        let now = std::time::SystemTime::now();
        let duration = now
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_else(|e| std::time::Duration::from_millis(e.duration().as_millis() as u64));
        duration.as_millis() as i64
    }

    fn wait_next_millis(&self, last_ts: i64) -> i64 {
        let mut ts = self.current_timestamp();
        while ts <= last_ts {
            std::hint::spin_loop();
            ts = self.current_timestamp();
        }
        ts
    }
}

impl IdGenerator for SnowflakeIdGenerator {
    fn generate(&self) -> Value {
        let id = self.generate_snowflake_id();
        id.into()
    }

    fn generate_for_type(&self, _entity_name: &str, _field_name: &str, field_type: &str) -> Option<Value> {
        let id = self.generate_snowflake_id();
        match field_type {
            "String" | "Option<String>" => Some(id.to_string().into()),
            "i32" | "Option<i32>" => Some((id as i32).into()),
            _ => None,
        }
    }
}

impl SnowflakeIdGenerator {
    fn generate_snowflake_id(&self) -> i64 {
        let mut ts = self.current_timestamp();
        let last_ts = self.last_timestamp.load(std::sync::atomic::Ordering::SeqCst);

        let seq = if ts == last_ts {
            let s = self.sequence.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if s >= MAX_SEQUENCE {
                ts = self.wait_next_millis(last_ts);
                self.sequence.store(0, std::sync::atomic::Ordering::SeqCst);
                0
            } else {
                s
            }
        } else if ts > last_ts {
            let swapped = self.last_timestamp.compare_exchange(
                last_ts,
                ts,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            );
            if swapped.is_err() {
                ts = self.last_timestamp.load(std::sync::atomic::Ordering::SeqCst);
            }
            self.sequence.store(0, std::sync::atomic::Ordering::SeqCst);
            0
        } else {
            ts = self.wait_next_millis(last_ts);
            self.sequence.store(0, std::sync::atomic::Ordering::SeqCst);
            0
        };

        ((ts - self.epoch) << TIMESTAMP_SHIFT)
            | (self.worker_id << WORKER_ID_SHIFT)
            | (seq as i64)
    }
}
