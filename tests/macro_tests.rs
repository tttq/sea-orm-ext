
mod common;
use common::*;
use sea_orm::ActiveModelTrait;
use sea_orm::ActiveValue::Set;
use sea_query::Value;
use serial_test::serial;
use std::collections::HashSet;

#[tokio::test]
#[serial]
async fn test_batch_insert_with_fill() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("batch_user")));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    let models = vec![
        new_product("Product A", Some(100.0)),
        new_product("Product B", Some(200.0)),
        new_product("Product C", None),
    ];

    let results = Product::insert_many_with_fill(models, &db).await.unwrap();

    assert_eq!(results.len(), 3);
    for r in &results {
        assert!(r.id > 0);
        assert_eq!(r.created_by, Some("batch_user".to_string()));
        assert_eq!(r.version, 1);
        assert_eq!(r.is_deleted, 0);
    }
    assert_eq!(results[0].name, "Product A");
    assert_eq!(results[1].price, Some(200.0));
    assert_eq!(results[2].price, None);
}

#[tokio::test]
#[serial]
async fn test_batch_insert_returning() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("batch_user")));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    let models = vec![
        new_product("R1", None),
        new_product("R2", None),
    ];

    let result = Product::insert_many_with_fill_exec(models, &db).await.unwrap();
    assert_eq!(result.rows_affected, 2);
}

#[tokio::test]
#[serial]
async fn test_batch_update_with_fill() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("updater")));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    let m1 = new_product("U1", Some(10.0)).insert(&db).await.unwrap();
    let m2 = new_product("U2", Some(20.0)).insert(&db).await.unwrap();

    let mut am1: ProductActiveModel = m1.into();
    am1.price = Set(Some(15.0));
    let mut am2: ProductActiveModel = m2.into();
    am2.price = Set(Some(25.0));

    let results = Product::update_many_with_fill_returning(vec![am1, am2], &db).await.unwrap();

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].price, Some(15.0));
    assert_eq!(results[1].price, Some(25.0));
    for r in &results {
        assert_eq!(r.updated_by, Some("updater".to_string()));
        assert!(r.version >= 1, "version should be filled by handler on update");
    }
}

#[tokio::test]
#[serial]
async fn test_batch_soft_delete() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("admin")));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    let m1 = new_product("D1", None).insert(&db).await.unwrap();
    let m2 = new_product("D2", None).insert(&db).await.unwrap();
    let _m3 = new_product("D3", None).insert(&db).await.unwrap();

    let am1: ProductActiveModel = m1.into();
    let am2: ProductActiveModel = m2.into();

    let result = Product::delete_many_soft(vec![am1, am2], &db).await.unwrap();
    assert_eq!(result.rows_affected, 2);

    let active = find_active_products(&db).await;
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].name, "D3");

    let all = Product::find_with_deleted().all(&db).await.unwrap();
    assert!(all.len() >= 3);
}

#[tokio::test]
#[serial]
async fn test_batch_insert_without_handler() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    let models = vec![
        new_product("NoFill-A", Some(10.0)),
        new_product("NoFill-B", None),
    ];

    let results = Product::insert_many_with_fill(models, &db).await.unwrap();
    assert_eq!(results.len(), 2);
    for r in &results {
        assert!(r.id > 0);
        assert_eq!(r.created_by, None, "created_by should be None without handler");
        assert_eq!(r.version, 0, "version should be 0 without handler");
    }
}

// =============================================================================
// 批量操作优化（CASE WHEN 单条 SQL）测试用例
// =============================================================================

