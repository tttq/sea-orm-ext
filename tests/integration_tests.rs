
mod common;
use common::*;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectOptions, QueryFilter};
use sea_query::Value;
use serial_test::serial;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[tokio::test]
#[serial]
async fn test_table_isolation_insert_and_query() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("tenant_test")));

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
        let inserted = new_product("Tenant1 Product A", Some(100.0)).insert(&db).await.unwrap();
        assert_eq!(inserted.tenant_id, Some("1".to_string()));
    }

    {
        let _guard = TenantGuard::set(Value::String(Some("2".to_string())));
        let inserted = new_product("Tenant2 Product B", Some(200.0)).insert(&db).await.unwrap();
        assert_eq!(inserted.tenant_id, Some("2".to_string()));
    }

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let results = Product::find().all(&db).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Tenant1 Product A");
    }

    {
        let _guard = TenantGuard::set(Value::String(Some("2".to_string())));
        let results = Product::find().all(&db).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Tenant2 Product B");
    }
}

#[tokio::test]
#[serial]
async fn test_table_isolation_tenant_guard_cleanup() {
    init_logging();
    reset_global_state();

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: None,
        ignored_tables: HashSet::new(),
    });

    {
        let _guard = TenantGuard::set(Value::String(Some("42".to_string())));
        let ctx = get_tenant_context();
        assert!(ctx.is_some());
    }

    let ctx = get_tenant_context();
    assert!(ctx.is_none(), "tenant context should be cleared after guard drops");
}

#[tokio::test]
#[serial]
async fn test_table_isolation_ignored_tables() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("admin")));

    let mut ignored = HashSet::new();
    ignored.insert("products".to_string());

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: Some(Value::String(Some("1".to_string()))),
        ignored_tables: ignored,
    });

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    {
        let _guard = TenantGuard::set(Value::String(Some("99".to_string())));
        let inserted = new_product("Ignored Table Product", None).insert(&db).await.unwrap();
        assert_eq!(inserted.tenant_id, Some("99".to_string()));
    }

    let all = Product::find_with_deleted().all(&db).await.unwrap();
    assert_eq!(all.len(), 1);
}

#[tokio::test]
#[serial]
async fn test_table_isolation_batch_insert_with_tenant() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("batch_tenant")));

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: Some(Value::String(Some("5".to_string()))),
        ignored_tables: HashSet::new(),
    });

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    {
        let _guard = TenantGuard::set(Value::String(Some("5".to_string())));
        let models = vec![
            new_product("T5-A", None),
            new_product("T5-B", None),
        ];
        let results = Product::insert_many_with_fill(models, &db).await.unwrap();
        for r in &results {
            assert_eq!(r.tenant_id, Some("5".to_string()));
            assert_eq!(r.created_by, Some("batch_tenant".to_string()));
        }
    }
}

#[tokio::test]
#[serial]
async fn test_table_isolation_update_and_delete_with_tenant() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("tenant_upd")));

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
        new_product("T1-Original", Some(100.0)).insert(&db).await.unwrap()
    };

    let _p2 = {
        let _guard = TenantGuard::set(Value::String(Some("2".to_string())));
        new_product("T2-Original", Some(200.0)).insert(&db).await.unwrap()
    };

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let mut am: ProductActiveModel = p1.clone().into();
        am.price = Set(Some(150.0));
        let updated = am.update(&db).await.unwrap();
        assert_eq!(updated.price, Some(150.0));
        assert_eq!(updated.updated_by, Some("tenant_upd".to_string()));
    }

    {
        let _guard = TenantGuard::set(Value::String(Some("2".to_string())));
        let results = Product::find().all(&db).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].price, Some(200.0));
    }

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let am: ProductActiveModel = p1.into();
        let result: Result<sea_orm::DeleteResult, sea_orm::DbErr> = am.delete(&db).await;
        assert!(result.is_err());
    }

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let active = Product::find()
            .filter(ProductColumn::TenantId.eq("1"))
            .all(&db)
            .await.unwrap();
        assert_eq!(active.len(), 0, "soft-deleted record should not appear in find() with tenant filter");
    }
}

