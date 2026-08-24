use sea_orm_ext::plugin::sea_orm_ext::{FieldFillHandlerComponent, SeaOrmExtPlugin, SnowflakeIdGenerator};
use sea_orm_ext::plugin::tenant::{TenantDatabaseProviderComponent, TenantIdProviderComponent, TenantPlugin};
use sea_orm_ext::{FieldFillHandler, FieldFillOperation, TenantDatabaseProvider, TenantIdProvider};
use sea_orm_ext::set_id_generator;
use sea_orm::ConnectOptions;
use sea_query::Value;
use std::collections::HashMap;
use std::sync::Arc;
use summer::plugin::MutableComponentRegistry;
use summer::App;

struct MyTenantIdProvider;

impl TenantIdProvider for MyTenantIdProvider {
    fn get_tenant_id(&self) -> Option<Value> {
        None
    }
}

struct MyTenantDatabaseProvider;

impl TenantDatabaseProvider for MyTenantDatabaseProvider {
    fn provide(&self) -> HashMap<Value, ConnectOptions> {
        let mut map = HashMap::new();
        map.insert(
            Value::BigInt(Some(1)),
            ConnectOptions::new("sqlite:///tenant_1.db"),
        );
        map.insert(
            Value::BigInt(Some(2)),
            ConnectOptions::new("sqlite:///tenant_2.db"),
        );
        map
    }
}

struct MyFieldFillHandler;

impl FieldFillHandler for MyFieldFillHandler {
    fn fill(&self, _entity_name: &str, field_name: &str, operation: FieldFillOperation) -> Option<Value> {
        match (field_name, operation) {
            ("created_by", FieldFillOperation::Insert) => Some("custom_user".into()),
            ("updated_by", FieldFillOperation::Update) => Some("custom_user".into()),
            _ => None,
        }
    }
}

fn build_app() -> summer::app::AppBuilder {
    let mut app = App::new();
    app.add_plugin(TenantPlugin::new())
        .add_plugin(SeaOrmExtPlugin::new())
        .add_component(TenantIdProviderComponent::new(Arc::new(MyTenantIdProvider)))
        .add_component(TenantDatabaseProviderComponent::new(Arc::new(MyTenantDatabaseProvider)))
        .add_component(FieldFillHandlerComponent::new(Arc::new(MyFieldFillHandler)));
    app
}

#[tokio::main]
async fn main() {
    set_id_generator(Box::new(SnowflakeIdGenerator::new(1)));
    let mut app = build_app();
    app.run().await;
}
