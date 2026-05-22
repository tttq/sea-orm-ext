
mod common;
use common::*;
use sea_orm::ActiveModelTrait;
use sea_orm::ActiveValue::Set;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn test_insert_with_auto_fill() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("admin")));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    let product = new_product("iPhone 16", Some(8999.0));
    let inserted = product.insert(&db).await.unwrap();

    assert!(inserted.id > 0, "id should be auto-generated");
    assert_eq!(inserted.name, "iPhone 16");
    assert_eq!(inserted.price, Some(8999.0));
    assert_eq!(inserted.created_by, Some("admin".to_string()));
    assert_eq!(inserted.updated_by, None);
    assert_eq!(inserted.version, 1);
    assert_eq!(inserted.is_deleted, 0);
    assert_eq!(inserted.tenant_id, None);
}

#[tokio::test]
#[serial]
async fn test_string_id_with_uuid_generator() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(UuidIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("doc_admin")));

    let db = create_sqlite_db().await;
    setup_document_table(&db).await;

    let doc = new_document("My First Doc", Some("Hello world"));
    let inserted = doc.insert(&db).await.unwrap();

    assert!(!inserted.id.is_empty(), "UUID string ID should be generated");
    assert!(inserted.id.len() == 36, "UUID v4 format should be 36 chars with hyphens");
    assert_eq!(inserted.title, "My First Doc");
    assert_eq!(inserted.content, Some("Hello world".to_string()));
    assert_eq!(inserted.created_by, Some("doc_admin".to_string()));
    assert_eq!(inserted.is_deleted, 0);

    let found = Document::find_by_id(inserted.id.clone()).one(&db).await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, inserted.id);

    let all = Document::find().all(&db).await.unwrap();
    assert_eq!(all.len(), 1);
}

#[tokio::test]
#[serial]
async fn test_string_id_with_typed_generator() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TypedIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("typed_admin")));

    let db = create_sqlite_db().await;
    setup_document_table(&db).await;

    let doc1 = new_document("Doc A", None);
    let doc2 = new_document("Doc B", Some("Content B"));
    let inserted1 = doc1.insert(&db).await.unwrap();
    let inserted2 = doc2.insert(&db).await.unwrap();

    assert!(!inserted1.id.is_empty());
    assert!(!inserted2.id.is_empty());
    assert_ne!(inserted1.id, inserted2.id, "Each document should get a unique UUID");

    let all = Document::find().all(&db).await.unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
#[serial]
async fn test_string_id_soft_delete() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(UuidIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("doc_del")));

    let db = create_sqlite_db().await;
    setup_document_table(&db).await;

    let doc = new_document("To Delete", None);
    let inserted = doc.insert(&db).await.unwrap();
    assert!(!inserted.id.is_empty());

    let am: DocumentActiveModel = inserted.into();
    let _ = am.delete(&db).await;

    let active = Document::find().all(&db).await.unwrap();
    assert_eq!(active.len(), 0, "soft-deleted document should not appear in find()");

    let all = Document::find_with_deleted().all(&db).await.unwrap();
    assert_eq!(all.len(), 1, "find_with_deleted() should include soft-deleted");
    assert_eq!(all[0].is_deleted, 1);
}

#[tokio::test]
#[serial]
async fn test_string_id_update_with_fill() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(UuidIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("doc_upd")));

    let db = create_sqlite_db().await;
    setup_document_table(&db).await;

    let doc = new_document("Original Title", Some("Original content"));
    let inserted = doc.insert(&db).await.unwrap();
    assert_eq!(inserted.created_by, Some("doc_upd".to_string()));

    let mut am: DocumentActiveModel = inserted.into();
    am.title = Set("Updated Title".to_string());
    let updated = am.update(&db).await.unwrap();
    assert_eq!(updated.title, "Updated Title");
    assert_eq!(updated.updated_by, Some("doc_upd".to_string()));
}

#[tokio::test]
#[serial]
async fn test_find_by_id() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("admin")));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    let inserted = new_product("MacBook Pro", Some(14999.0)).insert(&db).await.unwrap();

    let found = Product::find_by_id(inserted.id).one(&db).await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "MacBook Pro");
}

#[tokio::test]
#[serial]
async fn test_update_with_auto_fill() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("editor")));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    let inserted = new_product("iPad Air", Some(4999.0)).insert(&db).await.unwrap();
    assert_eq!(inserted.version, 1);

    let mut to_update: ProductActiveModel = inserted.into();
    to_update.price = Set(Some(4599.0));
    let updated = to_update.update(&db).await.unwrap();

    assert_eq!(updated.price, Some(4599.0));
    assert_eq!(updated.updated_by, Some("editor".to_string()));
    assert_eq!(updated.version, 1, "version should be filled by handler on update");
}