#[tokio::test]
#[serial]
async fn test_database_isolation_separate_connections() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("db_isolation")));

    let store = Arc::new(HashMapConnectionStore::new());

    let db1 = create_sqlite_db().await;
    setup_product_table(&db1).await;
    store.insert(Value::String(Some("1".to_string())), db1).unwrap();

    let db2 = create_sqlite_db().await;
    setup_product_table(&db2).await;
    store.insert(Value::String(Some("2".to_string())), db2).unwrap();

    set_tenant_store(store.clone());
    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Database,
        default_tenant_id: Some(Value::String(Some("1".to_string()))),
        ignored_tables: HashSet::new(),
    });

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let tenant_db = get_tenant_database().unwrap();
        assert!(tenant_db.is_some());
        let db = tenant_db.unwrap();
        new_product("DB1-Product", Some(100.0)).insert(&db).await.unwrap();
    }

    {
        let _guard = TenantGuard::set(Value::String(Some("2".to_string())));
        let tenant_db = get_tenant_database().unwrap();
        assert!(tenant_db.is_some());
        let db = tenant_db.unwrap();
        new_product("DB2-Product", Some(200.0)).insert(&db).await.unwrap();
    }

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let db = get_tenant_database().unwrap().unwrap();
        let results = Product::find().all(&db).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "DB1-Product");
    }

    {
        let _guard = TenantGuard::set(Value::String(Some("2".to_string())));
        let db = get_tenant_database().unwrap().unwrap();
        let results = Product::find().all(&db).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "DB2-Product");
    }
}

#[tokio::test]
#[serial]
async fn test_database_isolation_tenant_database_manager() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));

    let store = Arc::new(HashMapConnectionStore::new());

    let db10 = create_sqlite_db().await;
    setup_product_table(&db10).await;
    store.insert(Value::String(Some("10".to_string())), db10).unwrap();

    let db20 = create_sqlite_db().await;
    setup_product_table(&db20).await;
    store.insert(Value::String(Some("20".to_string())), db20).unwrap();

    set_tenant_store(store.clone());
    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Database,
        default_tenant_id: Some(Value::String(Some("10".to_string()))),
        ignored_tables: HashSet::new(),
    });

    set_field_fill_handler(Box::new(TestFillHandler::new("db_mgr")));

    {
        let _guard = TenantGuard::set(Value::String(Some("10".to_string())));
        let db = get_tenant_database().unwrap().unwrap();
        new_product("Mgr-T10", None).insert(&db).await.unwrap();
    }

    {
        let _guard = TenantGuard::set(Value::String(Some("20".to_string())));
        let db = get_tenant_database().unwrap().unwrap();
        new_product("Mgr-T20", None).insert(&db).await.unwrap();
    }

    {
        let _guard = TenantGuard::set(Value::String(Some("10".to_string())));
        let db = get_tenant_database().unwrap().unwrap();
        let results = Product::find().all(&db).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Mgr-T10");
    }

    {
        let _guard = TenantGuard::set(Value::String(Some("20".to_string())));
        let db = get_tenant_database().unwrap().unwrap();
        let results = Product::find().all(&db).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Mgr-T20");
    }
}

#[tokio::test]
#[serial]
async fn test_database_isolation_get_database_for_tenant() {
    init_logging();
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
async fn test_database_isolation_crud_per_tenant() {
    init_logging();
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
async fn test_sea_orm_ext_connection_insert_and_query() {
    init_logging();
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
async fn test_auto_fill_tenant_insert_with_id_and_tenant() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("order_admin")));

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: Some(Value::String(Some("1".to_string()))),
        ignored_tables: HashSet::new(),
    });

    let db = create_sqlite_db().await;
    setup_order_table(&db).await;

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let order = new_order("Widget", 5);
        let inserted = order.insert(&db).await.unwrap();

        assert!(inserted.id > 0, "ID should be auto-generated by IdGenerator");
        assert_eq!(inserted.product_name, "Widget");
        assert_eq!(inserted.quantity, 5);
        assert_eq!(inserted.created_by, Some("order_admin".to_string()));
        assert_eq!(inserted.tenant_id, Some("1".to_string()), "tenant_id should be auto-injected");
    }

    {
        let _guard = TenantGuard::set(Value::String(Some("2".to_string())));
        let order = new_order("Gadget", 10);
        let inserted = order.insert(&db).await.unwrap();
        assert_eq!(inserted.tenant_id, Some("2".to_string()));
    }
}

#[tokio::test]
#[serial]
async fn test_auto_fill_tenant_update_with_fill() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("order_upd")));

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: None,
        ignored_tables: HashSet::new(),
    });

    let db = create_sqlite_db().await;
    setup_order_table(&db).await;

    let inserted = {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        new_order("Original", 3).insert(&db).await.unwrap()
    };

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let mut am: OrderActiveModel = inserted.into();
        am.quantity = Set(7);
        let updated = am.update(&db).await.unwrap();
        assert_eq!(updated.quantity, 7);
        assert_eq!(updated.updated_by, Some("order_upd".to_string()));
    }
}