/// 同时更新多个字段，验证 CASE WHEN 表达式为每个字段都生成正确的批量赋值。
#[tokio::test]
#[serial]
async fn test_batch_update_multiple_fields() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("multi_upd")));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    let m1 = new_product("M1", Some(10.0)).insert(&db).await.unwrap();
    let m2 = new_product("M2", Some(20.0)).insert(&db).await.unwrap();
    let m3 = new_product("M3", Some(30.0)).insert(&db).await.unwrap();

    // 同时更新 name 和 price 两个字段
    let mut am1: ProductActiveModel = m1.into();
    am1.name = Set("M1-Updated".to_string());
    am1.price = Set(Some(11.0));
    let mut am2: ProductActiveModel = m2.into();
    am2.name = Set("M2-Updated".to_string());
    am2.price = Set(Some(22.0));
    // m3 只更新 name，不更新 price（验证 CASE WHEN 中 ELSE 分支保留原值）
    let mut am3: ProductActiveModel = m3.into();
    am3.name = Set("M3-Updated".to_string());

    let results = Product::update_many_with_fill_returning(vec![am1, am2, am3], &db).await.unwrap();

    assert_eq!(results.len(), 3);

    // 按 name 索引结果，便于断言
    let by_name: std::collections::HashMap<String, ProductModel> =
        results.iter().map(|r| (r.name.clone(), r.clone())).collect();

    // 验证 name 字段全部更新
    assert!(by_name.contains_key("M1-Updated"));
    assert!(by_name.contains_key("M2-Updated"));
    assert!(by_name.contains_key("M3-Updated"));

    // 验证 price 字段：m1=11.0, m2=22.0, m3 保留原值 30.0（ELSE 分支）
    assert_eq!(by_name["M1-Updated"].price, Some(11.0));
    assert_eq!(by_name["M2-Updated"].price, Some(22.0));
    assert_eq!(by_name["M3-Updated"].price, Some(30.0), "m3 price should be preserved by CASE WHEN ELSE branch");

    // 验证 fill handler 在批量更新时也生效
    for r in &results {
        assert_eq!(r.updated_by, Some("multi_upd".to_string()));
    }
}

/// 空批次：update_many_with_fill 和 update_many_with_fill_returning 都应直接返回空结果。
#[tokio::test]
#[serial]
async fn test_batch_update_empty_input() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("empty")));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    let empty: Vec<ProductActiveModel> = Vec::new();

    let result = Product::update_many_with_fill(empty.clone(), &db).await.unwrap();
    assert_eq!(result.rows_affected, 0, "empty batch should affect 0 rows");

    let returning = Product::update_many_with_fill_returning(empty, &db).await.unwrap();
    assert!(returning.is_empty(), "empty batch returning should be empty");
}

/// 单条记录的批量更新：边界场景，验证 CASE WHEN 在只有一行时也能正确生成。
#[tokio::test]
#[serial]
async fn test_batch_update_single_record() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("single_upd")));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    let m = new_product("Single", Some(100.0)).insert(&db).await.unwrap();
    let id = m.id;

    let mut am: ProductActiveModel = m.into();
    am.price = Set(Some(150.0));

    let result = Product::update_many_with_fill(vec![am], &db).await.unwrap();
    assert_eq!(result.rows_affected, 1, "single record should affect 1 row");

    // 从数据库重新读取验证
    let reloaded = Product::find_by_id(id).one(&db).await.unwrap().unwrap();
    assert_eq!(reloaded.price, Some(150.0));
    assert_eq!(reloaded.updated_by, Some("single_upd".to_string()));
}

/// update_many_with_fill（不带 returning）：只返回 UpdateResult，不回读 Model。
#[tokio::test]
#[serial]
async fn test_batch_update_without_returning() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("no_ret")));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    let m1 = new_product("NR1", Some(10.0)).insert(&db).await.unwrap();
    let m2 = new_product("NR2", Some(20.0)).insert(&db).await.unwrap();
    let id1 = m1.id;
    let id2 = m2.id;

    let mut am1: ProductActiveModel = m1.into();
    am1.price = Set(Some(15.0));
    let mut am2: ProductActiveModel = m2.into();
    am2.price = Set(Some(25.0));

    let result = Product::update_many_with_fill(vec![am1, am2], &db).await.unwrap();
    assert_eq!(result.rows_affected, 2, "should affect 2 rows");

    // 重新查询验证更新结果
    let all = Product::find().all(&db).await.unwrap();
    let by_id: std::collections::HashMap<i64, ProductModel> =
        all.iter().map(|r| (r.id, r.clone())).collect();
    assert_eq!(by_id.get(&id1).unwrap().price, Some(15.0));
    assert_eq!(by_id.get(&id2).unwrap().price, Some(25.0));
    for r in &all {
        assert_eq!(r.updated_by, Some("no_ret".to_string()));
    }
}