#[tokio::test]
#[serial]
async fn test_soft_delete() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("admin")));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    let inserted = new_product("AirPods", Some(1299.0)).insert(&db).await.unwrap();

    let active_before = Product::find().all(&db).await.unwrap();
    assert_eq!(active_before.len(), 1);

    let to_delete: ProductActiveModel = inserted.into();
    let result: Result<sea_orm::DeleteResult, sea_orm::DbErr> = to_delete.delete(&db).await;
    assert!(result.is_err(), "soft delete should return DbErr::Custom");
    assert!(result.unwrap_err().to_string().contains("soft-deleted"));

    let active_after = Product::find().all(&db).await.unwrap();
    assert_eq!(active_after.len(), 0, "find() should not return soft-deleted records");

    let all_with_deleted = Product::find_with_deleted().all(&db).await.unwrap();
    assert_eq!(all_with_deleted.len(), 1, "find_with_deleted() should return soft-deleted records");
    assert_eq!(all_with_deleted[0].is_deleted, 1);
}

#[tokio::test]
#[serial]
async fn test_find_active_vs_find_with_deleted() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("admin")));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    new_product("Active 1", None).insert(&db).await.unwrap();
    new_product("Active 2", None).insert(&db).await.unwrap();
    let p3 = new_product("To Delete", None).insert(&db).await.unwrap();

    let am: ProductActiveModel = p3.into();
    let _ = am.delete(&db).await;

    let active = Product::find().all(&db).await.unwrap();
    assert_eq!(active.len(), 2, "find() should return 2 active records");

    let all = Product::find_with_deleted().all(&db).await.unwrap();
    assert_eq!(all.len(), 3, "find_with_deleted() should return all 3 records");
}

#[tokio::test]
#[serial]
async fn test_auto_fill_without_handler() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    let product = new_product("No Handler", None);
    let inserted = product.insert(&db).await.unwrap();

    assert!(inserted.id > 0);
    assert_eq!(inserted.created_by, None, "created_by should be None when no handler set");
    assert_eq!(inserted.version, 0, "version should be 0 when no handler fills it");
}

#[tokio::test]
#[serial]
async fn test_entity_without_tenant() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("system")));

    let db = create_sqlite_db().await;
    setup_sys_config_table(&db).await;

    let config = SysConfigActiveModel {
        key: Set("app.name".to_string()),
        value: Set("Template Admin".to_string()),
        ..Default::default()
    };
    let inserted = config.insert(&db).await.unwrap();

    assert!(inserted.id > 0);
    assert_eq!(inserted.key, "app.name");
    assert_eq!(inserted.created_by, Some("system".to_string()));
}

#[tokio::test]
#[serial]
async fn test_update_without_handler() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("creator")));

    let db = create_sqlite_db().await;
    setup_product_table(&db).await;

    let inserted = new_product("Initial", Some(100.0)).insert(&db).await.unwrap();
    assert_eq!(inserted.created_by, Some("creator".to_string()));

    clear_field_fill_handler();

    let mut am: ProductActiveModel = inserted.into();
    am.price = Set(Some(200.0));
    let updated = am.update(&db).await.unwrap();
    assert_eq!(updated.price, Some(200.0));
    assert_eq!(updated.updated_by, None, "updated_by should be None when no handler set");
}

#[tokio::test]
#[serial]
async fn test_sys_config_soft_delete_and_find() {
    init_logging();
    reset_global_state();

    set_id_generator(Box::new(TestIdGenerator::new()));
    set_field_fill_handler(Box::new(TestFillHandler::new("cfg_admin")));

    let db = create_sqlite_db().await;
    setup_sys_config_table(&db).await;

    let c1 = SysConfigActiveModel {
        key: Set("key1".to_string()),
        value: Set("val1".to_string()),
        ..Default::default()
    };
    let c2 = SysConfigActiveModel {
        key: Set("key2".to_string()),
        value: Set("val2".to_string()),
        ..Default::default()
    };
    let inserted1 = c1.insert(&db).await.unwrap();
    let _inserted2 = c2.insert(&db).await.unwrap();

    let am: SysConfigActiveModel = inserted1.into();
    let _ = am.delete(&db).await;

    let active = SysConfig::find().all(&db).await.unwrap();
    assert_eq!(active.len(), 1, "find() should return 1 active config");

    let all = SysConfig::find_with_deleted().all(&db).await.unwrap();
    assert_eq!(all.len(), 2, "find_with_deleted() should return all 2 configs");
}
