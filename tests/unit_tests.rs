
mod common;
use common::*;
use sea_orm::ActiveModelTrait;
use sea_orm::ActiveValue::Set;
use sea_orm::ConnectOptions;
use sea_query::Value;
use serial_test::serial;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// ===========================================================================
// config 模块测试
// ===========================================================================

#[test]
fn test_database_config_default() {
    let config = DatabaseConfig::default();
    assert!(config.url.is_empty());
    assert_eq!(config.max_connections, Some(50));
    assert_eq!(config.min_connections, Some(5));
    assert_eq!(config.connect_timeout_secs, Some(30));
    assert_eq!(config.acquire_timeout_secs, Some(30));
    assert_eq!(config.idle_timeout_secs, Some(600));
    assert!(!config.enable_logging);
}

#[test]
fn test_database_config_custom() {
    let config = DatabaseConfig {
        url: "sqlite::memory:".to_string(),
        max_connections: Some(20),
        min_connections: Some(5),
        connect_timeout_secs: Some(60),
        acquire_timeout_secs: Some(10),
        enable_logging: false,
        idle_timeout_secs: None,
    };
    assert_eq!(config.url, "sqlite::memory:");
    assert_eq!(config.max_connections, Some(20));
}

#[test]
fn test_tenant_database_config_file_from_toml() {
    let toml_str = r#"
        [[tenants]]
        tenant_id = "1"
        [tenants.database]
        url = "sqlite::memory:"

        [[tenants]]
        tenant_id = "2"
        [tenants.database]
        url = "sqlite::memory:"
        max_connections = 20
    "#;

    let config = TenantDatabaseConfigFile::from_toml(toml_str).unwrap();
    assert_eq!(config.tenants.len(), 2);
    assert_eq!(config.tenants[0].tenant_id, "1");
    assert_eq!(config.tenants[0].database.url, "sqlite::memory:");
    assert_eq!(config.tenants[1].tenant_id, "2");
    assert_eq!(config.tenants[1].database.max_connections, Some(20));
}

#[test]
fn test_tenant_database_config_file_default() {
    let config = TenantDatabaseConfigFile::default();
    assert!(config.tenants.is_empty());
    assert!(config.default_databases.is_none());
}

#[test]
fn test_tenant_database_config_get_database_config() {
    let toml_str = r#"
        [[tenants]]
        tenant_id = "1"
        [tenants.database]
        url = "sqlite::memory:"

        [[tenants]]
        tenant_id = "2"
        [tenants.database]
        url = "sqlite::memory:"
    "#;

    let config = TenantDatabaseConfigFile::from_toml(toml_str).unwrap();
    assert!(config.get_database_config("1").is_some());
    assert!(config.get_database_config("2").is_some());
    assert!(config.get_database_config("999").is_none());
}

#[test]
fn test_tenant_database_config_get_all_tenant_ids() {
    let toml_str = r#"
        [[tenants]]
        tenant_id = "10"
        [tenants.database]
        url = "sqlite::memory:"

        [[tenants]]
        tenant_id = "20"
        [tenants.database]
        url = "sqlite::memory:"
    "#;

    let config = TenantDatabaseConfigFile::from_toml(toml_str).unwrap();
    let ids = config.get_all_tenant_ids();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"10".to_string()));
    assert!(ids.contains(&"20".to_string()));
}

#[cfg(feature = "summer")]
#[test]
fn test_tenant_plugin_config_from_toml_with_integer_tenant_id() {
    use sea_orm_ext::plugin::tenant::TenantPluginConfig;

    let toml_str = r#"
        enabled = true
        mode = "database"
        database_source = "config"
        default_tenant_id = 1876543210000000001

        [[databases]]
        tenant_id = 1876543210000000001
        [databases.database]
        url = "postgres://user:pass@localhost:5432/tenant_1"
        max_connections = 10

        [[databases]]
        tenant_id = "1876543210000000002"
        [databases.database]
        url = "postgres://user:pass@localhost:5432/tenant_2"
        max_connections = 20
    "#;

    let table: toml::Table = toml::from_str(toml_str).unwrap();
    let config: TenantPluginConfig = table.try_into().unwrap();

    assert!(config.enabled);
    assert_eq!(config.mode, "database");
    assert_eq!(config.default_tenant_id, Some("1876543210000000001".to_string()));

    let databases = config.databases.unwrap();
    assert_eq!(databases.len(), 2);
    assert_eq!(databases[0].tenant_id, "1876543210000000001");
    assert_eq!(databases[1].tenant_id, "1876543210000000002");
    assert_eq!(databases[0].database.max_connections, Some(10));
    assert_eq!(databases[1].database.max_connections, Some(20));
}

#[test]
fn test_tenant_database_config_file_invalid_toml() {
    let result = TenantDatabaseConfigFile::from_toml("invalid toml {{{");
    assert!(result.is_err());
}

/// 验证 `[[default_databases]]` 数组表语法可以解析多个默认数据库。
#[test]
fn test_tenant_database_config_file_multiple_default_databases() {
    let toml_str = r#"
        [[tenants]]
        tenant_id = "1"
        [tenants.database]
        url = "sqlite::memory:"

        [[default_databases]]
        url = "postgres://user:pass@localhost:5432/default_db"
        max_connections = 10

        [[default_databases]]
        url = "postgres://user:pass@localhost:5432/default_db_fallback"
        max_connections = 5
    "#;

    let config = TenantDatabaseConfigFile::from_toml(toml_str).unwrap();
    let defaults = config.default_databases.expect("default_databases should be parsed");
    assert_eq!(defaults.len(), 2, "should parse 2 default databases");
    assert_eq!(defaults[0].url, "postgres://user:pass@localhost:5432/default_db");
    assert_eq!(defaults[0].max_connections, Some(10));
    assert_eq!(defaults[1].url, "postgres://user:pass@localhost:5432/default_db_fallback");
    assert_eq!(defaults[1].max_connections, Some(5));
}

/// 验证旧的单数 `[default_database]` 写法仍然兼容（被解析为单元素列表）。
#[test]
fn test_tenant_database_config_file_legacy_single_default_database() {
    let toml_str = r#"
        [[tenants]]
        tenant_id = "1"
        [tenants.database]
        url = "sqlite::memory:"

        [default_database]
        url = "postgres://user:pass@localhost:5432/legacy_db"
        max_connections = 3
    "#;

    let config = TenantDatabaseConfigFile::from_toml(toml_str).unwrap();
    let defaults = config.default_databases.expect("legacy default_database should be parsed via alias");
    assert_eq!(defaults.len(), 1, "legacy single config should be parsed as 1-element list");
    assert_eq!(defaults[0].url, "postgres://user:pass@localhost:5432/legacy_db");
    assert_eq!(defaults[0].max_connections, Some(3));
}

/// 验证重复的 `[default_database]` 表头会报错（这是 TOML 标准的限制，
/// 用户必须改用 `[[default_databases]]` 数组表语法）。
#[test]
fn test_tenant_database_config_file_duplicate_default_database_table_header_errors() {
    // 同一个 [table] 表头重复定义 - TOML 标准禁止，应报错
    let toml_str = r#"
        [[tenants]]
        tenant_id = "1"
        [tenants.database]
        url = "sqlite::memory:"

        [default_database]
        url = "postgres://user:pass@localhost:5432/db1"

        [default_database]
        url = "postgres://user:pass@localhost:5432/db2"
    "#;

    let result = TenantDatabaseConfigFile::from_toml(toml_str);
    assert!(result.is_err(), "duplicate [default_database] table header should error per TOML spec");
}

// ===========================================================================
// errors 模块测试
// ===========================================================================

#[test]
fn test_sea_orm_ext_error_tenant_not_found() {
    let err = SeaOrmExtError::TenantNotFound("t1".to_string());
    assert!(err.to_string().contains("Tenant not found"));
    assert!(err.to_string().contains("t1"));
}

#[test]
fn test_sea_orm_ext_error_tenant_id_required() {
    let err = SeaOrmExtError::TenantIdRequired;
    assert!(err.to_string().contains("required"));
}

#[test]
fn test_sea_orm_ext_error_connection_store_not_initialized() {
    let err = SeaOrmExtError::ConnectionStoreNotInitialized;
    assert!(err.to_string().contains("Connection store"));
}

#[test]
fn test_sea_orm_ext_error_tenant_store_not_initialized() {
    let err = SeaOrmExtError::TenantStoreNotInitialized;
    assert!(err.to_string().contains("Tenant store"));
}

#[test]
fn test_sea_orm_ext_error_tenant_config_not_set() {
    let err = SeaOrmExtError::TenantConfigNotSet;
    assert!(err.to_string().contains("config"));
}

#[test]
fn test_sea_orm_ext_error_invalid_tenant_mode() {
    let err = SeaOrmExtError::InvalidTenantMode("sharding".to_string());
    assert!(err.to_string().contains("sharding"));
}

