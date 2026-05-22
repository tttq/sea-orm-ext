
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
    assert_eq!(config.max_connections, Some(10));
    assert_eq!(config.min_connections, Some(1));
    assert_eq!(config.connect_timeout_secs, Some(30));
    assert_eq!(config.acquire_timeout_secs, Some(30));
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
    assert!(config.default_database.is_none());
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
    use summer_sea_orm_ext::plugin::tenant::TenantPluginConfig;

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

// ===========================================================================
// errors 模块测试
// ===========================================================================

#[test]
fn test_summer_sea_orm_ext_error_tenant_not_found() {
    let err = SeaOrmExtError::TenantNotFound("t1".to_string());
    assert!(err.to_string().contains("Tenant not found"));
    assert!(err.to_string().contains("t1"));
}

#[test]
fn test_summer_sea_orm_ext_error_tenant_id_required() {
    let err = SeaOrmExtError::TenantIdRequired;
    assert!(err.to_string().contains("required"));
}

#[test]
fn test_summer_sea_orm_ext_error_connection_store_not_initialized() {
    let err = SeaOrmExtError::ConnectionStoreNotInitialized;
    assert!(err.to_string().contains("Connection store"));
}

#[test]
fn test_summer_sea_orm_ext_error_tenant_store_not_initialized() {
    let err = SeaOrmExtError::TenantStoreNotInitialized;
    assert!(err.to_string().contains("Tenant store"));
}

#[test]
fn test_summer_sea_orm_ext_error_tenant_config_not_set() {
    let err = SeaOrmExtError::TenantConfigNotSet;
    assert!(err.to_string().contains("config"));
}

#[test]
fn test_summer_sea_orm_ext_error_invalid_tenant_mode() {
    let err = SeaOrmExtError::InvalidTenantMode("sharding".to_string());
    assert!(err.to_string().contains("sharding"));
}

#[test]
fn test_summer_sea_orm_ext_error_field_fill_handler_not_found() {
    let err = SeaOrmExtError::FieldFillHandlerNotFound;
    assert!(err.to_string().contains("Field fill handler"));
}

#[test]
fn test_summer_sea_orm_ext_error_id_generator_not_initialized() {
    let err = SeaOrmExtError::IdGeneratorNotInitialized;
    assert!(err.to_string().contains("ID generator"));
}

#[test]
fn test_summer_sea_orm_ext_error_config() {
    let err = SeaOrmExtError::Config("bad config".to_string());
    assert!(err.to_string().contains("bad config"));
}

#[test]
fn test_summer_sea_orm_ext_error_database_from_db_err() {
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
// summer_sea_orm_ext_connection 模块测试
// ===========================================================================

#[tokio::test]
#[serial]
async fn test_summer_sea_orm_ext_connection_new() {
    reset_global_state();

    let db = create_sqlite_db().await;
    let ext_db = SeaOrmExtConnection::new(db);

    assert_eq!(ext_db.get_database_backend(), sea_orm::DbBackend::Sqlite);
}

#[tokio::test]
#[serial]
async fn test_summer_sea_orm_ext_connection_inner() {
    reset_global_state();

    let db = create_sqlite_db().await;
    let ext_db = SeaOrmExtConnection::new(db);

    let _inner = ext_db.inner();
}

#[tokio::test]
#[serial]
async fn test_summer_sea_orm_ext_connection_into_inner() {
    reset_global_state();

    let db = create_sqlite_db().await;
    let ext_db = SeaOrmExtConnection::new(db);

    let _raw = ext_db.into_inner();
}

#[tokio::test]
#[serial]
async fn test_summer_sea_orm_ext_connection_deref() {
    reset_global_state();

    let db = create_sqlite_db().await;
    let ext_db = SeaOrmExtConnection::new(db);

    let backend = (*ext_db).get_database_backend();
    assert_eq!(backend, sea_orm::DbBackend::Sqlite);
}

#[tokio::test]
#[serial]
async fn test_summer_sea_orm_ext_connection_insert_and_query() {
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
async fn test_summer_sea_orm_ext_connection_update() {
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
async fn test_summer_sea_orm_ext_connection_soft_delete() {
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
async fn test_summer_sea_orm_ext_connection_batch_operations() {
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