#[tokio::test]
#[serial]
async fn test_auto_fill_tenant_find_with_tenant() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("order_qry")));

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: Some(Value::String(Some("1".to_string()))),
        ignored_tables: HashSet::new(),
    });

    let db = create_sqlite_db().await;
    setup_order_table(&db).await;

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        new_order("T1-Order", 1).insert(&db).await.unwrap();
    }

    {
        let _guard = TenantGuard::set(Value::String(Some("2".to_string())));
        new_order("T2-Order", 2).insert(&db).await.unwrap();
    }

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let results = Order::find().all(&db).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].product_name, "T1-Order");
    }

    {
        let _guard = TenantGuard::set(Value::String(Some("2".to_string())));
        let results = Order::find().all(&db).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].product_name, "T2-Order");
    }
}

#[tokio::test]
#[serial]
async fn test_auto_fill_tenant_batch_insert() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("batch_order")));

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: Some(Value::String(Some("5".to_string()))),
        ignored_tables: HashSet::new(),
    });

    let db = create_sqlite_db().await;
    setup_order_table(&db).await;

    {
        let _guard = TenantGuard::set(Value::String(Some("5".to_string())));
        let orders = vec![
            new_order("Batch-A", 1),
            new_order("Batch-B", 2),
        ];
        let results = Order::insert_many_with_fill(orders, &db).await.unwrap();
        assert_eq!(results.len(), 2);
        for r in &results {
            assert!(r.id > 0, "ID should be auto-generated");
            assert_eq!(r.created_by, Some("batch_order".to_string()));
            assert_eq!(r.tenant_id, Some("5".to_string()));
        }
    }
}

#[tokio::test]
#[serial]
async fn test_auto_fill_tenant_delete_is_physical() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("del_order")));

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: None,
        ignored_tables: HashSet::new(),
    });

    let db = create_sqlite_db().await;
    setup_order_table(&db).await;

    let inserted = {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        new_order("ToDelete", 1).insert(&db).await.unwrap()
    };

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let am: OrderActiveModel = inserted.into();
        let result = am.delete(&db).await;
        assert!(result.is_ok(), "DeriveAutoFillTenant should use physical DELETE (no soft delete)");
    }

    let all = Order::find_without_tenant().all(&db).await.unwrap();
    assert_eq!(all.len(), 0, "record should be physically deleted");
}

#[tokio::test]
#[serial]
async fn test_sea_orm_ext_connection_update() {
    init_logging();
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
    init_logging();
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
async fn test_sql_log_toggle() {
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

#[tokio::test]
#[serial]
async fn test_sea_orm_ext_connection_batch_operations() {
    init_logging();
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

#[tokio::test]
#[serial]
async fn test_tenant_id_provider_returns_id() {
    init_logging();
    reset_global_state();

    let provider = TestTenantIdProvider::new();
    provider.set(Value::String(Some("42".to_string())));
    set_tenant_id_provider(provider.handle());

    let tenant_id = get_current_tenant_id();
    assert!(tenant_id.is_some());
    assert_eq!(tenant_id.unwrap(), Value::String(Some("42".to_string())));
}

#[tokio::test]
#[serial]
async fn test_tenant_id_provider_priority() {
    init_logging();
    reset_global_state();

    let provider = TestTenantIdProvider::new();
    provider.set(Value::String(Some("99".to_string())));
    set_tenant_id_provider(provider.handle());

    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let tenant_id = get_current_tenant_id();
        assert!(tenant_id.is_some());
        assert_eq!(tenant_id.unwrap(), Value::String(Some("1".to_string())));
    }

    let tenant_id = get_current_tenant_id();
    assert!(tenant_id.is_some());
    assert_eq!(tenant_id.unwrap(), Value::String(Some("99".to_string())));
}

#[tokio::test]
#[serial]
async fn test_tenant_id_provider_with_table_mode() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("provider_tenant")));

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: None,
        ignored_tables: HashSet::new(),
    });

    let provider = TestTenantIdProvider::new();
    provider.set(Value::String(Some("7".to_string())));
    set_tenant_id_provider(provider.handle());

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    let inserted = new_product("Provider Product", Some(77.0)).insert(&db).await.unwrap();
    assert_eq!(inserted.tenant_id, Some("7".to_string()));
}

#[tokio::test]
#[serial]
async fn test_tenant_db_table_mode_returns_default() {
    init_logging();
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
async fn test_tenant_db_database_mode_returns_tenant_conn() {
    init_logging();
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

#[tokio::test]
#[serial]
async fn test_tenant_db_database_mode_no_tenant_id_errors() {
    init_logging();
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
    init_logging();
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

#[test]
#[serial]
fn test_tenant_database_provider_provide() {
    init_logging();
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