#[test]
fn test_sea_orm_ext_error_field_fill_handler_not_found() {
    let err = SeaOrmExtError::FieldFillHandlerNotFound;
    assert!(err.to_string().contains("Field fill handler"));
}

#[test]
fn test_sea_orm_ext_error_id_generator_not_initialized() {
    let err = SeaOrmExtError::IdGeneratorNotInitialized;
    assert!(err.to_string().contains("ID generator"));
}

#[test]
fn test_sea_orm_ext_error_config() {
    let err = SeaOrmExtError::Config("bad config".to_string());
    assert!(err.to_string().contains("bad config"));
}

#[test]
fn test_sea_orm_ext_error_database_from_db_err() {
    let db_err = sea_orm::DbErr::Custom("test error".to_string());
    let ext_err: SeaOrmExtError = db_err.into();
    assert!(matches!(ext_err, SeaOrmExtError::Database(_)));
}

// ===========================================================================
// fill 模块测试
// ===========================================================================

#[test]
#[serial]
fn test_id_generator_set_and_get() {
    reset_global_state();

    assert!(get_id_generator().is_none());

    set_id_generator(Box::new(TestIdGenerator::new()));
    assert!(get_id_generator().is_some());

    let gen = get_id_generator().unwrap();
    let val = gen.generate();
    assert!(matches!(val, Value::BigInt(Some(_))));

    clear_id_generator();
    assert!(get_id_generator().is_none());
}

#[test]
#[serial]
fn test_uuid_id_generator() {
    reset_global_state();

    let gen = UuidIdGenerator::new();
    let val = gen.generate();

    match val {
        Value::String(Some(s)) => {
            assert_eq!(s.len(), 36);
            assert!(s.contains('-'));
        }
        _ => panic!("Expected String value from UuidIdGenerator"),
    }

    let typed = gen.generate_for_type("users", "id", "String");
    assert!(typed.is_some());

    let untyped = gen.generate_for_type("users", "id", "i64");
    assert!(untyped.is_none());
}

#[test]
#[serial]
fn test_typed_id_generator() {
    reset_global_state();

    let gen = TypedIdGenerator::new();

    let val_i64 = gen.generate_for_type("test", "id", "i64");
    assert!(val_i64.is_some());

    let val_i32 = gen.generate_for_type("test", "id", "i32");
    assert!(val_i32.is_some());

    let val_string = gen.generate_for_type("test", "id", "String");
    assert!(val_string.is_some());
    if let Value::String(Some(s)) = val_string.unwrap() {
        assert!(!s.is_empty());
    } else {
        panic!("Expected String value");
    }

    let val_unknown = gen.generate_for_type("test", "id", "f64");
    assert!(val_unknown.is_none());
}

#[test]
#[serial]
fn test_typed_id_generator_with_start() {
    let gen = TypedIdGenerator::with_int_start(5000);
    let val = gen.generate();
    match val {
        Value::BigInt(Some(v)) => assert_eq!(v, 5000),
        _ => panic!("Expected BigInt"),
    }
}

#[cfg(feature = "summer")]
fn extract_snowflake_value(val: Value) -> i64 {
    match val {
        Value::BigInt(Some(v)) => v,
        _ => panic!("Expected BigInt from SnowflakeIdGenerator"),
    }
}

#[test]
#[cfg(feature = "summer")]
#[serial]
fn test_snowflake_id_generator_single_thread_unique() {
    let gen = sea_orm_ext::plugin::sea_orm_ext::SnowflakeIdGenerator::new(1);

    let mut ids = HashSet::with_capacity(5000);
    for _ in 0..5000 {
        let id = extract_snowflake_value(gen.generate());
        assert_ne!(id, 0);
        assert!(ids.insert(id), "duplicate snowflake id: {}", id);
    }

    // 同一 worker 内（时间戳+序列号）保证 ID 单调递增
    let generated: Vec<i64> = std::iter::from_fn(|| Some(extract_snowflake_value(gen.generate())))
        .take(1000)
        .collect();
    for w in generated.windows(2) {
        assert!(w[1] > w[0], "snowflake ids should be strictly increasing");
    }
}

#[test]
#[cfg(feature = "summer")]
#[serial]
fn test_snowflake_id_generator_concurrent_unique() {
    use std::sync::Barrier;

    const THREADS: usize = 8;
    const IDS_PER_THREAD: usize = 20_000;

    let gen = Arc::new(sea_orm_ext::plugin::sea_orm_ext::SnowflakeIdGenerator::new(1));
    let barrier = Arc::new(Barrier::new(THREADS));

    let handles: Vec<_> = (0..THREADS)
        .map(|_| {
            let gen = Arc::clone(&gen);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                let mut local = Vec::with_capacity(IDS_PER_THREAD);
                for _ in 0..IDS_PER_THREAD {
                    local.push(extract_snowflake_value(gen.generate()));
                }
                local
            })
        })
        .collect();

    let mut all_ids = HashSet::with_capacity(THREADS * IDS_PER_THREAD);
    for h in handles {
        for id in h.join().unwrap() {
            assert!(
                all_ids.insert(id),
                "duplicate snowflake id under concurrency: {}",
                id
            );
        }
    }
    assert_eq!(all_ids.len(), THREADS * IDS_PER_THREAD);
}

#[test]
#[serial]
fn test_field_fill_handler_set_and_get() {
    reset_global_state();

    assert!(get_field_fill_handler().is_none());

    set_field_fill_handler(Box::new(TestFillHandler::new("test_user")));
    assert!(get_field_fill_handler().is_some());

    let handler = get_field_fill_handler().unwrap();
    let val = handler.fill("products", "created_by", FieldFillOperation::Insert);
    assert!(val.is_some());

    clear_field_fill_handler();
    assert!(get_field_fill_handler().is_none());
}

#[test]
#[serial]
fn test_field_fill_handler_no_match() {
    let handler = TestFillHandler::new("test_user");
    let val = handler.fill("products", "unknown_field", FieldFillOperation::Insert);
    assert!(val.is_none());
}

// ===========================================================================
// soft_delete 模块测试
// ===========================================================================

#[test]
fn test_soft_delete_trait_default_values() {
    let default_val = Product::soft_delete_default();
    let del_val = Product::soft_delete_del();

    match default_val {
        Value::Int(Some(0)) => {}
        _ => panic!("Expected default value 0, got {:?}", default_val),
    }

    match del_val {
        Value::Int(Some(1)) => {}
        _ => panic!("Expected del value 1, got {:?}", del_val),
    }
}

// ===========================================================================
// log 模块测试
// ===========================================================================

#[test]
#[serial]
fn test_sql_log_toggle() {
    reset_global_state();

    assert!(!is_sql_log_enabled());

    enable_sql_log();
    assert!(is_sql_log_enabled());

    disable_sql_log();
    assert!(!is_sql_log_enabled());

    set_sql_log_enabled(true);
    assert!(is_sql_log_enabled());

    set_sql_log_enabled(false);
    assert!(!is_sql_log_enabled());
}

// ===========================================================================
// tenant 模块测试
// ===========================================================================

#[test]
#[serial]
fn test_tenant_config_default() {
    reset_global_state();

    let config = TenantConfig::default();
    assert!(!config.enabled);
    assert_eq!(config.mode, TenantMode::Table);
    assert!(config.default_tenant_id.is_none());
    assert!(config.ignored_tables.is_empty());
}

#[test]
#[serial]
fn test_is_tenant_enabled_default() {
    reset_global_state();
    assert!(!is_tenant_enabled());
}

#[test]
#[serial]
fn test_is_tenant_enforced() {
    reset_global_state();

    assert!(!is_tenant_enforced());

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: None,
        ignored_tables: HashSet::new(),
    });
    assert!(is_tenant_enforced());

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Database,
        default_tenant_id: None,
        ignored_tables: HashSet::new(),
    });
    assert!(!is_tenant_enforced(), "Database mode should not be enforced");
}

#[test]
#[serial]
fn test_is_table_tenant_ignored() {
    reset_global_state();

    assert!(!is_table_tenant_ignored("products"));

    let mut ignored = HashSet::new();
    ignored.insert("products".to_string());
    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: None,
        ignored_tables: ignored,
    });

    assert!(is_table_tenant_ignored("products"));
    assert!(!is_table_tenant_ignored("orders"));
}

#[test]
#[serial]
fn test_get_tenant_mode() {
    reset_global_state();

    assert!(get_tenant_mode().is_none());

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Database,
        default_tenant_id: None,
        ignored_tables: HashSet::new(),
    });
    assert_eq!(get_tenant_mode(), Some(TenantMode::Database));
}