/// 无 fill handler 时批量更新：fill 字段不被填充，但普通字段 CASE WHEN 仍正常工作。
#[tokio::test]
#[serial]
async fn test_batch_update_without_handler() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    let m1 = new_product("NH1", Some(10.0)).insert(&db).await.unwrap();
    let m2 = new_product("NH2", Some(20.0)).insert(&db).await.unwrap();

    let mut am1: ProductActiveModel = m1.into();
    am1.price = Set(Some(11.0));
    let mut am2: ProductActiveModel = m2.into();
    am2.price = Set(Some(22.0));

    let results = Product::update_many_with_fill_returning(vec![am1, am2], &db).await.unwrap();
    assert_eq!(results.len(), 2);
    for r in &results {
        assert_eq!(r.updated_by, None, "updated_by should be None without handler");
    }
    assert_eq!(results[0].price, Some(11.0));
    assert_eq!(results[1].price, Some(22.0));
}

/// 批量更新跨多条数据（>2），验证 CASE WHEN 表达式可以处理多分支。
#[tokio::test]
#[serial]
async fn test_batch_update_many_records() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("batch_big")));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    // 插入 5 条记录
    let mut models = Vec::new();
    for i in 1..=5 {
        models.push(new_product(&format!("P{}", i), Some(i as f64 * 10.0)));
    }
    let inserted = Product::insert_many_with_fill(models, &db).await.unwrap();
    assert_eq!(inserted.len(), 5);

    // 批量更新 5 条记录的 price
    let updates: Vec<ProductActiveModel> = inserted.iter().map(|m| {
        let mut am: ProductActiveModel = m.clone().into();
        am.price = Set(Some(m.id as f64)); // 用 id 作为新 price，便于校验
        am
    }).collect();

    let result = Product::update_many_with_fill(updates, &db).await.unwrap();
    assert_eq!(result.rows_affected, 5);

    let all = Product::find().all(&db).await.unwrap();
    for r in &all {
        assert_eq!(r.price, Some(r.id as f64), "price should equal id after batch update");
    }
}

/// 批量软删除：验证单条 SQL UPDATE 完成 N 条记录的软删除。
#[tokio::test]
#[serial]
async fn test_batch_soft_delete_multiple() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("sd_batch")));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    // 插入 4 条记录
    let models = vec![
        new_product("SD1", None),
        new_product("SD2", None),
        new_product("SD3", None),
        new_product("SD4", None),
    ];
    let inserted = Product::insert_many_with_fill(models, &db).await.unwrap();
    assert_eq!(inserted.len(), 4);

    // 批量软删除前 3 条
    let to_delete: Vec<ProductActiveModel> = inserted[0..3].iter().map(|m| m.clone().into()).collect();
    let result = Product::delete_many_soft(to_delete, &db).await.unwrap();
    assert_eq!(result.rows_affected, 3);

    // find() 只看到 1 条（未删除的）
    let active = find_active_products(&db).await;
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].name, "SD4");

    // find_with_deleted() 看到 4 条
    let all = Product::find_with_deleted().all(&db).await.unwrap();
    assert_eq!(all.len(), 4);
}

/// 租户隔离下的批量更新：只能更新当前租户的记录。
#[tokio::test]
#[serial]
async fn test_batch_update_with_tenant_isolation() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("tenant_batch_upd")));

    set_tenant_config(TenantConfig {
        enabled: true,
        mode: TenantMode::Table,
        default_tenant_id: Some(Value::String(Some("1".to_string()))),
        ignored_tables: HashSet::new(),
    });

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    // 租户 1 插入 2 条
    let (m1_t1, m2_t1) = {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let m1 = new_product("T1-A", Some(10.0)).insert(&db).await.unwrap();
        let m2 = new_product("T1-B", Some(20.0)).insert(&db).await.unwrap();
        (m1, m2)
    };

    // 租户 2 插入 2 条
    let (m1_t2, _m2_t2) = {
        let _guard = TenantGuard::set(Value::String(Some("2".to_string())));
        let m1 = new_product("T2-A", Some(100.0)).insert(&db).await.unwrap();
        let m2 = new_product("T2-B", Some(200.0)).insert(&db).await.unwrap();
        (m1, m2)
    };

    // 在租户 1 上下文中批量更新 4 条（其中 2 条属于租户 2）
    // 预期：只有租户 1 的 2 条会被更新，租户 2 的记录不受影响
    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let mut am1: ProductActiveModel = m1_t1.clone().into();
        am1.price = Set(Some(15.0));
        let mut am2: ProductActiveModel = m2_t1.clone().into();
        am2.price = Set(Some(25.0));
        // 试图更新租户 2 的记录（应被租户过滤忽略）
        let mut am3: ProductActiveModel = m1_t2.clone().into();
        am3.price = Set(Some(999.0));

        let result = Product::update_many_with_fill(vec![am1, am2, am3], &db).await.unwrap();
        // 租户隔离下，WHERE 子句会过滤掉租户 2 的记录，只更新租户 1 的 2 条
        assert_eq!(result.rows_affected, 2, "tenant filter should only affect tenant 1 records");
    }

    // 验证租户 1 的记录已更新
    {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        let all = Product::find().all(&db).await.unwrap();
        assert_eq!(all.len(), 2);
        let by_id: std::collections::HashMap<i64, ProductModel> =
            all.iter().map(|r| (r.id, r.clone())).collect();
        assert_eq!(by_id.get(&m1_t1.id).unwrap().price, Some(15.0));
        assert_eq!(by_id.get(&m2_t1.id).unwrap().price, Some(25.0));
    }

    // 验证租户 2 的记录未被修改
    {
        let _guard = TenantGuard::set(Value::String(Some("2".to_string())));
        let all = Product::find().all(&db).await.unwrap();
        assert_eq!(all.len(), 2);
        let by_id: std::collections::HashMap<i64, ProductModel> =
            all.iter().map(|r| (r.id, r.clone())).collect();
        assert_eq!(by_id.get(&m1_t2.id).unwrap().price, Some(100.0),
            "tenant 2 record should not be modified by tenant 1 batch update");
    }
}

