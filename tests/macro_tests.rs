
mod common;
use common::*;
use sea_orm::ActiveModelTrait;
use sea_orm::ActiveValue::Set;
use serial_test::serial;

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

    let result = Product::insert_many_with_fill_returning(models, &db).await.unwrap();
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