#[test]
#[serial]
fn test_tenant_context_set_and_get() {
    reset_global_state();

    assert!(get_tenant_context().is_none());

    set_tenant_context(TenantContext { tenant_id: Value::String(Some("42".to_string())) });
    let ctx = get_tenant_context();
    assert!(ctx.is_some());
    assert_eq!(ctx.unwrap().tenant_id, Value::String(Some("42".to_string())));

    clear_tenant_context();
    assert!(get_tenant_context().is_none());
}

#[test]
#[serial]
fn test_tenant_guard_raii() {
    reset_global_state();

    assert!(get_tenant_context().is_none());

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let ctx = get_tenant_context();
        assert!(ctx.is_some());
    }

    assert!(get_tenant_context().is_none(), "TenantGuard should clear context on drop");
}

#[test]
#[serial]
fn test_get_current_tenant_id_priority() {
    reset_global_state();

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: Some(Value::String(Some("99".to_string()))),
        ignored_tables: HashSet::new(),
    });

    let id = get_current_tenant_id();
    assert_eq!(id.unwrap(), Value::String(Some("99".to_string())), "Should use default_tenant_id");

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let id = get_current_tenant_id();
        assert_eq!(id.unwrap(), Value::String(Some("1".to_string())), "TenantGuard should take priority");
    }

    let provider = TestTenantIdProvider::new();
    provider.set(Value::String(Some("7".to_string())));
    set_tenant_id_provider(provider.handle());

    let id = get_current_tenant_id();
    assert_eq!(id.unwrap(), Value::String(Some("7".to_string())), "Provider should be used when no guard");
}

#[test]
#[serial]
fn test_try_get_tenant_id_success() {
    reset_global_state();

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: Some(Value::String(Some("1".to_string()))),
        ignored_tables: HashSet::new(),
    });

    let result = try_get_tenant_id();
    assert!(result.is_ok());
}

#[test]
#[serial]
fn test_try_get_tenant_id_failure() {
    reset_global_state();

    let result = try_get_tenant_id();
    assert!(result.is_err());
}

#[test]
#[serial]
fn test_tenant_id_provider() {
    reset_global_state();

    let provider = TestTenantIdProvider::new();
    provider.set(Value::String(Some("42".to_string())));
    set_tenant_id_provider(provider.handle());

    let id = get_current_tenant_id();
    assert!(id.is_some());
    assert_eq!(id.unwrap(), Value::String(Some("42".to_string())));
}

// ===========================================================================
// tenant_store 模块测试
// ===========================================================================

#[tokio::test]
#[serial]
async fn test_hashmap_connection_store_insert_and_get() {
    reset_global_state();

    let store = HashMapConnectionStore::new();
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);

    let db = create_sqlite_db().await;
    store.insert(Value::String(Some("1".to_string())), db).unwrap();

    assert!(!store.is_empty());
    assert_eq!(store.len(), 1);

    let result = store.get(&Value::String(Some("1".to_string())));
    assert!(result.is_some());

    let result = store.get(&Value::String(Some("999".to_string())));
    assert!(result.is_none());
}

#[tokio::test]
#[serial]
async fn test_hashmap_connection_store_remove() {
    reset_global_state();

    let store = HashMapConnectionStore::new();
    let db = create_sqlite_db().await;
    store.insert(Value::String(Some("1".to_string())), db).unwrap();
    assert_eq!(store.len(), 1);

    store.remove(&Value::String(Some("1".to_string()))).unwrap();
    assert_eq!(store.len(), 0);
    assert!(store.get(&Value::String(Some("1".to_string()))).is_none());
}

#[tokio::test]
#[serial]
async fn test_hashmap_connection_store_get_all_tenants() {
    reset_global_state();

    let store = HashMapConnectionStore::new();
    let db1 = create_sqlite_db().await;
    let db2 = create_sqlite_db().await;
    store.insert(Value::String(Some("1".to_string())), db1).unwrap();
    store.insert(Value::String(Some("2".to_string())), db2).unwrap();

    let tenants = store.get_all_tenants();
    assert_eq!(tenants.len(), 2);
}

#[tokio::test]
#[serial]
async fn test_hashmap_connection_store_invalid_tenant_id_type() {
    reset_global_state();

    let store = HashMapConnectionStore::new();
    let db = create_sqlite_db().await;

    let result = store.insert(Value::Float(Some(1.0)), db);
    assert!(result.is_err());
}

#[tokio::test]
#[serial]
async fn test_hashmap_connection_store_bigint_tenant_id() {
    reset_global_state();

    let store = HashMapConnectionStore::new();
    let db = create_sqlite_db().await;

    store.insert(Value::String(Some("100".to_string())), db).unwrap();
    let result = store.get(&Value::String(Some("100".to_string())));
    assert!(result.is_some());
}

// ===========================================================================
// sea_orm_ext_connection 模块测试
// ===========================================================================

#[tokio::test]
#[serial]
async fn test_sea_orm_ext_connection_new() {
    reset_global_state();

    let db = create_sqlite_db().await;
    let ext_db = SeaOrmExtConnection::new(db);

    assert_eq!(ext_db.get_database_backend(), sea_orm::DbBackend::Sqlite);
}

#[tokio::test]
#[serial]
async fn test_sea_orm_ext_connection_inner() {
    reset_global_state();

    let db = create_sqlite_db().await;
    let ext_db = SeaOrmExtConnection::new(db);

    let _inner = ext_db.inner();
}

#[tokio::test]
#[serial]
async fn test_sea_orm_ext_connection_into_inner() {
    reset_global_state();

    let db = create_sqlite_db().await;
    let ext_db = SeaOrmExtConnection::new(db);

    let _raw = ext_db.into_inner();
}

#[tokio::test]
#[serial]
async fn test_sea_orm_ext_connection_deref() {
    reset_global_state();

    let db = create_sqlite_db().await;
    let ext_db = SeaOrmExtConnection::new(db);

    let backend = (*ext_db).get_database_backend();
    assert_eq!(backend, sea_orm::DbBackend::Sqlite);
}

#[tokio::test]
#[serial]
async fn test_sea_orm_ext_connection_insert_and_query() {
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("log_test")));
    enable_sql_log();

    let db = create_logging_sqlite_db().await;
    setup_product_table(&db).await;

    let inserted = new_product("Logged Product", Some(999.0)).insert(&db).await.unwrap();
    assert_eq!(inserted.name, "Logged Product");
    assert_eq!(inserted.created_by, Some("log_test".to_string()));

    let found = Product::find_by_id(inserted.id).one(&db).await.unwrap();
    assert!(found.is_some());

    disable_sql_log();
}

#[tokio::test]
#[serial]
async fn test_sea_orm_ext_connection_update() {
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("log_upd")));
    enable_sql_log();

    let db = create_logging_sqlite_db().await;
    setup_product_table(&db).await;

    let inserted = new_product("Before Update", Some(50.0)).insert(&db).await.unwrap();

    let mut am: ProductActiveModel = inserted.into();
    am.name = Set("After Update".to_string());
    let updated = am.update(&db).await.unwrap();
    assert_eq!(updated.name, "After Update");
    assert_eq!(updated.updated_by, Some("log_upd".to_string()));

    disable_sql_log();
}

#[tokio::test]
#[serial]
async fn test_sea_orm_ext_connection_soft_delete() {
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("log_del")));
    enable_sql_log();

    let db = create_logging_sqlite_db().await;
    setup_product_table(&db).await;

    let inserted = new_product("To Soft Delete", None).insert(&db).await.unwrap();
    let am: ProductActiveModel = inserted.into();
    let result: Result<sea_orm::DeleteResult, sea_orm::DbErr> = am.delete(&db).await;
    assert!(result.is_err());

    let active = find_active_products(&db).await;
    assert_eq!(active.len(), 0);

    disable_sql_log();
}

#[tokio::test]
#[serial]
async fn test_sea_orm_ext_connection_batch_operations() {
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("log_batch")));
    enable_sql_log();

    let db = create_logging_sqlite_db().await;
    setup_product_table(&db).await;

    let models = vec![
        new_product("Batch Log 1", None),
        new_product("Batch Log 2", None),
    ];
    let results = Product::insert_many_with_fill(models, &db).await.unwrap();
    assert_eq!(results.len(), 2);

    let ams: Vec<ProductActiveModel> = results.into_iter().map(|m| m.into()).collect();
    let del_result = Product::delete_many_soft(ams, &db).await.unwrap();
    assert_eq!(del_result.rows_affected, 2);

    disable_sql_log();
}

// ===========================================================================
// tenant_db 便捷函数测试
// ===========================================================================

#[tokio::test]
#[serial]
async fn test_tenant_db_not_enabled_returns_default() {
    reset_global_state();

    let db = create_sqlite_db().await;
    let result = tenant_db(&db);
    assert!(result.is_ok());
}