/// ignored_tables 下的批量更新：跳过租户过滤，所有记录都可见。
#[tokio::test]
#[serial]
async fn test_batch_update_ignored_table_skips_tenant() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("ignored_upd")));

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

    // 即使设置了 TenantGuard，products 表的租户过滤也被忽略
    let m1 = {
        let _guard = TenantGuard::set(Value::String(Some("1".to_string())));
        new_product("IG-1", Some(10.0)).insert(&db).await.unwrap()
    };
    let m2 = {
        let _guard = TenantGuard::set(Value::String(Some("2".to_string())));
        new_product("IG-2", Some(20.0)).insert(&db).await.unwrap()
    };

    // ignored_tables 中的表不应注入 tenant_id
    assert_eq!(m1.tenant_id, None, "ignored_tables should skip tenant injection on insert");
    assert_eq!(m2.tenant_id, None);

    // 批量更新不应受租户过滤影响
    let mut am1: ProductActiveModel = m1.clone().into();
    am1.price = Set(Some(11.0));
    let mut am2: ProductActiveModel = m2.clone().into();
    am2.price = Set(Some(22.0));

    let result = Product::update_many_with_fill(vec![am1, am2], &db).await.unwrap();
    assert_eq!(result.rows_affected, 2, "ignored_tables should update all records regardless of tenant");

    let all = Product::find().all(&db).await.unwrap();
    assert_eq!(all.len(), 2, "ignored_tables should see all records");
}

/// 批量插入 + 批量更新 + 批量软删除 组合场景，验证三种批量操作协同工作。
#[tokio::test]
#[serial]
async fn test_batch_operations_combo() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("combo")));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    // 1. 批量插入 3 条
    let models = vec![
        new_product("C1", Some(10.0)),
        new_product("C2", Some(20.0)),
        new_product("C3", Some(30.0)),
    ];
    let inserted = Product::insert_many_with_fill(models, &db).await.unwrap();
    assert_eq!(inserted.len(), 3);

    // 2. 批量更新 3 条
    let updates: Vec<ProductActiveModel> = inserted.iter().map(|m| {
        let mut am: ProductActiveModel = m.clone().into();
        am.price = Set(Some(m.price.unwrap() + 5.0));
        am
    }).collect();
    let upd_result = Product::update_many_with_fill(updates, &db).await.unwrap();
    assert_eq!(upd_result.rows_affected, 3);

    // 3. 批量软删除 2 条
    let to_delete: Vec<ProductActiveModel> = inserted[0..2].iter().map(|m| m.clone().into()).collect();
    let del_result = Product::delete_many_soft(to_delete, &db).await.unwrap();
    assert_eq!(del_result.rows_affected, 2);

    // 4. 验证最终状态：find() 只剩 1 条，find_with_deleted() 有 3 条
    let active = Product::find().all(&db).await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].name, "C3");
    assert_eq!(active[0].price, Some(35.0));

    let all = Product::find_with_deleted().all(&db).await.unwrap();
    assert_eq!(all.len(), 3);
}
