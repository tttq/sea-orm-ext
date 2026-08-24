use sea_query::Value;
use std::sync::{Arc, RwLock};

/// 字段自动填充的操作类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldFillOperation {
    Insert,
    Update,
}

/// 字段自动填充处理器 trait。
pub trait FieldFillHandler: Send + Sync + 'static {
    fn fill(
        &self,
        entity_name: &str,
        field_name: &str,
        operation: FieldFillOperation,
    ) -> Option<Value>;
}

type SharedFillHandler = Arc<RwLock<Option<Arc<dyn FieldFillHandler>>>>;

static FIELD_FILL_HANDLER: std::sync::OnceLock<SharedFillHandler> = std::sync::OnceLock::new();

fn fill_handler_store() -> &'static SharedFillHandler {
    FIELD_FILL_HANDLER.get_or_init(|| Arc::new(RwLock::new(None)))
}

/// 设置全局字段填充处理器。
pub fn set_field_fill_handler(handler: Box<dyn FieldFillHandler>) {
    let store = fill_handler_store();
    match store.write() {
        Ok(mut guard) => *guard = Some(Arc::from(handler)),
        Err(_) => tracing::error!("FIELD-FILL-HANDLER: lock poisoned, set_field_fill_handler ignored"),
    }
}

/// 清除全局字段填充处理器。
pub fn clear_field_fill_handler() {
    let store = fill_handler_store();
    match store.write() {
        Ok(mut guard) => *guard = None,
        Err(_) => tracing::error!("FIELD-FILL-HANDLER: lock poisoned, clear_field_fill_handler ignored"),
    }
}

/// 获取全局字段填充处理器的 Arc 引用。
///
/// 返回 `Arc<dyn FieldFillHandler>`，调用方持有期间 handler 不会被释放。
/// 即使另一个线程调用了 `set_field_fill_handler` 替换 handler，
/// 已获取的 Arc 仍然指向旧 handler，保证安全。
pub fn get_field_fill_handler() -> Option<Arc<dyn FieldFillHandler>> {
    let store = fill_handler_store();
    match store.read() {
        Ok(guard) => guard.clone(),
        Err(_) => {
            tracing::error!("FIELD-FILL-HANDLER: lock poisoned, get_field_fill_handler returning None");
            None
        }
    }
}

/// ID 生成器 trait。
///
/// 为实体的主键字段提供可插拔的 ID 生成策略。
///
/// # 类型感知生成
///
/// 默认的 `generate()` 方法返回 `i64` 类型的 `Value`，适用于整数主键。
/// 对于 `String`、`Uuid` 等非整数主键，应实现 `generate_for_type()` 方法，
/// 根据目标字段类型返回匹配的 `Value`。
///
/// # 调用优先级
///
/// 宏生成的主键填充代码会按以下顺序尝试：
/// 1. 调用 `generate_for_type(entity, field, field_type)` — 如果返回 `Some(val)`，直接使用
/// 2. 回退到 `generate()` + `ValueType::try_from()` 类型转换
pub trait IdGenerator: Send + Sync + 'static {
    /// 生成一个 ID 值（默认为 i64 类型）。
    ///
    /// 此方法保持向后兼容，适用于整数主键场景。
    /// 对于字符串等非整数主键，请实现 [`generate_for_type`](IdGenerator::generate_for_type)。
    fn generate(&self) -> Value;

    /// 根据目标字段类型生成 ID 值。
    ///
    /// 当实体主键为 `String`、`Uuid` 等非整数类型时，应覆盖此方法。
    ///
    /// # 参数
    ///
    /// - `entity_name` — 实体表名（如 `"users"`）
    /// - `field_name` — 字段名（如 `"id"`）
    /// - `field_type` — 字段的 Rust 类型名（如 `"String"`、`"i64"`、`"i32"`）
    ///
    /// # 返回值
    ///
    /// - `Some(val)` — 使用此值作为 ID
    /// - `None` — 回退到 `generate()` + `ValueType::try_from()` 转换
    fn generate_for_type(&self, _entity_name: &str, _field_name: &str, _field_type: &str) -> Option<Value> {
        None
    }
}

type SharedIdGenerator = Arc<RwLock<Option<Arc<dyn IdGenerator>>>>;

static ID_GENERATOR: std::sync::OnceLock<SharedIdGenerator> = std::sync::OnceLock::new();

fn id_generator_store() -> &'static SharedIdGenerator {
    ID_GENERATOR.get_or_init(|| Arc::new(RwLock::new(None)))
}