#[tokio::test]
#[serial]
async fn test_tenant_db_table_mode_returns_default() {
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("table_db_test")));

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: Some(Value::String(Some("1".to_string()))),
        ignored_tables: HashSet::new(),
    });

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    let result = tenant_db(&db);
    assert!(result.is_ok());

    let tenant_conn = result.unwrap();
    new_product("TableMode Product", Some(50.0)).insert(&tenant_conn).await.unwrap();
    let results = Product::find().all(&tenant_conn).await.unwrap();
    assert_eq!(results.len(), 1);
}

#[tokio::test]
#[serial]
async fn test_tenant_db_database_mode_no_tenant_id_errors() {
    reset_global_state();

    set_tenant_id_provider(Arc::new(TestTenantIdProvider::new()));

    let store = Arc::new(HashMapConnectionStore::new());
    let db1 = create_sqlite_db().await;
    store.insert(Value::String(Some("1".to_string())), db1).unwrap();

    set_tenant_store(store);
    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Database,
        default_tenant_id: None,
        ignored_tables: HashSet::new(),
    });

    let default_db = create_sqlite_db().await;
    let result = tenant_db(&default_db);
    assert!(result.is_err());
}

#[tokio::test]
#[serial]
async fn test_tenant_db_for_explicit_tenant() {
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("db_for_test")));

    let store = Arc::new(HashMapConnectionStore::new());
    let db1 = create_sqlite_db().await;
    setup_product_table(&db1).await;
    store.insert(Value::String(Some("1".to_string())), db1).unwrap();

    set_tenant_store(store);
    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Database,
        default_tenant_id: None,
        ignored_tables: HashSet::new(),
    });

    let default_db = create_sqlite_db().await;

    let result = tenant_db_for(&default_db, &Value::String(Some("1".to_string())));
    assert!(result.is_ok());
    let tenant_conn = result.unwrap();
    new_product("Explicit Tenant Product", Some(88.0)).insert(&tenant_conn).await.unwrap();
    let results = Product::find().all(&tenant_conn).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "Explicit Tenant Product");

    let result = tenant_db_for(&default_db, &Value::String(Some("999".to_string())));
    assert!(result.is_err());
}

// ===========================================================================
// TenantDatabaseProvider 测试
// ===========================================================================

#[test]
#[serial]
fn test_tenant_database_provider_provide() {
    reset_global_state();

    let mut databases = HashMap::new();
    databases.insert(Value::String(Some("1".to_string())), ConnectOptions::new("sqlite::memory:"));
    databases.insert(Value::String(Some("2".to_string())), ConnectOptions::new("sqlite::memory:"));

    let provider = TestTenantDatabaseProvider::new(databases);
    let provided = provider.provide();
    assert_eq!(provided.len(), 2);
    assert!(provided.contains_key(&Value::String(Some("1".to_string()))));
    assert!(provided.contains_key(&Value::String(Some("2".to_string()))));
}

// ===========================================================================
// TenantSelectExt / TenantEntityExt 测试
// ===========================================================================

#[tokio::test]
#[serial]
async fn test_tenant_select_ext_no_enforcement() {
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("select_ext")));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    new_product("P1", None).insert(&db).await.unwrap();
    new_product("P2", None).insert(&db).await.unwrap();

    let results = Product::find().all(&db).await.unwrap();
    assert_eq!(results.len(), 2, "Without tenant enforcement, all records should be returned");
}

#[tokio::test]
#[serial]
async fn test_tenant_select_ext_with_enforcement() {
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("select_ext_tenant")));

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: Some(Value::String(Some("1".to_string()))),
        ignored_tables: HashSet::new(),
    });

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        new_product("T1-P1", None).insert(&db).await.unwrap();
    }
    {
        let _guard = TenantGuard::set(Value::String(Some("2".to_string())));
        new_product("T2-P1", None).insert(&db).await.unwrap();
    }

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let results = Product::find().all(&db).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "T1-P1");
    }
}

// ===========================================================================
// 综合集成测试：完整 CRUD 生命周期
// ===========================================================================

#[tokio::test]
#[serial]
async fn test_full_crud_lifecycle_with_tenant_and_soft_delete() {
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("lifecycle")));

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: None,
        ignored_tables: HashSet::new(),
    });

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    let p1 = {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        new_product("Lifecycle Product", Some(100.0)).insert(&db).await.unwrap()
    };

    assert!(p1.id > 0);
    assert_eq!(p1.name, "Lifecycle Product");
    assert_eq!(p1.created_by, Some("lifecycle".to_string()));
    assert_eq!(p1.tenant_id, Some("1".to_string()));
    assert_eq!(p1.is_deleted, 0);

    let updated = {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let mut am: ProductActiveModel = p1.clone().into();
        am.price = Set(Some(150.0));
        am.update(&db).await.unwrap()
    };
    assert_eq!(updated.price, Some(150.0));
    assert_eq!(updated.updated_by, Some("lifecycle".to_string()));

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let am: ProductActiveModel = p1.into();
        let result: Result<sea_orm::DeleteResult, sea_orm::DbErr> = am.delete(&db).await;
        assert!(result.is_err(), "Soft delete should return DbErr");
    }

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let active = Product::find().all(&db).await.unwrap();
        assert_eq!(active.len(), 0, "Soft-deleted record should not appear in find()");
    }

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let all = Product::find_with_deleted().all(&db).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].is_deleted, 1);
    }
}

#[tokio::test]
#[serial]
async fn test_batch_operations_full_lifecycle() {
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("batch_lifecycle")));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    let models = vec![
        new_product("BL-1", Some(10.0)),
        new_product("BL-2", Some(20.0)),
        new_product("BL-3", Some(30.0)),
    ];

    let inserted = Product::insert_many_with_fill(models, &db).await.unwrap();
    assert_eq!(inserted.len(), 3);
    for p in &inserted {
        assert!(p.id > 0);
        assert_eq!(p.created_by, Some("batch_lifecycle".to_string()));
        assert_eq!(p.version, 1);
    }

    let mut ams: Vec<ProductActiveModel> = inserted.into_iter().map(|m| {
        let mut am: ProductActiveModel = m.into();
        am.price = Set(Some(999.0));
        am
    }).collect();
    ams[0].price = Set(Some(11.0));
    ams[1].price = Set(Some(22.0));
    ams[2].price = Set(Some(33.0));

    let updated = Product::update_many_with_fill_returning(ams, &db).await.unwrap();
    assert_eq!(updated.len(), 3);
    for p in &updated {
        assert_eq!(p.updated_by, Some("batch_lifecycle".to_string()));
    }

    let ams: Vec<ProductActiveModel> = updated.into_iter().map(|m| m.into()).collect();
    let del_result = Product::delete_many_soft(ams, &db).await.unwrap();
    assert_eq!(del_result.rows_affected, 3);

    let active = find_active_products(&db).await;
    assert_eq!(active.len(), 0);

    let all = Product::find_with_deleted().all(&db).await.unwrap();
    assert_eq!(all.len(), 3);
}

// ===========================================================================
// 数据库隔离模式完整测试
// ===========================================================================

#[tokio::test]
#[serial]
async fn test_database_isolation_crud_per_tenant() {
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("db_crud")));

    let store = Arc::new(HashMapConnectionStore::new());

    let db_a = create_sqlite_db().await;
    setup_product_table(&db_a).await;
    store.insert(Value::String(Some("1".to_string())), db_a).unwrap();

    let db_b = create_sqlite_db().await;
    setup_product_table(&db_b).await;
    store.insert(Value::String(Some("2".to_string())), db_b).unwrap();

    set_tenant_store(store);
    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Database,
        default_tenant_id: None,
        ignored_tables: HashSet::new(),
    });

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let db = get_tenant_database().unwrap().unwrap();
        let p = new_product("Tenant1-Item", Some(50.0)).insert(&db).await.unwrap();
        assert_eq!(p.created_by, Some("db_crud".to_string()));

        let mut am: ProductActiveModel = p.into();
        am.price = Set(Some(75.0));
        let updated = am.update(&db).await.unwrap();
        assert_eq!(updated.price, Some(75.0));
        assert_eq!(updated.updated_by, Some("db_crud".to_string()));

        let found = Product::find_by_id(updated.id).one(&db).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().price, Some(75.0));
    }

    {
        let _guard = TenantGuard::set(Value::String(Some("2".to_string())));
        let db = get_tenant_database().unwrap().unwrap();
        let results = Product::find().all(&db).await.unwrap();
        assert_eq!(results.len(), 0, "tenant 2 should see no records from tenant 1");
    }
}

#[tokio::test]
#[serial]
async fn test_database_isolation_get_database_for_tenant() {
    reset_global_state();

    let store = Arc::new(HashMapConnectionStore::new());
    let db = create_sqlite_db().await;
    store.insert(Value::String(Some("100".to_string())), db).unwrap();

    set_tenant_store(store);
    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Database,
        default_tenant_id: None,
        ignored_tables: HashSet::new(),
    });

    let result = get_database_for_tenant(&Value::String(Some("100".to_string()))).unwrap();
    assert!(result.is_some());

    let result = get_database_for_tenant(&Value::String(Some("999".to_string()))).unwrap();
    assert!(result.is_none());
}

#[tokio::test]
#[serial]
async fn test_tenant_db_database_mode_returns_tenant_conn() {
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("tenant_db_test")));

    let store = Arc::new(HashMapConnectionStore::new());
    let db1 = create_sqlite_db().await;
    setup_product_table(&db1).await;
    store.insert(Value::String(Some("1".to_string())), db1).unwrap();

    set_tenant_store(store);
    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Database,
        default_tenant_id: Some(Value::String(Some("1".to_string()))),
        ignored_tables: HashSet::new(),
    });

    let default_db = create_sqlite_db().await;

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let tenant_conn = tenant_db(&default_db).unwrap();
        new_product("TenantDB Product", Some(100.0)).insert(&tenant_conn).await.unwrap();
        let results = Product::find().all(&tenant_conn).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "TenantDB Product");
    }
}

// ===========================================================================
// TenantIdCodec 加解密测试
// ===========================================================================

use sea_orm_ext::{
    TenantIdCodec, set_tenant_id_codec, get_tenant_id_codec, clear_tenant_id_codec,
    TenantIgnoreGuard,
};

/// 简单的字符串前缀加解密器：encrypt 在前面加 "enc:" 前缀，decrypt 去除前缀。
struct PrefixCodec;

impl TenantIdCodec for PrefixCodec {
    fn decrypt(&self, encrypted: &str) -> Result<Value, sea_orm::DbErr> {
        if let Some(stripped) = encrypted.strip_prefix("enc:") {
            Ok(Value::String(Some(stripped.to_string())))
        } else {
            Err(sea_orm::DbErr::Custom(format!(
                "invalid encrypted tenant_id: {}",
                encrypted
            )))
        }
    }

    fn encrypt(&self, tenant_id: &Value) -> Result<String, sea_orm::DbErr> {
        match tenant_id {
            Value::String(Some(s)) => Ok(format!("enc:{}", s)),
            _ => Err(sea_orm::DbErr::Custom("unsupported tenant_id type".into())),
        }
    }
}

#[test]
#[serial]
fn test_tenant_id_codec_register_and_get() {
    reset_global_state();

    assert!(get_tenant_id_codec().is_none(), "codec should be None initially");

    set_tenant_id_codec(Arc::new(PrefixCodec));
    assert!(get_tenant_id_codec().is_some(), "codec should be registered");

    clear_tenant_id_codec();
    assert!(get_tenant_id_codec().is_none(), "codec should be cleared");
}

#[test]
#[serial]
fn test_tenant_id_codec_encrypt_decrypt() {
    reset_global_state();

    let codec = PrefixCodec;
    let original = Value::String(Some("tenant-123".to_string()));

    let encrypted = codec.encrypt(&original).unwrap();
    assert_eq!(encrypted, "enc:tenant-123");

    let decrypted = codec.decrypt(&encrypted).unwrap();
    match decrypted {
        Value::String(Some(s)) => assert_eq!(s, "tenant-123"),
        _ => panic!("expected String value"),
    }
}

#[test]
#[serial]
fn test_tenant_id_codec_decrypt_failure() {
    reset_global_state();

    let codec = PrefixCodec;
    let result = codec.decrypt("invalid-no-prefix");
    assert!(result.is_err(), "decrypt should fail for invalid input");
}

// ===========================================================================
// TenantIgnoreGuard 测试（验证 #[ignore_tenant] 宏底层机制）
// ===========================================================================

#[test]
#[serial]
fn test_tenant_ignore_guard_disables_filter() {
    reset_global_state();

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: Some(Value::String(Some("1".to_string()))),
        ignored_tables: HashSet::new(),
    });

    // 启用租户过滤时 is_tenant_enforced 应为 true
    assert!(is_tenant_enforced());

    // 构造 guard 后应禁用
    {
        let _guard = TenantIgnoreGuard::new();
        assert!(!is_tenant_enforced(), "TenantIgnoreGuard should disable enforcement");
    }

    // guard 释放后应恢复
    assert!(is_tenant_enforced(), "enforcement should restore after guard drops");
}

#[test]
#[serial]
fn test_tenant_ignore_guard_nested() {
    reset_global_state();

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: Some(Value::String(Some("1".to_string()))),
        ignored_tables: HashSet::new(),
    });

    assert!(is_tenant_enforced());

    let outer = TenantIgnoreGuard::new();
    assert!(!is_tenant_enforced());

    {
        let _inner = TenantIgnoreGuard::new();
        assert!(!is_tenant_enforced(), "nested guard should still disable");
    }

    // 内层 guard 释放后仍应禁用（外层还在）
    assert!(!is_tenant_enforced(), "outer guard should still be in effect");

    drop(outer);
    assert!(is_tenant_enforced(), "enforcement should restore after all guards drop");
}

// ===========================================================================
// DynamicTenantConfigProvider / TenantManager 测试
// ===========================================================================

use sea_orm_ext::{
    DynamicTenantConfigProvider, TenantConnectionConfig, TenantManager,
    DynamicTenantConfig,
};
use async_trait::async_trait;

/// 测试用 DynamicTenantConfigProvider：返回两个 sqlite 内存库配置
struct TestDynamicConfigProvider {
    configs: Vec<TenantConnectionConfig>,
}

impl TestDynamicConfigProvider {
    fn new() -> Self {
        Self {
            configs: vec![
                TenantConnectionConfig {
                    tenant_id: "tenant-a".to_string(),
                    db_url: "sqlite::memory:".to_string(),
                    db_driver: "sqlite".to_string(),
                    max_connections: Some(5),
                    min_connections: Some(1),
                    connect_timeout_secs: Some(10),
                    acquire_timeout_secs: Some(10),
                    idle_timeout_secs: Some(60),
                    enable_logging: Some(false),
                },
                TenantConnectionConfig {
                    tenant_id: "tenant-b".to_string(),
                    db_url: "sqlite::memory:".to_string(),
                    db_driver: "sqlite".to_string(),
                    max_connections: Some(5),
                    min_connections: Some(1),
                    connect_timeout_secs: Some(10),
                    acquire_timeout_secs: Some(10),
                    idle_timeout_secs: Some(60),
                    enable_logging: Some(false),
                },
            ],
        }
    }
}

#[async_trait]
impl DynamicTenantConfigProvider for TestDynamicConfigProvider {
    async fn load_all(&self, _main_db: &sea_orm::DatabaseConnection) -> Result<Vec<TenantConnectionConfig>, sea_orm::DbErr> {
        Ok(self.configs.clone())
    }

    async fn load_one(&self, _main_db: &sea_orm::DatabaseConnection, tenant_id: &str) -> Result<Option<TenantConnectionConfig>, sea_orm::DbErr> {
        Ok(self.configs.iter().find(|c| c.tenant_id == tenant_id).cloned())
    }
}

#[tokio::test]
#[serial]
async fn test_dynamic_tenant_manager_initialize() {
    reset_global_state();

    let main_db = create_sqlite_db().await;
    let store: Arc<dyn ConnectionStore> = Arc::new(HashMapConnectionStore::new());
    let provider = Arc::new(TestDynamicConfigProvider::new());
    let config = DynamicTenantConfig::default();

    let manager = TenantManager::new(main_db, store, provider, config);

    // 初始化：应加载 2 个租户
    manager.initialize().await.unwrap();

    // 验证缓存
    let tenant_a = manager.inner_get_store().get(&Value::String(Some("tenant-a".to_string())));
    let tenant_b = manager.inner_get_store().get(&Value::String(Some("tenant-b".to_string())));
    assert!(tenant_a.is_some(), "tenant-a should be in cache");
    assert!(tenant_b.is_some(), "tenant-b should be in cache");
}

#[tokio::test]
#[serial]
async fn test_dynamic_tenant_manager_add_and_remove() {
    reset_global_state();

    let main_db = create_sqlite_db().await;
    let store: Arc<dyn ConnectionStore> = Arc::new(HashMapConnectionStore::new());
    let provider = Arc::new(TestDynamicConfigProvider::new());
    let config = DynamicTenantConfig::default();

    let manager = TenantManager::new(main_db, store, provider, config);

    // 初始为空
    assert_eq!(manager.inner_get_store().len(), 0);

    // 添加 tenant-a
    manager.add_tenant("tenant-a").await.unwrap();
    assert_eq!(manager.inner_get_store().len(), 1);
    assert!(manager.inner_get_store().get(&Value::String(Some("tenant-a".to_string()))).is_some());

    // 添加 tenant-b
    manager.add_tenant("tenant-b").await.unwrap();
    assert_eq!(manager.inner_get_store().len(), 2);

    // 移除 tenant-a
    manager.remove_tenant("tenant-a").await.unwrap();
    assert_eq!(manager.inner_get_store().len(), 1);
    assert!(manager.inner_get_store().get(&Value::String(Some("tenant-a".to_string()))).is_none());
    assert!(manager.inner_get_store().get(&Value::String(Some("tenant-b".to_string()))).is_some());
}