/// 设置全局 ID 生成器。
pub fn set_id_generator(generator: Box<dyn IdGenerator>) {
    let store = id_generator_store();
    match store.write() {
        Ok(mut guard) => *guard = Some(Arc::from(generator)),
        Err(_) => tracing::error!("ID-GENERATOR: lock poisoned, set_id_generator ignored"),
    }
}

/// 清除全局 ID 生成器。
pub fn clear_id_generator() {
    let store = id_generator_store();
    match store.write() {
        Ok(mut guard) => *guard = None,
        Err(_) => tracing::error!("ID-GENERATOR: lock poisoned, clear_id_generator ignored"),
    }
}

/// 获取全局 ID 生成器的 Arc 引用。
pub fn get_id_generator() -> Option<Arc<dyn IdGenerator>> {
    let store = id_generator_store();
    match store.read() {
        Ok(guard) => guard.clone(),
        Err(_) => {
            tracing::error!("ID-GENERATOR: lock poisoned, get_id_generator returning None");
            None
        }
    }
}

/// 基于 UUID v4 的字符串 ID 生成器。
///
/// 生成随机的 UUID v4 字符串，适用于主键类型为 `String` 的实体。
///
/// # 用法
///
/// ```ignore
/// use sea_orm_ext::{set_id_generator, UuidIdGenerator};
///
/// set_id_generator(Box::new(UuidIdGenerator::new()));
/// ```
///
/// # 行为
///
/// - `generate()` — 返回 UUID 字符串的 `Value::String`
/// - `generate_for_type()` — 当 `field_type` 为 `"String"` 时返回 UUID 字符串，
///   否则返回 `None`（回退到 `generate()` + `ValueType::try_from()`）
#[derive(Debug, Default)]
pub struct UuidIdGenerator;

impl UuidIdGenerator {
    pub fn new() -> Self {
        Self
    }
}

impl IdGenerator for UuidIdGenerator {
    fn generate(&self) -> Value {
        let uuid = uuid::Uuid::new_v4();
        uuid.to_string().into()
    }

    fn generate_for_type(&self, _entity_name: &str, _field_name: &str, field_type: &str) -> Option<Value> {
        match field_type {
            "String" | "Option<String>" => {
                let uuid = uuid::Uuid::new_v4();
                Some(uuid.to_string().into())
            }
            _ => None,
        }
    }
}

/// 组合多种类型生成器的 ID 生成器。
///
/// 根据目标字段类型自动选择合适的 ID 生成策略：
/// - 整数类型（`i32`、`i64`、`u32`、`u64`）→ 使用内置的整数生成器
/// - 字符串类型（`String`）→ 使用 UUID v4 生成器
///
/// # 用法
///
/// ```ignore
/// use sea_orm_ext::{set_id_generator, TypedIdGenerator};
///
/// // 使用默认配置：整数用原子计数器，字符串用 UUID
/// set_id_generator(Box::new(TypedIdGenerator::new()));
///
/// // 自定义整数起始值
/// set_id_generator(Box::new(TypedIdGenerator::with_int_start(10000)));
/// ```
#[derive(Debug)]
pub struct TypedIdGenerator {
    int_counter: std::sync::atomic::AtomicI64,
}

impl Default for TypedIdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl TypedIdGenerator {
    pub fn new() -> Self {
        Self {
            int_counter: std::sync::atomic::AtomicI64::new(1),
        }
    }

    pub fn with_int_start(start: i64) -> Self {
        Self {
            int_counter: std::sync::atomic::AtomicI64::new(start),
        }
    }
}

impl IdGenerator for TypedIdGenerator {
    fn generate(&self) -> Value {
        self.int_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            .into()
    }

    fn generate_for_type(&self, _entity_name: &str, _field_name: &str, field_type: &str) -> Option<Value> {
        match field_type {
            "String" | "Option<String>" => {
                let uuid = uuid::Uuid::new_v4();
                Some(uuid.to_string().into())
            }
            "i32" | "Option<i32>" => {
                let v = self.int_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Some((v as i32).into())
            }
            "i64" | "Option<i64>" => {
                let v = self.int_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Some(v.into())
            }
            "u32" | "Option<u32>" => {
                let v = self.int_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Some((v as u32).into())
            }
            "u64" | "Option<u64>" => {
                let v = self.int_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Some((v as u64).into())
            }
            _ => None,
        }
    }
}