#[tokio::test]
#[serial]
async fn test_dynamic_tenant_manager_refresh_cache() {
    reset_global_state();

    let main_db = create_sqlite_db().await;
    let store: Arc<dyn ConnectionStore> = Arc::new(HashMapConnectionStore::new());
    let provider = Arc::new(TestDynamicConfigProvider::new());
    let config = DynamicTenantConfig::default();

    let manager = TenantManager::new(main_db, store, provider, config);

    // 初始化
    manager.initialize().await.unwrap();
    assert_eq!(manager.inner_get_store().len(), 2);

    // 全量重载
    manager.refresh_cache().await.unwrap();
    assert_eq!(manager.inner_get_store().len(), 2, "refresh should reload all tenants");
}

#[tokio::test]
#[serial]
async fn test_dynamic_tenant_manager_update_tenant() {
    reset_global_state();

    let main_db = create_sqlite_db().await;
    let store: Arc<dyn ConnectionStore> = Arc::new(HashMapConnectionStore::new());
    let provider = Arc::new(TestDynamicConfigProvider::new());
    let config = DynamicTenantConfig::default();

    let manager = TenantManager::new(main_db, store, provider, config);

    // 添加 tenant-a
    manager.add_tenant("tenant-a").await.unwrap();
    assert_eq!(manager.inner_get_store().len(), 1);

    // 更新 tenant-a：先移除再重新添加
    manager.update_tenant("tenant-a").await.unwrap();
    assert_eq!(manager.inner_get_store().len(), 1, "update should keep count the same");
    assert!(manager.inner_get_store().get(&Value::String(Some("tenant-a".to_string()))).is_some());
}

#[tokio::test]
#[serial]
async fn test_dynamic_tenant_manager_add_nonexistent_fails() {
    reset_global_state();

    let main_db = create_sqlite_db().await;
    let store: Arc<dyn ConnectionStore> = Arc::new(HashMapConnectionStore::new());
    let provider = Arc::new(TestDynamicConfigProvider::new());
    let config = DynamicTenantConfig::default();

    let manager = TenantManager::new(main_db, store, provider, config);

    // 添加不存在的租户应失败
    let result = manager.add_tenant("nonexistent-tenant").await;
    assert!(result.is_err(), "adding a nonexistent tenant should fail");
}

// ===========================================================================
// 组合场景测试：动态租户 + ignore_tenant + 加解密
// ===========================================================================

use sea_orm_ext::ignore_tenant;

/// 组合场景 1：TenantIdCodec 加解密 + TenantGuard 上下文设置
///
/// 模拟前端传入加密后的 tenant_id → 后端解密 → 设置租户上下文 → 插入数据 → 验证
#[tokio::test]
#[serial]
async fn test_combo_tenant_id_codec_with_guard_and_insert() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("codec_user")));

    // 注册加解密器
    set_tenant_id_codec(Arc::new(PrefixCodec));

    // 开启 Table 模式多租户
    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: None, // 不设默认，强制由 codec 解密后提供
        ignored_tables: HashSet::new(),
    });

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    // 模拟前端传入的加密 tenant_id（"enc:tenant-100"）
    let encrypted_from_frontend = "enc:tenant-100";

    // 后端拿到加密 id 后解密
    let codec = get_tenant_id_codec().expect("codec should be registered");
    let decrypted = codec.decrypt(encrypted_from_frontend).unwrap();
    match &decrypted {
        Value::String(Some(s)) => assert_eq!(s, "tenant-100"),
        _ => panic!("decrypted value should be String"),
    }

    // 用解密后的 tenant_id 设置上下文并插入数据
    {
        let _guard = TenantGuard::set(decrypted.clone());
        let inserted = new_product("Codec Test Product", Some(50.0))
            .insert(&db)
            .await
            .unwrap();
        assert_eq!(
            inserted.tenant_id,
            Some("tenant-100".to_string()),
            "inserted record should have decrypted tenant_id"
        );
    }

    // 验证：用解密后的 tenant_id 查询应能查到记录
    {
        let _guard = TenantGuard::set(decrypted);
        let results = Product::find().all(&db).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Codec Test Product");
        assert_eq!(results[0].tenant_id, Some("tenant-100".to_string()));
    }

    // 验证：未设置 tenant_id 时查询应返回空（除非有默认）
    {
        let results = Product::find().all(&db).await.unwrap();
        assert_eq!(results.len(), 0, "without tenant context, no records visible");
    }

    clear_tenant_id_codec();
}

/// 组合场景 2：动态租户管理初始化 + #[ignore_tenant] 跨租户查询
///
/// 验证动态租户管理初始化的连接可以正常使用，
/// 且 #[ignore_tenant] 宏在 Table 模式下能跳过 WHERE 过滤。
#[tokio::test]
#[serial]
async fn test_combo_dynamic_tenant_with_ignore_tenant_macro() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("combo_user")));

    // 开启 Table 模式多租户（动态租户管理仅在 Database 模式生效，此处仅测 ignore_tenant）
    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: Some(Value::String(Some("t1".to_string()))),
        ignored_tables: HashSet::new(),
    });

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    // 在不同租户上下文下插入数据
    {
        let _guard = TenantGuard::set(Value::String(Some("t1".to_string())));
        new_product("T1-Combo-A", Some(10.0)).insert(&db).await.unwrap();
        new_product("T1-Combo-B", Some(20.0)).insert(&db).await.unwrap();
    }
    {
        let _guard = TenantGuard::set(Value::String(Some("t2".to_string())));
        new_product("T2-Combo-A", Some(100.0)).insert(&db).await.unwrap();
    }

    // 正常查询：租户 t1 只能看到 2 条
    {
        let _guard = TenantGuard::set(Value::String(Some("t1".to_string())));
        let results = Product::find().all(&db).await.unwrap();
        assert_eq!(results.len(), 2, "tenant t1 should see 2 records");
    }

    // 使用 #[ignore_tenant] 跨租户查询：应看到全部 3 条
    let all_count = count_all_products_with_ignore_tenant(&db).await;
    assert_eq!(all_count, 3, "ignore_tenant should see all 3 records across tenants");

    // 宏函数返回后，租户过滤应恢复
    assert!(is_tenant_enforced(), "tenant filter should restore after macro call");
}

/// 辅助函数：被 #[ignore_tenant] 标记，跨租户查询所有产品
#[ignore_tenant]
async fn count_all_products_with_ignore_tenant(db: &sea_orm::DatabaseConnection) -> usize {
    // 函数体内租户过滤应被禁用
    assert!(!is_tenant_enforced(), "inside #[ignore_tenant], filter should be disabled");
    Product::find().all(db).await.unwrap().len()
}

/// 组合场景 3：TenantIdCodec 解密失败时不应设置租户上下文
///
/// 模拟前端传入无效的加密 tenant_id → 解密失败 → 跳过上下文设置 → 使用默认库
#[tokio::test]
#[serial]
async fn test_combo_tenant_id_codec_decrypt_failure_skips_context() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("fail_user")));

    // 注册加解密器
    set_tenant_id_codec(Arc::new(PrefixCodec));

    // 开启 Table 模式多租户，设置默认 tenant_id
    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: Some(Value::String(Some("default-t".to_string()))),
        ignored_tables: HashSet::new(),
    });

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    // 模拟前端传入无效的加密 tenant_id（无 "enc:" 前缀）
    let invalid_encrypted = "invalid-no-prefix";

    // 解密应失败
    let codec = get_tenant_id_codec().expect("codec should be registered");
    let result = codec.decrypt(invalid_encrypted);
    assert!(result.is_err(), "decrypt should fail for invalid input");

    // 解密失败后，业务层应跳过设置租户上下文，使用 default_tenant_id
    // 这里模拟 TenantLayer 的行为：解密失败时不调用 set_tenant_context
    // 因此 get_current_tenant_id() 应返回 default_tenant_id
    let current = get_current_tenant_id();
    assert_eq!(
        current,
        Some(Value::String(Some("default-t".to_string()))),
        "decrypt failure should fall back to default_tenant_id"
    );

    clear_tenant_id_codec();
}

/// 组合场景 4：动态租户管理 + 多租户数据隔离
///
/// 验证通过 DynamicTenantManager 初始化的连接在不同租户间是隔离的。
#[tokio::test]
#[serial]
async fn test_combo_dynamic_tenant_data_isolation() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("iso_user")));

    // 使用动态租户管理器初始化两个租户连接
    let main_db = create_sqlite_db().await;
    let store: Arc<dyn ConnectionStore> = Arc::new(HashMapConnectionStore::new());
    let provider = Arc::new(TestDynamicConfigProvider::new());
    let config = DynamicTenantConfig::default();

    let manager = TenantManager::new(main_db, store, provider, config);
    manager.initialize().await.unwrap();

    assert_eq!(manager.inner_get_store().len(), 2, "should have 2 tenants initialized");

    // 获取两个租户的连接
    let tenant_a_db = manager
        .inner_get_store()
        .get(&Value::String(Some("tenant-a".to_string())))
        .expect("tenant-a connection should exist");
    let tenant_b_db = manager
        .inner_get_store()
        .get(&Value::String(Some("tenant-b".to_string())))
        .expect("tenant-b connection should exist");

    // 在租户 A 的库中创建表并插入数据
    setup_product_table(&tenant_a_db).await;
    new_product("TenantA-Isolation", Some(100.0))
        .insert(&tenant_a_db)
        .await
        .unwrap();

    // 在租户 B 的库中创建表并插入数据
    setup_product_table(&tenant_b_db).await;
    new_product("TenantB-Isolation", Some(200.0))
        .insert(&tenant_b_db)
        .await
        .unwrap();

    // 验证隔离性：租户 A 的库只能看到 A 的数据
    let a_results = Product::find_without_tenant().all(&tenant_a_db).await.unwrap();
    assert_eq!(a_results.len(), 1, "tenant-a db should have 1 record");
    assert_eq!(a_results[0].name, "TenantA-Isolation");

    // 验证隔离性：租户 B 的库只能看到 B 的数据
    let b_results = Product::find_without_tenant().all(&tenant_b_db).await.unwrap();
    assert_eq!(b_results.len(), 1, "tenant-b db should have 1 record");
    assert_eq!(b_results[0].name, "TenantB-Isolation");

    // 动态移除租户 A 后，缓存中应不再有 A 的连接
    manager.remove_tenant("tenant-a").await.unwrap();
    assert_eq!(manager.inner_get_store().len(), 1, "should have 1 tenant after removal");
    assert!(
        manager.inner_get_store()
            .get(&Value::String(Some("tenant-a".to_string())))
            .is_none(),
        "tenant-a should be removed from cache"
    );
    assert!(
        manager.inner_get_store()
            .get(&Value::String(Some("tenant-b".to_string())))
            .is_some(),
        "tenant-b should still be in cache"
    );
}

/// 组合场景 5：TenantIgnoreGuard 嵌套 + #[ignore_tenant] 宏共存
///
/// 验证手动构造的 TenantIgnoreGuard 与宏生成的 guard 能正确嵌套。
#[tokio::test]
#[serial]
async fn test_combo_manual_guard_with_ignore_tenant_macro() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("nest_user")));

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: Some(Value::String(Some("1".to_string()))),
        ignored_tables: HashSet::new(),
    });

    assert!(is_tenant_enforced(), "filter should be enforced initially");

    // 手动构造外层 guard
    let outer_guard = TenantIgnoreGuard::new();
    assert!(!is_tenant_enforced(), "outer guard should disable filter");

    // 调用被 #[ignore_tenant] 标记的函数（内部会再构造一个 guard）
    let result = nested_ignore_tenant_helper().await;
    assert_eq!(result, 100);

    // 内层 guard 释放后，外层 guard 仍应保持禁用状态
    assert!(!is_tenant_enforced(), "outer guard should still be in effect");

    // 释放外层 guard
    drop(outer_guard);
    assert!(is_tenant_enforced(), "filter should restore after all guards drop");
}

/// 辅助函数：在已存在外层 TenantIgnoreGuard 的情况下被 #[ignore_tenant] 调用
#[ignore_tenant]
async fn nested_ignore_tenant_helper() -> i32 {
    // 即使是嵌套场景，过滤也应被禁用（计数器 > 0）
    assert!(!is_tenant_enforced(), "nested #[ignore_tenant] should keep filter disabled");
    100
}

// ===========================================================================
// SeaOrmExtConnection 自动路由测试（修改方案 v2）
// ===========================================================================

/// 测试 1：get_effective_tenant_mode() 优先级
///
/// provider.get_tenant_mode() > 全局配置 mode
#[test]
#[serial]
fn test_get_effective_tenant_mode_priority() {
    reset_global_state();

    // 先获取 provider（会清除 tenant_config，需在之后重新设置）
    let provider = reset_global_state_with_provider();

    // 全局配置为 table
    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: Some(Value::String(Some("1".to_string()))),
        ignored_tables: HashSet::new(),
    });

    // provider 未设置 mode → 回退全局配置 table
    assert_eq!(get_effective_tenant_mode(), Some(TenantMode::Table));

    // provider 设置 mode = database → 优先使用 provider
    provider.set(Value::String(Some("1".to_string())));
    provider.set_mode(Some("database"));
    assert_eq!(get_effective_tenant_mode(), Some(TenantMode::Database));

    // provider 设置 mode = table
    provider.set_mode(Some("table"));
    assert_eq!(get_effective_tenant_mode(), Some(TenantMode::Table));

    // provider 设置无效 mode → 回退全局配置
    provider.set_mode(Some("invalid"));
    assert_eq!(get_effective_tenant_mode(), Some(TenantMode::Table));

    // provider 设置 None → 回退全局配置
    provider.set_mode(None);
    assert_eq!(get_effective_tenant_mode(), Some(TenantMode::Table));
}

/// 测试 2：is_tenant_enforced() 在 database 模式下返回 false（不注入 WHERE）
#[test]
#[serial]
fn test_is_tenant_enforced_with_runtime_database_mode() {
    reset_global_state();

    // 先获取 provider（会清除 tenant_config，需在之后重新设置）
    let provider = reset_global_state_with_provider();

    // 全局配置为 table，但 provider 指定当前租户为 database 模式
    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: Some(Value::String(Some("1".to_string()))),
        ignored_tables: HashSet::new(),
    });

    provider.set(Value::String(Some("1".to_string())));

    // provider mode = database → 不注入 WHERE
    provider.set_mode(Some("database"));
    assert!(!is_tenant_enforced(), "database mode should not enforce WHERE injection");

    // provider mode = table → 注入 WHERE
    provider.set_mode(Some("table"));
    assert!(is_tenant_enforced(), "table mode should enforce WHERE injection");

    // TenantIgnoreGuard 生效时 → 不注入 WHERE
    provider.set_mode(Some("table"));
    let _guard = TenantIgnoreGuard::new();
    assert!(!is_tenant_enforced(), "TenantIgnoreGuard should disable enforcement");
}

/// 测试 3：SeaOrmExtConnection 在 database 模式下自动路由到租户库
///
/// 验证：业务层直接用 self.db，框架自动路由到租户专属库
#[tokio::test]
#[serial]
async fn test_sea_orm_ext_connection_auto_routing_database_mode() {
    init_logging();
    reset_global_state();
    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("auto_route")));

    // 全局配置为 table（模拟混合模式项目）
    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: None,
        ignored_tables: HashSet::new(),
    });

    // 创建主库（SeaOrmExtConnection 包装）
    let main_db = create_sqlite_db().await;
    setup_product_table(&main_db).await;
    // 主库插入一条数据（用 TenantIgnoreGuard 跳过 tenant_id 注入，便于测试 setup）
    {
        let _guard = TenantIgnoreGuard::new();
        new_product("Main-Product", Some(100.0)).insert(&main_db).await.unwrap();
    }

    // 创建租户库（独立的 sqlite::memory:）
    let tenant_db_conn = create_sqlite_db().await;
    setup_product_table(&tenant_db_conn).await;
    // 租户库插入一条数据
    {
        let _guard = TenantIgnoreGuard::new();
        new_product("Tenant-Product", Some(200.0)).insert(&tenant_db_conn).await.unwrap();
    }

    // 注册租户连接到 ConnectionStore（需在 reset_global_state_with_provider 之前）
    let store: Arc<dyn ConnectionStore> = Arc::new(HashMapConnectionStore::new());
    store.insert_ext(
        Value::String(Some("tenant-1".to_string())),
        SeaOrmExtConnection::new(tenant_db_conn.clone()),
    ).unwrap();

    // 设置 provider：当前租户为 tenant-1，模式为 database
    // 注意：reset_global_state_with_provider 会清除 tenant_store，需在之后重新设置
    let provider = reset_global_state_with_provider();
    // 重新设置 tenant_config（reset_global_state_with_provider 会清除）
    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: None,
        ignored_tables: HashSet::new(),
    });
    // 重新注册 tenant_store
    set_tenant_store(store);
    provider.set(Value::String(Some("tenant-1".to_string())));
    provider.set_mode(Some("database"));

    // 关键验证：用 SeaOrmExtConnection（包装主库）查询，应自动路由到租户库
    let ext_conn = SeaOrmExtConnection::new(main_db.clone());

    // 自动路由到租户库 → 应看到 "Tenant-Product"，看不到 "Main-Product"
    let results = Product::find().all(&ext_conn).await.unwrap();
    assert_eq!(results.len(), 1, "should route to tenant db and see only tenant data");
    assert_eq!(results[0].name, "Tenant-Product");

    // 验证 is_tenant_enforced() 返回 false（database 模式不注入 WHERE）
    assert!(!is_tenant_enforced(), "database mode should not inject WHERE tenant_id");
}

/// 测试 4：TenantIgnoreGuard 让 SeaOrmExtConnection 走主库
///
/// 验证：查询全局表时用 TenantIgnoreGuard 临时走主库
#[tokio::test]
#[serial]
async fn test_sea_orm_ext_connection_ignore_guard_uses_main_db() {
    init_logging();
    reset_global_state();
    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("guard_test")));

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: None,
        ignored_tables: HashSet::new(),
    });

    // 主库有 2 条数据（用 TenantIgnoreGuard 跳过 tenant_id 注入）
    let main_db = create_sqlite_db().await;
    setup_product_table(&main_db).await;
    {
        let _guard = TenantIgnoreGuard::new();
        new_product("Main-A", Some(10.0)).insert(&main_db).await.unwrap();
        new_product("Main-B", Some(20.0)).insert(&main_db).await.unwrap();
    }

    // 租户库有 1 条数据
    let tenant_db_conn = create_sqlite_db().await;
    setup_product_table(&tenant_db_conn).await;
    {
        let _guard = TenantIgnoreGuard::new();
        new_product("Tenant-A", Some(100.0)).insert(&tenant_db_conn).await.unwrap();
    }

    // 注册租户连接到 ConnectionStore（需在 reset_global_state_with_provider 之前）
    let store: Arc<dyn ConnectionStore> = Arc::new(HashMapConnectionStore::new());
    store.insert_ext(
        Value::String(Some("tenant-1".to_string())),
        SeaOrmExtConnection::new(tenant_db_conn.clone()),
    ).unwrap();

    // 注意：reset_global_state_with_provider 会清除 tenant_store 和 tenant_config
    let provider = reset_global_state_with_provider();
    // 重新设置 tenant_config
    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: None,
        ignored_tables: HashSet::new(),
    });
    // 重新注册 tenant_store
    set_tenant_store(store);
    provider.set(Value::String(Some("tenant-1".to_string())));
    provider.set_mode(Some("database"));

    let ext_conn = SeaOrmExtConnection::new(main_db.clone());

    // 不带 guard：自动路由到租户库，看到 1 条
    let tenant_results = Product::find().all(&ext_conn).await.unwrap();
    assert_eq!(tenant_results.len(), 1, "without guard, should route to tenant db");
    assert_eq!(tenant_results[0].name, "Tenant-A");

    // 带 guard：走主库，看到 2 条
    {
        let _guard = TenantIgnoreGuard::new();
        let main_results = Product::find().all(&ext_conn).await.unwrap();
        assert_eq!(main_results.len(), 2, "with guard, should use main db");
        assert!(main_results.iter().any(|r| r.name == "Main-A"));
        assert!(main_results.iter().any(|r| r.name == "Main-B"));
    }

    // guard 释放后：恢复自动路由到租户库
    let tenant_results_after = Product::find().all(&ext_conn).await.unwrap();
    assert_eq!(tenant_results_after.len(), 1, "after guard drop, should route to tenant db again");
    assert_eq!(tenant_results_after[0].name, "Tenant-A");
}

/// 测试 5：table 模式下 SeaOrmExtConnection 走主库（不路由）
#[tokio::test]
#[serial]
async fn test_sea_orm_ext_connection_table_mode_uses_main_db() {
    init_logging();
    reset_global_state();
    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("table_mode")));

    // 先获取 provider（会清除 tenant_config，需在之后重新设置）
    let provider = reset_global_state_with_provider();

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: Some(Value::String(Some("1".to_string()))),
        ignored_tables: HashSet::new(),
    });

    let main_db = create_sqlite_db().await;
    setup_product_table(&main_db).await;

    // 插入不同租户的数据（TenantGuard 设置当前租户上下文，宏会自动填充 tenant_id）
    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        new_product("T1-Product", Some(10.0)).insert(&main_db).await.unwrap();
    }
    {
        let _guard = TenantGuard::set(Value::String(Some("2".to_string())));
        new_product("T2-Product", Some(20.0)).insert(&main_db).await.unwrap();
    }

    let ext_conn = SeaOrmExtConnection::new(main_db.clone());

    // table 模式 + 租户 1 上下文 → 走主库 + WHERE tenant_id = '1'
    provider.set(Value::String(Some("1".to_string())));
    provider.set_mode(Some("table"));

    let results = Product::find().all(&ext_conn).await.unwrap();
    assert_eq!(results.len(), 1, "table mode should filter by tenant_id");
    assert_eq!(results[0].name, "T1-Product");
}

/// 测试 6：get_tenant_database_for_current() 在不同场景下的返回值
#[test]
#[serial]
fn test_get_tenant_database_for_current_scenarios() {
    reset_global_state();

    // 场景 1：租户未启用 → None
    assert!(get_tenant_database_for_current().unwrap().is_none());

    // 场景 2：启用 table 模式 → None（不路由）
    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: Some(Value::String(Some("1".to_string()))),
        ignored_tables: HashSet::new(),
    });
    assert!(get_tenant_database_for_current().unwrap().is_none());

    // 场景 3：TenantIgnoreGuard 生效 → None（走主库）
    // 注意：reset_global_state_with_provider 会清除 tenant_config，需在之后重新设置
    let provider = reset_global_state_with_provider();
    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: Some(Value::String(Some("1".to_string()))),
        ignored_tables: HashSet::new(),
    });
    provider.set(Value::String(Some("1".to_string())));
    provider.set_mode(Some("database"));
    let _guard = TenantIgnoreGuard::new();
    assert!(get_tenant_database_for_current().unwrap().is_none());

    // 场景 4：database 模式 + 无租户库 → None（回退主库）
    drop(_guard);
    let store: Arc<dyn ConnectionStore> = Arc::new(HashMapConnectionStore::new());
    set_tenant_store(store);
    // store 为空，查不到连接 → 返回 None（fallback）
    let result = get_tenant_database_for_current().unwrap();
    assert!(result.is_none(), "empty store should return None (fallback to main db)");
}

/// 测试 7：get_database_for_tenant_unchecked() 在 store 未初始化时返回 Ok(None) 走 fallback
///
/// 验证：DynamicTenantPlugin 未启用或启动失败时，业务层调用不受影响，
/// 自动回退到默认数据库（main db）。
#[test]
#[serial]
fn test_get_database_for_tenant_unchecked_store_not_initialized() {
    reset_global_state();

    // 启用租户但未注册 store（模拟 DynamicTenantPlugin 未启动的场景）
    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table, // 全局 table 模式
        default_tenant_id: Some(Value::String(Some("1".to_string()))),
        ignored_tables: HashSet::new(),
    });

    // 未注册 store → 不应报错，返回 Ok(None) 或 Ok(默认库)
    let result = get_database_for_tenant_unchecked(&Value::String(Some("tenant-1".to_string())));
    assert!(result.is_ok(), "should not return Err when store is not initialized");
    // 没有默认库时返回 None
    assert!(result.unwrap().is_none(), "should fallback to None when no default db configured");
}

/// 测试 8：get_database_for_tenant_unchecked() 跳过 mode 检查
///
/// 验证：全局配置为 table 模式，但本函数仍能查到租户连接（用于运行时 database 模式租户）
#[test]
#[serial]
fn test_get_database_for_tenant_unchecked_skips_mode_check() {
    init_logging();
    reset_global_state();

    // 全局配置为 table 模式
    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: None,
        ignored_tables: HashSet::new(),
    });

    // 注册一个租户连接到 store
    let store: Arc<dyn ConnectionStore> = Arc::new(HashMapConnectionStore::new());
    let rt = tokio::runtime::Runtime::new().unwrap();
    let tenant_conn = rt.block_on(async {
        sea_orm::Database::connect("sqlite::memory:").await.unwrap()
    });
    store.insert(
        Value::String(Some("tenant-1".to_string())),
        tenant_conn,
    ).unwrap();
    set_tenant_store(store);

    // 即使全局是 table 模式，unchecked 版本仍能查到连接
    // （用于运行时 provider 指定 database 模式的租户）
    let result = get_database_for_tenant_unchecked(&Value::String(Some("tenant-1".to_string())));
    assert!(result.is_ok(), "should succeed even in global table mode");
    assert!(result.unwrap().is_some(), "should find the tenant connection");

    // 查不存在的租户 → 返回 None（fallback）
    let result = get_database_for_tenant_unchecked(&Value::String(Some("non-existent".to_string())));
    assert!(result.is_ok(), "should not error for unknown tenant");
    assert!(result.unwrap().is_none(), "should return None for unknown tenant (fallback)");
}
