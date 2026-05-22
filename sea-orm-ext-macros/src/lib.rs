//! # sea-orm-ext-macros
//!
//! 过程宏(proc-macro)实现，为 sea-orm-ext 提供派生宏支持。
//!
//! ## 提供的派生宏
//!
//! | 派生宏 | 功能 |
//! |---|---|
//! | `DeriveAutoFill` | 在 insert/update 时自动填充字段值(如 created_at, updated_at) |
//! | `DeriveSoftDelete` | 将 delete 操作转换为软删除(标记字段而非实际删除行) |
//! | `DeriveAutoFillSoftDelete` | 同时启用自动填充和软删除 |
//! | `DeriveTenant` | 多租户支持，在 insert 时自动注入租户 ID |
//! | `DeriveAutoFillSoftDeleteTenant` | 同时启用自动填充、软删除和多租户 |
//!
//! ## 工作原理
//!
//! 所有派生宏都通过 `expand_derive` 统一入口处理，根据 `DeriveKind`
//! 决定生成哪些 trait 实现和辅助方法：
//! - **ActiveModelBehavior 实现**：为 `ActiveModel` 实现
//!   `sea_orm::ActiveModelBehavior`，在 `before_save` 钩子中注入填充逻辑
//!   和租户逻辑，在 `before_delete` 钩子中实现软删除。
//! - **SoftDeleteTrait 实现**：为 `Entity` 实现自定义的 `SoftDeleteTrait`。
//! - **TenantEntity 实现**：为 `Entity` 实现自定义的 `TenantEntity`，
//!   返回租户列信息。
//! - **查询辅助方法**：在 `Entity` 上生成 `find_active()`(带软删除过滤)
//!   和 `find_with_tenant()`(带租户过滤) 两个便捷查询方法。
//! - **批量操作方法**：在 `Entity` 上生成 `insert_many_with_fill`、
//!   `update_many_with_fill`、`delete_many_soft` 等批量操作辅助方法。

use heck::ToUpperCamelCase;
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{Data, DataStruct, Expr, Fields, LitInt, Type};

/// 字段的自动填充模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FillMode {
    /// 仅在 Insert 时填充
    Insert,
    /// 仅在 Update 时填充
    Update,
    /// Insert 和 Update 时都填充
    InsertUpdate,
}

/// 解析后的填充字段信息
struct FillFieldInfo {
    /// 字段标识符(如 `created_at`)
    field_ident: Ident,
    /// 字段在 Rust 中的类型(去除了外层 `Option<...>` 的内部类型)
    field_type: Type,
    /// 原始字段是否为 Option 类型
    is_option: bool,
    /// 填充模式
    fill_mode: FillMode,
}

/// 主键字段信息(仅针对 auto_generate 的主键)
struct PrimaryKeyInfo {
    /// 字段标识符(如 `id`)
    field_ident: Ident,
    /// 字段类型(如 `i32`)
    field_type: Type,
    /// 原始字段是否为 Option 类型
    is_option: bool,
}

/// 软删除字段信息
struct SoftDeleteFieldInfo {
    /// 字段标识符(如 `is_deleted`)
    field_ident: Ident,
    /// 对应的列枚举变体名(由蛇形命名转换为大驼峰，如 `IsDeleted`)
    column_ident: Ident,
    /// 字段类型
    field_type: Type,
    /// 原始字段是否为 Option 类型
    is_option: bool,
    /// 默认值(未删除状态)，默认 0
    default_value: LitInt,
    /// 删除标记值，默认 1
    del_value: LitInt,
}

/// 租户字段信息
struct TenantFieldInfo {
    /// 字段标识符(如 `tenant_id`)
    field_ident: Ident,
    /// 对应的列枚举变体名
    column_ident: Ident,
    /// 字段类型
    field_type: Type,
    /// 原始字段是否为 Option 类型
    is_option: bool,
}

/// 派生宏生成类型，决定要生成哪些功能的实现代码
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeriveKind {
    /// 仅自动填充
    AutoFill,
    /// 仅软删除
    SoftDelete,
    /// 自动填充 + 软删除
    AutoFillSoftDelete,
    /// 仅多租户
    Tenant,
    /// 自动填充 + 多租户
    AutoFillTenant,
    /// 自动填充 + 软删除 + 多租户(全功能)
    AutoFillSoftDeleteTenant,
}

/// `DeriveAutoFill` 派生宏入口
///
/// 为标记了 `#[sea_orm_ext(insert)]` / `#[sea_orm_ext(update)]` /
/// `#[sea_orm_ext(insert_update)]` 的字段在对应时机调用全局填充处理器。
#[proc_macro_derive(DeriveAutoFill, attributes(sea_orm_ext, sea_orm))]
pub fn derive_auto_fill(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    expand_derive(DeriveKind::AutoFill, &input).into()
}

/// `DeriveSoftDelete` 派生宏入口
///
/// 为标记了 `#[soft_delete(default = 0, del = 1)]` 的字段生成软删除逻辑：
/// `before_delete` 中标记该字段为删除值并返回错误以阻止实际删除。
#[proc_macro_derive(DeriveSoftDelete, attributes(sea_orm_ext, sea_orm, soft_delete))]
pub fn derive_soft_delete(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    expand_derive(DeriveKind::SoftDelete, &input).into()
}

/// `DeriveAutoFillSoftDelete` 派生宏入口
///
/// 同时启用自动填充和软删除功能。
#[proc_macro_derive(DeriveAutoFillSoftDelete, attributes(sea_orm_ext, sea_orm, soft_delete))]
pub fn derive_auto_fill_soft_delete(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    expand_derive(DeriveKind::AutoFillSoftDelete, &input).into()
}

/// `DeriveTenant` 派生宏入口
///
/// 为标记了 `#[sea_orm_ext(TENANT)]` 的字段生成多租户注入逻辑：
/// 在 `before_save` 中自动将当前租户 ID 写入该字段。
#[proc_macro_derive(DeriveTenant, attributes(sea_orm_ext, sea_orm))]
pub fn derive_tenant(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    expand_derive(DeriveKind::Tenant, &input).into()
}

/// `DeriveAutoFillTenant` 派生宏入口
///
/// 同时启用自动填充和多租户功能（不含软删除）。
/// 适用于需要自动 ID 生成、字段填充和租户隔离，但不需要软删除的实体。
#[proc_macro_derive(DeriveAutoFillTenant, attributes(sea_orm_ext, sea_orm))]
pub fn derive_auto_fill_tenant(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    expand_derive(DeriveKind::AutoFillTenant, &input).into()
}

/// `DeriveAutoFillSoftDeleteTenant` 派生宏入口
///
/// 同时启用自动填充、软删除和多租户功能(全功能组合)。
#[proc_macro_derive(DeriveAutoFillSoftDeleteTenant, attributes(sea_orm_ext, sea_orm, soft_delete))]
pub fn derive_auto_fill_soft_delete_tenant(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    expand_derive(DeriveKind::AutoFillSoftDeleteTenant, &input).into()
}

/// 统一的派生宏展开入口
///
/// 检查结构体名称必须为 `Model`(sea-orm 约定)，根据 `DeriveKind`
/// 决定需要解析哪些字段，然后分别生成各部分的 trait 实现和方法。
fn expand_derive(kind: DeriveKind, input: &syn::DeriveInput) -> TokenStream {
    // 仅处理名为 "Model" 的结构体，符合 sea-orm 约定
    if input.ident != "Model" {
        return TokenStream::new();
    }

    // 根据 DeriveKind 判断需要哪些功能模块
    let need_fill = matches!(kind, DeriveKind::AutoFill | DeriveKind::AutoFillSoftDelete | DeriveKind::AutoFillTenant | DeriveKind::AutoFillSoftDeleteTenant);
    let need_sd = matches!(kind, DeriveKind::SoftDelete | DeriveKind::AutoFillSoftDelete | DeriveKind::AutoFillSoftDeleteTenant);
    let need_tenant = matches!(kind, DeriveKind::Tenant | DeriveKind::AutoFillTenant | DeriveKind::AutoFillSoftDeleteTenant);

    // 解析填充字段配置
    let fill_fields = if need_fill {
        match parse_fill_fields(&input.data) {
            Ok(f) => f,
            Err(e) => return e.to_compile_error(),
        }
    } else {
        Vec::new()
    };

    // 解析主键字段(auto_generate 的 primary key)
    let primary_key = match parse_primary_key(&input.data) {
        Ok(pk) => pk,
        Err(e) => return e.to_compile_error(),
    };

    // 解析软删除字段
    let soft_delete = if need_sd {
        match parse_soft_delete_field(&input.data) {
            Ok(Some(sd)) => Some(sd),
            Ok(None) => {
                return syn::Error::new_spanned(
                    &input.ident,
                    "DeriveSoftDelete requires a field annotated with #[soft_delete(...)]",
                )
                .to_compile_error();
            }
            Err(e) => return e.to_compile_error(),
        }
    } else {
        None
    };

    // 解析租户字段
    let tenant_field = if need_tenant {
        match parse_tenant_field(&input.data) {
            Ok(Some(t)) => Some(t),
            Ok(None) => {
                return syn::Error::new_spanned(
                    &input.ident,
                    "DeriveTenant requires a field annotated with #[sea_orm_ext(TENANT)]",
                )
                .to_compile_error();
            }
            Err(e) => return e.to_compile_error()
        }
    } else {
        None
    };

    // 生成各功能模块的 TokenStream
    let behavior_impl =
        expand_active_model_behavior_impl(&fill_fields, &primary_key, &soft_delete, &tenant_field);
    let soft_delete_impl = if let Some(sd) = &soft_delete {
        expand_soft_delete_trait_impl(sd)
    } else {
        TokenStream::new()
    };
    let find_impl = expand_find_methods(&soft_delete, &tenant_field);
    let batch_impls = expand_batch_entity_methods(&fill_fields, &primary_key, &soft_delete, &tenant_field);
    let tenant_impl = if let Some(t) = &tenant_field {
        expand_tenant_trait_impl(t)
    } else {
        TokenStream::new()
    };
    let tenant_find_impl = TokenStream::new();

    // 组合所有生成的代码块
    quote! {
        #behavior_impl
        #soft_delete_impl
        #find_impl
        #tenant_impl
        #tenant_find_impl
        #batch_impls
    }
}

/// 解析结构体中标注了填充模式的字段
///
/// 遍历结构体的 `#[sea_orm_ext(insert)]` / `#[sea_orm_ext(update)]` /
/// `#[sea_orm_ext(insert_update)]` 标注，收集需要自动填充的字段。
/// 跳过关联字段(如 `HasOne`, `HasMany`)和被忽略的字段(`#[sea_orm(ignore)]`)。
fn parse_fill_fields(data: &Data) -> syn::Result<Vec<FillFieldInfo>> {
    // 只处理具名字段的结构体
    let fields = match data {
        Data::Struct(DataStruct {
            fields: Fields::Named(named),
            ..
        }) => &named.named,
        _ => return Ok(Vec::new()),
    };

    let mut result = Vec::new();

    for field in fields {
        // 跳过匿名字段(tuple struct 不会出现，但安全考虑)
        let Some(ident) = &field.ident else {
            continue;
        };

        let field_type = &field.ty;
        // 将类型转为无空格的字符串以便模式匹配
        let field_type_str: String = quote! { #field_type }
            .to_string()
            .split_whitespace()
            .collect();

        // 跳过关联关系字段(无实际数据库列)
        if is_compound_field(&field_type_str) {
            continue;
        }

        let mut is_ignored = false;
        let mut fill_mode = None;

        // 解析 `sea_orm` 和 `sea_orm_ext` 属性中的标注
        for attr in field.attrs.iter() {
            if attr.path().is_ident("sea_orm") || attr.path().is_ident("sea_orm_ext") {
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("ignore") {
                        is_ignored = true;
                    } else if meta.path.is_ident("insert") {
                        fill_mode = Some(FillMode::Insert);
                    } else if meta.path.is_ident("update") {
                        fill_mode = Some(FillMode::Update);
                    } else if meta.path.is_ident("insert_update") {
                        fill_mode = Some(FillMode::InsertUpdate);
                    } else {
                        // 忽略未知属性值(如 `#[sea_orm(primary_key)]`)
                        let _: Option<Expr> = meta.value().and_then(|v| v.parse()).ok();
                    }
                    Ok(())
                })?;
            }
        }

        if is_ignored {
            continue;
        }

        // 提取内部类型和是否为 Option 标志
        if let Some(mode) = fill_mode {
            let (inner_type, is_option) = extract_inner_type_and_option(&field_type_str);
            let ty: Type = syn::LitStr::new(inner_type, field.span())
                .parse()
                .map_err(|_| syn::Error::new_spanned(ident, "Failed to parse field type"))?;

            result.push(FillFieldInfo {
                field_ident: ident.clone(),
                field_type: ty,
                is_option,
                fill_mode: mode,
            });
        }
    }

    Ok(result)
}

/// 解析主键字段
///
/// 查找同时标注了 `#[sea_orm(primary)]` 和 `#[sea_orm(auto_generate)]` 的字段，
/// 该字段将在 insert 时通过 `sea_orm_ext::get_id_generator()` 自动生成 ID。
fn parse_primary_key(data: &Data) -> syn::Result<Option<PrimaryKeyInfo>> {
    let fields = match data {
        Data::Struct(DataStruct {
            fields: Fields::Named(named),
            ..
        }) => &named.named,
        _ => return Ok(None),
    };

    for field in fields {
        let Some(ident) = &field.ident else {
            continue;
        };

        for attr in field.attrs.iter() {
            if !attr.path().is_ident("sea_orm") {
                continue;
            }

            let mut is_primary = false;
            let mut auto_generate = false;

            // 检查是否为 primary + auto_generate 的组合
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("primary_key") || meta.path.is_ident("primary") {
                    is_primary = true;
                } else if meta.path.is_ident("auto_generate") {
                    auto_generate = true;
                } else {
                    let _: Option<Expr> = meta.value().and_then(|v| v.parse()).ok();
                }
                Ok(())
            })?;

            if is_primary && auto_generate {
                let field_type = &field.ty;
                let field_type_str: String = quote! { #field_type }
                    .to_string()
                    .split_whitespace()
                    .collect();
                let (inner_type, is_option) = extract_inner_type_and_option(&field_type_str);
                let ty: Type = syn::LitStr::new(inner_type, field.span())
                    .parse()
                    .map_err(|_| {
                        syn::Error::new_spanned(ident, "Failed to parse field type")
                    })?;

                return Ok(Some(PrimaryKeyInfo {
                    field_ident: ident.clone(),
                    field_type: ty,
                    is_option,
                }));
            }
        }
    }

    Ok(None)
}

/// 解析软删除字段
///
/// 查找标注了 `#[soft_delete(default = 0, del = 1)]` 的字段。
/// - `default`：未删除时的字段值(默认 0)
/// - `del`：标记为已删除时的字段值(默认 1)
/// 返回的 `column_ident` 是字段名的大驼峰形式，用作 sea-orm `Column` 枚举变体名。
fn parse_soft_delete_field(data: &Data) -> syn::Result<Option<SoftDeleteFieldInfo>> {
    let fields = match data {
        Data::Struct(DataStruct {
            fields: Fields::Named(named),
            ..
        }) => &named.named,
        _ => return Ok(None),
    };

    for field in fields {
        let Some(ident) = &field.ident else {
            continue;
        };

        for attr in field.attrs.iter() {
            if attr.path().is_ident("soft_delete") {
                let mut default_val: Option<LitInt> = None;
                let mut del_val: Option<LitInt> = None;

                // 解析 `default` 和 `del` 参数
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("default") {
                        let lit: LitInt = meta.value()?.parse()?;
                        default_val = Some(lit);
                    } else if meta.path.is_ident("del") {
                        let lit: LitInt = meta.value()?.parse()?;
                        del_val = Some(lit);
                    }
                    Ok(())
                })?;

                let default_value =
                    default_val.unwrap_or_else(|| LitInt::new("0", ident.span()));
                let del_value = del_val.unwrap_or_else(|| LitInt::new("1", ident.span()));

                let field_type = &field.ty;
                let field_type_str: String = quote! { #field_type }
                    .to_string()
                    .split_whitespace()
                    .collect();
                let (inner_type, is_option) = extract_inner_type_and_option(&field_type_str);
                let ty: Type = syn::LitStr::new(inner_type, field.span())
                    .parse()
                    .map_err(|_| syn::Error::new_spanned(ident, "Failed to parse field type"))?;

                // 列枚举变体：蛇形字段名 -> 大驼峰(如 `is_deleted` -> `IsDeleted`)
                let column_name = ident.to_string().to_upper_camel_case();
                let column_ident = format_ident!("{}", column_name);

                return Ok(Some(SoftDeleteFieldInfo {
                    field_ident: ident.clone(),
                    column_ident,
                    field_type: ty,
                    is_option,
                    default_value,
                    del_value,
                }));
            }
        }
    }

    Ok(None)
}

/// 解析租户字段
///
/// 查找标注了 `#[sea_orm_ext(TENANT)]` 的字段，该字段将在 insert 时
/// 自动注入当前租户 ID。
fn parse_tenant_field(data: &Data) -> syn::Result<Option<TenantFieldInfo>> {
    let fields = match data {
        Data::Struct(DataStruct {
            fields: Fields::Named(named),
            ..
        }) => &named.named,
        _ => return Ok(None),
    };

    for field in fields {
        let Some(ident) = &field.ident else {
            continue;
        };

        for attr in field.attrs.iter() {
            if attr.path().is_ident("sea_orm_ext") {
                let mut is_tenant = false;
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("TENANT") {
                        is_tenant = true;
                    }
                    Ok(())
                })?;

                if is_tenant {
                    let field_type = &field.ty;
                    let field_type_str: String = quote! { #field_type }
                        .to_string()
                        .split_whitespace()
                        .collect();
                    let (inner_type, is_option) = extract_inner_type_and_option(&field_type_str);
                    let ty: Type = syn::LitStr::new(inner_type, field.span())
                        .parse()
                        .map_err(|_| syn::Error::new_spanned(ident, "Failed to parse field type"))?;

                    // 生成列枚举变体：蛇形 -> 大驼峰
                    let column_name = ident.to_string().to_upper_camel_case();
                    let column_ident = format_ident!("{}", column_name);

                    return Ok(Some(TenantFieldInfo {
                        field_ident: ident.clone(),
                        column_ident,
                        field_type: ty,
                        is_option,
                    }));
                }
            }
        }
    }

    Ok(None)
}

/// 判断字段是否为关联关系字段(无实际数据库列)
///
/// 排除以下类型的字段：
/// - `HasOne<...>`, `HasMany<...>` — sea-orm 的一对一/一对多关联
/// - `HasOneModel<...>`, `HasManyModel<...>` — 关联模型类型
/// - `Option<...::Entity>` / `Option<...::Relation>` — 可选实体/关联引用
/// - `Vec<...::Entity>` / `Vec<...::Relation>` — 多实体/关联引用
fn is_compound_field(field_type_str: &str) -> bool {
    field_type_str.starts_with("HasOne<")
        || field_type_str.starts_with("HasMany<")
        || field_type_str.starts_with("HasOneModel<")
        || field_type_str.starts_with("HasManyModel<")
        || (field_type_str.starts_with("Option<")
            && (field_type_str.ends_with("::Entity>")
                || field_type_str.ends_with("::Relation>")))
        || (field_type_str.starts_with("Vec<")
            && (field_type_str.ends_with("::Entity>")
                || field_type_str.ends_with("::Relation>")))
}

/// 提取 `Option<T>` 的内部类型 `T`，并返回是否为 Option 类型
///
/// sea-orm 的 `ActiveValue::Set()` 需要正确处理 Option 类型，
/// 对于 `Option<T>` 类型的字段，返回 `(T, true)`，
/// 对于非 Option 类型，返回 `(T, false)`。
fn extract_inner_type_and_option(field_type_str: &str) -> (&str, bool) {
    if field_type_str.starts_with("Option<") {
        (
            &field_type_str["Option<".len()..field_type_str.len() - 1],
            true
        )
    } else {
        (field_type_str, false)
    }
}

/// 生成 `ActiveModelBehavior` trait 实现
///
/// 这是核心生成逻辑，为 `ActiveModel` 实现 `sea_orm::ActiveModelBehavior`：
/// - `before_save`：处理主键自动生成、字段填充、租户 ID 注入
/// - `before_delete`：处理软删除逻辑
fn expand_active_model_behavior_impl(
    fill_fields: &[FillFieldInfo],
    primary_key: &Option<PrimaryKeyInfo>,
    soft_delete: &Option<SoftDeleteFieldInfo>,
    tenant_field: &Option<TenantFieldInfo>,
) -> TokenStream {
    let before_save_body = expand_before_save_body(fill_fields, primary_key, tenant_field);
    let before_delete_body = expand_before_delete_body(soft_delete);

    // 如果没有需要处理的钩子，生成一个空的默认实现
    if before_save_body.is_empty() && before_delete_body.is_empty() {
        return quote! {
            #[automatically_derived]
            impl sea_orm::ActiveModelBehavior for ActiveModel {}
        };
    }

    // 仅在填充逻辑非空时生成 before_save 钩子
    let before_save_fn = if before_save_body.is_empty() {
        quote!()
    } else {
        quote! {
            async fn before_save<C>(self, db: &C, insert: bool) -> Result<Self, sea_orm::DbErr>
            where
                C: sea_orm::ConnectionTrait,
            {
                let mut am = self;
                #before_save_body
                Ok(am)
            }
        }
    };

    // 仅在软删除字段存在时生成 before_delete 钩子
    let before_delete_fn = if before_delete_body.is_empty() {
        quote!()
    } else {
        quote! {
            async fn before_delete<C>(self, db: &C) -> Result<Self, sea_orm::DbErr>
            where
                C: sea_orm::ConnectionTrait,
            {
                let mut am = self;
                #before_delete_body
            }
        }
    };

    quote! {
        #[automatically_derived]
        #[::async_trait::async_trait]
        impl sea_orm::ActiveModelBehavior for ActiveModel {
            #before_save_fn
            #before_delete_fn
        }
    }
}

/// 生成 `before_save` 钩子的代码体
///
/// 按顺序生成三种逻辑：
/// 1. **主键自动生成**：当 `insert=true` 且主键未设置时，调用全局 ID 生成器
/// 2. **字段填充**：根据填充模式(Inser/Update/InsertUpdate)调用全局填充处理器
/// 3. **租户注入**：在 `TenantMode::Table` 模式下，注入当前租户 ID
fn expand_before_save_body(
    fill_fields: &[FillFieldInfo],
    primary_key: &Option<PrimaryKeyInfo>,
    tenant_field: &Option<TenantFieldInfo>,
) -> TokenStream {
    let mut statements: Vec<TokenStream> = Vec::new();

    // 主键自动生成：仅在 insert 且主键未设置时触发
    if let Some(pk) = primary_key {
        let field_ident = &pk.field_ident;
        let field_type = &pk.field_type;
        let is_option = pk.is_option;
        let field_type_str = quote! { #field_type }.to_string().split_whitespace().collect::<String>();
        let set_code = if is_option {
            quote! { sea_orm::Set(Some(v)) }
        } else {
            quote! { sea_orm::Set(v) }
        };
        statements.push(quote! {
            if insert && am.#field_ident.is_not_set() {
                if let Some(gen) = sea_orm_ext::get_id_generator() {
                    let entity_name = <Entity as sea_orm::EntityName>::table_name(&Entity::default());
                    let val = if let Some(typed_val) = gen.generate_for_type(entity_name, stringify!(#field_ident), #field_type_str) {
                        typed_val
                    } else {
                        gen.generate()
                    };
                    let v = <#field_type as sea_orm::sea_query::ValueType>::try_from(val)
                        .map_err(|e| sea_orm::DbErr::Type(e.to_string()))?;
                    am.#field_ident = #set_code;
                }
            }
        });
    }

    // 字段填充：根据 fill_mode 分别收集 insert 和 update 的填充逻辑
    if !fill_fields.is_empty() {
        let mut insert_branches = Vec::new();
        let mut update_branches = Vec::new();

        for field in fill_fields {
            let field_ident = &field.field_ident;
            let field_type = &field.field_type;
            let field_name_str = field_ident.to_string();
            let is_option = field.is_option;
            let set_code = if is_option {
                quote! { sea_orm::Set(Some(v)) }
            } else {
                quote! { sea_orm::Set(v) }
            };

            // 调用全局 FieldFillHandler 获取填充值
            let fill_code = quote! {
                if let Some(handler) = sea_orm_ext::get_field_fill_handler() {
                    let op = if insert {
                        sea_orm_ext::FieldFillOperation::Insert
                    } else {
                        sea_orm_ext::FieldFillOperation::Update
                    };
                    if let Some(val) = handler.fill(
                        <Entity as sea_orm::EntityName>::table_name(&Entity::default()),
                        #field_name_str,
                        op,
                    ) {
                        let v = <#field_type as sea_orm::sea_query::ValueType>::try_from(val)
                            .map_err(|e| sea_orm::DbErr::Type(e.to_string()))?;
                        am.#field_ident = #set_code;
                    }
                }
            };

            match field.fill_mode {
            FillMode::Insert => insert_branches.push(fill_code),
            FillMode::Update => update_branches.push(fill_code),
            FillMode::InsertUpdate => {
                // InsertUpdate 模式在两种场景都需要填充
                insert_branches.push(fill_code.clone());
                update_branches.push(fill_code);
            }
        }
        }

        // 将收集到的填充代码放入 insert/!insert 分支中
        if !insert_branches.is_empty() {
            statements.push(quote! {
                if insert {
                    #(#insert_branches)*
                }
            });
        }

        if !update_branches.is_empty() {
            statements.push(quote! {
                if !insert {
                    #(#update_branches)*
                }
            });
        }
    }

    // 租户 ID 注入：仅在 Table 模式且租户字段未设置时触发，缺失时返回错误
    if let Some(t) = tenant_field {
        let field_ident = &t.field_ident;
        let field_type = &t.field_type;
        let is_option = t.is_option;
        let set_code = if is_option {
            quote! { sea_orm::Set(Some(v)) }
        } else {
            quote! { sea_orm::Set(v) }
        };
        statements.push(quote! {
            if sea_orm_ext::is_tenant_enforced() && am.#field_ident.is_not_set() {
                let tenant_id = sea_orm_ext::try_get_tenant_id()?;
                let v = <#field_type as sea_orm::sea_query::ValueType>::try_from(tenant_id)
                    .map_err(|e| sea_orm::DbErr::Type(e.to_string()))?;
                am.#field_ident = #set_code;
            }
        });
    }

    quote! {
        #(#statements)*
    }
}

/// 生成 `before_delete` 钩子的代码体(软删除逻辑)
///
/// 将软删除字段设为 `del_value`，然后执行 update 保存修改，
/// 最后返回 `DbErr::Custom("...")` 以阻止实际删除操作。
fn expand_before_delete_body(soft_delete: &Option<SoftDeleteFieldInfo>) -> TokenStream {
    let Some(sd) = soft_delete else {
        return TokenStream::new();
    };

    let field_ident = &sd.field_ident;
    let del_value = &sd.del_value;
    let field_type = &sd.field_type;
    let is_option = sd.is_option;
    let set_code = if is_option {
        quote! { sea_orm::Set(Some(#del_value as #field_type)) }
    } else {
        quote! { sea_orm::Set(#del_value as #field_type) }
    };

    quote! {
        am.#field_ident = #set_code;
        let _model = am.update(db).await?;
        Err(sea_orm::DbErr::Custom("Record was soft-deleted".to_owned()))
    }
}

/// 生成 `SoftDeleteTrait` trait 实现
///
/// 为 `Entity` 提供软删除的默认值和删除值，供外部工具方法使用。
fn expand_soft_delete_trait_impl(sd: &SoftDeleteFieldInfo) -> TokenStream {
    let default_value = &sd.default_value;
    let del_value = &sd.del_value;
    let field_type = &sd.field_type;

    quote! {
        #[automatically_derived]
        impl sea_orm_ext::SoftDeleteTrait for Entity {
            fn soft_delete_default() -> sea_query::Value {
                (#default_value as #field_type).into()
            }

            fn soft_delete_del() -> sea_query::Value {
                (#del_value as #field_type).into()
            }
        }
    }
}

/// 生成查询方法（统一处理软删除 + 租户过滤）。
///
/// 根据实体配置的功能组合，覆盖 `Entity::find()` 和 `Entity::find_by_id()`：
/// - 仅软删除：自动过滤已软删除记录
/// - 仅租户：自动过滤当前租户记录
/// - 软删除 + 租户：同时添加两个过滤条件
///
/// 同时提供对应的"无过滤"版本方法用于特殊场景。
fn expand_find_methods(
    soft_delete: &Option<SoftDeleteFieldInfo>,
    tenant_field: &Option<TenantFieldInfo>,
) -> TokenStream {
    let has_sd = soft_delete.is_some();
    let has_tenant = tenant_field.is_some();

    if !has_sd && !has_tenant {
        return TokenStream::new();
    }

    let mut find_filters: Vec<TokenStream> = Vec::new();
    let mut find_by_id_filters: Vec<TokenStream> = Vec::new();

    let mut sd_filter_code: Vec<TokenStream> = Vec::new();
    let mut tenant_filter_code: Vec<TokenStream> = Vec::new();

    let mut method_decls: Vec<TokenStream> = Vec::new();

    if let Some(sd) = soft_delete {
        let column_ident = &sd.column_ident;
        let default_value = &sd.default_value;
        let field_type = &sd.field_type;

        let sd_filter = quote! {
            select = select.filter(Column::#column_ident.eq(#default_value as #field_type));
        };
        find_filters.push(sd_filter.clone());
        find_by_id_filters.push(sd_filter.clone());
        sd_filter_code.push(sd_filter);

        method_decls.push(quote! {
            #[deprecated(note = "use `Entity::find()` instead, which now auto-filters soft-deleted records")]
            pub fn find_active() -> sea_orm::Select<Entity> {
                Self::find()
            }
        });
    }

    if let Some(t) = tenant_field {
        let column_ident = &t.column_ident;

        let tenant_filter = quote! {
            if sea_orm_ext::is_tenant_enforced() && !sea_orm_ext::is_table_tenant_ignored(<Entity as sea_orm::EntityName>::table_name(&Entity::default()).as_ref()) {
                let tenant_id = sea_orm_ext::require_tenant_id();
                select = select.filter(Column::#column_ident.eq(tenant_id));
            }
        };
        find_filters.push(tenant_filter.clone());
        find_by_id_filters.push(tenant_filter.clone());
        tenant_filter_code.push(tenant_filter);

        method_decls.push(quote! {
            #[deprecated(note = "use `Entity::find()` instead, which now auto-filters tenant records")]
            pub fn find_with_tenant() -> sea_orm::Select<Entity> {
                Self::find()
            }
        });
    }

    if has_sd && has_tenant {
        method_decls.push(quote! {
            pub fn find_with_deleted() -> sea_orm::Select<Entity> {
                let mut select = <Entity as sea_orm::EntityTrait>::find();
                #(#tenant_filter_code)*
                select
            }

            pub fn find_by_id_with_deleted(
                values: <<Entity as sea_orm::EntityTrait>::PrimaryKey as sea_orm::PrimaryKeyTrait>::ValueType,
            ) -> sea_orm::Select<Entity> {
                let mut select = <Entity as sea_orm::EntityTrait>::find_by_id(values);
                #(#tenant_filter_code)*
                select
            }

            pub fn find_without_tenant() -> sea_orm::Select<Entity> {
                let mut select = <Entity as sea_orm::EntityTrait>::find();
                #(#sd_filter_code)*
                select
            }

            pub fn find_by_id_without_tenant(
                values: <<Entity as sea_orm::EntityTrait>::PrimaryKey as sea_orm::PrimaryKeyTrait>::ValueType,
            ) -> sea_orm::Select<Entity> {
                let mut select = <Entity as sea_orm::EntityTrait>::find_by_id(values);
                #(#sd_filter_code)*
                select
            }
        });
    } else if has_sd {
        method_decls.push(quote! {
            pub fn find_with_deleted() -> sea_orm::Select<Entity> {
                <Entity as sea_orm::EntityTrait>::find()
            }

            pub fn find_by_id_with_deleted(
                values: <<Entity as sea_orm::EntityTrait>::PrimaryKey as sea_orm::PrimaryKeyTrait>::ValueType,
            ) -> sea_orm::Select<Entity> {
                <Entity as sea_orm::EntityTrait>::find_by_id(values)
            }
        });
    } else if has_tenant {
        method_decls.push(quote! {
            pub fn find_without_tenant() -> sea_orm::Select<Entity> {
                <Entity as sea_orm::EntityTrait>::find()
            }

            pub fn find_by_id_without_tenant(
                values: <<Entity as sea_orm::EntityTrait>::PrimaryKey as sea_orm::PrimaryKeyTrait>::ValueType,
            ) -> sea_orm::Select<Entity> {
                <Entity as sea_orm::EntityTrait>::find_by_id(values)
            }
        });
    }

    let find_filter_code = find_filters;
    let find_by_id_filter_code = find_by_id_filters;

    quote! {
        #[automatically_derived]
        impl Entity {
            pub fn find() -> sea_orm::Select<Entity> {
                let mut select = <Entity as sea_orm::EntityTrait>::find();
                #(#find_filter_code)*
                select
            }

            pub fn find_by_id(
                values: <<Entity as sea_orm::EntityTrait>::PrimaryKey as sea_orm::PrimaryKeyTrait>::ValueType,
            ) -> sea_orm::Select<Entity> {
                let mut select = <Entity as sea_orm::EntityTrait>::find_by_id(values);
                #(#find_by_id_filter_code)*
                select
            }

            #(#method_decls)*
        }
    }
}

/// 生成 `TenantEntity` trait 实现
///
/// 返回租户列引用，供多租户拦截器使用。
fn expand_tenant_trait_impl(t: &TenantFieldInfo) -> TokenStream {
    let column_ident = &t.column_ident;

    quote! {
        #[automatically_derived]
        impl sea_orm_ext::TenantEntity for Entity {
            type TenantColumn = Column;
            fn tenant_column() -> Self::TenantColumn {
                Column::#column_ident
            }
        }
    }
}

/// 生成所有批量操作方法的入口
///
/// 组合生成批量 insert、批量 update、批量软删除三个方法。
fn expand_batch_entity_methods(
    fill_fields: &[FillFieldInfo],
    primary_key: &Option<PrimaryKeyInfo>,
    soft_delete: &Option<SoftDeleteFieldInfo>,
    tenant_field: &Option<TenantFieldInfo>,
) -> TokenStream {
    let batch_insert = expand_batch_insert_method(fill_fields, primary_key, tenant_field);
    let batch_update = expand_batch_update_method(fill_fields, tenant_field);
    let batch_delete = expand_batch_delete_method(soft_delete, tenant_field, primary_key);

    quote! {
        #batch_insert
        #batch_update
        #batch_delete
    }
}

/// 生成批量 insert 方法
///
/// 生成两个方法：
/// - `insert_many_with_fill`：批量插入并返回 `Vec<Model>`
/// - `insert_many_with_fill_returning`：批量插入并返回 `UpdateResult`
/// 每个模型在插入前都会经过主键生成、字段填充和租户注入的处理。
fn expand_batch_insert_method(
    fill_fields: &[FillFieldInfo],
    primary_key: &Option<PrimaryKeyInfo>,
    tenant_field: &Option<TenantFieldInfo>,
) -> TokenStream {
    // 主键自动生成代码块
    let id_gen_block = if let Some(pk) = primary_key {
        let field_ident = &pk.field_ident;
        let field_type = &pk.field_type;
        let is_option = pk.is_option;
        let field_type_str = quote! { #field_type }.to_string().split_whitespace().collect::<String>();
        let set_code = if is_option {
            quote! { sea_orm::Set(Some(v)) }
        } else {
            quote! { sea_orm::Set(v) }
        };
        quote! {
            if am.#field_ident.is_not_set() {
                if let Some(gen) = sea_orm_ext::get_id_generator() {
                    let entity_name = <Entity as sea_orm::EntityName>::table_name(&Entity::default());
                    let val = if let Some(typed_val) = gen.generate_for_type(entity_name, stringify!(#field_ident), #field_type_str) {
                        typed_val
                    } else {
                        gen.generate()
                    };
                    if let Ok(v) = <#field_type as sea_orm::sea_query::ValueType>::try_from(val) {
                        am.#field_ident = #set_code;
                    }
                }
            }
        }
    } else {
        TokenStream::new()
    };

    // 过滤出需要在 Insert 时机填充的字段
    let fill_insert_fields: Vec<_> = fill_fields
        .iter()
        .filter(|f| matches!(f.fill_mode, FillMode::Insert | FillMode::InsertUpdate))
        .collect();

    // 插入时字段填充逻辑(使用 `if let Ok` 忽略转换错误，避免中断批量操作)
    let fill_block = if !fill_insert_fields.is_empty() {
        let fill_stmts: Vec<_> = fill_insert_fields
            .iter()
            .map(|f| {
                let field_ident = &f.field_ident;
                let field_type = &f.field_type;
                let field_name = field_ident.to_string();
                let is_option = f.is_option;
                let set_code = if is_option {
                    quote! { sea_orm::Set(Some(v)) }
                } else {
                    quote! { sea_orm::Set(v) }
                };
                quote! {
                    if let Some(handler) = sea_orm_ext::get_field_fill_handler() {
                        if let Some(val) = handler.fill(
                            <Entity as sea_orm::EntityName>::table_name(&Entity::default()),
                            #field_name,
                            sea_orm_ext::FieldFillOperation::Insert,
                        ) {
                            if let Ok(v) = <#field_type as sea_orm::sea_query::ValueType>::try_from(val) {
                                am.#field_ident = #set_code;
                            }
                        }
                    }
                }
            })
            .collect();
        quote! { #(#fill_stmts)* }
    } else {
        TokenStream::new()
    };

    // 租户 ID 注入逻辑
    let tenant_block = if let Some(t) = tenant_field {
        let field_ident = &t.field_ident;
        let field_type = &t.field_type;
        let is_option = t.is_option;
        let set_code = if is_option {
            quote! { sea_orm::Set(Some(v)) }
        } else {
            quote! { sea_orm::Set(v) }
        };
        quote! {
            if sea_orm_ext::is_tenant_enforced() && am.#field_ident.is_not_set() {
                let tenant_id = sea_orm_ext::try_get_tenant_id()?;
                let v = <#field_type as sea_orm::sea_query::ValueType>::try_from(tenant_id)
                    .map_err(|e| sea_orm::DbErr::Type(e.to_string()))?;
                am.#field_ident = #set_code;
            }
        }
    } else {
        TokenStream::new()
    };

    quote! {
        #[automatically_derived]
        impl Entity {
            pub async fn insert_many_with_fill<C>(
                models: Vec<ActiveModel>,
                db: &C,
            ) -> Result<Vec<Model>, sea_orm::DbErr>
            where
                C: sea_orm::ConnectionTrait,
            {
                if models.is_empty() {
                    return Ok(Vec::new());
                }
                let mut processed = Vec::with_capacity(models.len());
                for mut am in models {
                    #id_gen_block
                    #fill_block
                    #tenant_block
                    processed.push(am);
                }
                Entity::insert_many(processed).exec_with_returning(db).await
            }

            pub async fn insert_many_with_fill_returning<C>(
                models: Vec<ActiveModel>,
                db: &C,
            ) -> Result<sea_orm::UpdateResult, sea_orm::DbErr>
            where
                C: sea_orm::ConnectionTrait,
            {
                if models.is_empty() {
                    return Ok(sea_orm::UpdateResult { rows_affected: 0 });
                }
                let mut processed = Vec::with_capacity(models.len());
                for mut am in models {
                    #id_gen_block
                    #fill_block
                    #tenant_block
                    processed.push(am);
                }
                let count = processed.len() as u64;
                Entity::insert_many(processed).exec_without_returning(db).await?;
                Ok(sea_orm::UpdateResult { rows_affected: count })
            }
        }
    }
}

/// 生成批量 update 方法
///
/// 生成两个方法：
/// - `update_many_with_fill`：批量更新并返回 `UpdateResult`
/// - `update_many_with_fill_returning`：批量更新并返回 `Vec<Model>`
/// 仅处理 Update 和 InsertUpdate 模式的字段填充。
fn expand_batch_update_method(fill_fields: &[FillFieldInfo], tenant_field: &Option<TenantFieldInfo>) -> TokenStream {
    // 过滤出需要在 Update 时机填充的字段
    let fill_update_fields: Vec<_> = fill_fields
        .iter()
        .filter(|f| matches!(f.fill_mode, FillMode::Update | FillMode::InsertUpdate))
        .collect();

    // 更新时字段填充逻辑
    let fill_block = if !fill_update_fields.is_empty() {
        let fill_stmts: Vec<_> = fill_update_fields
            .iter()
            .map(|f| {
                let field_ident = &f.field_ident;
                let field_type = &f.field_type;
                let field_name = field_ident.to_string();
                let is_option = f.is_option;
                let set_code = if is_option {
                    quote! { sea_orm::Set(Some(v)) }
                } else {
                    quote! { sea_orm::Set(v) }
                };
                quote! {
                    if let Some(handler) = sea_orm_ext::get_field_fill_handler() {
                        if let Some(val) = handler.fill(
                            <Entity as sea_orm::EntityName>::table_name(&Entity::default()),
                            #field_name,
                            sea_orm_ext::FieldFillOperation::Update,
                        ) {
                            if let Ok(v) = <#field_type as sea_orm::sea_query::ValueType>::try_from(val) {
                                am.#field_ident = #set_code;
                            }
                        }
                    }
                }
            })
            .collect();
        quote! { #(#fill_stmts)* }
    } else {
        TokenStream::new()
    };

    // 租户 ID 注入逻辑(更新时同样检查)
    let tenant_block = if let Some(t) = tenant_field {
        let field_ident = &t.field_ident;
        let field_type = &t.field_type;
        let is_option = t.is_option;
        let set_code = if is_option {
            quote! { sea_orm::Set(Some(v)) }
        } else {
            quote! { sea_orm::Set(v) }
        };
        quote! {
            if sea_orm_ext::is_tenant_enforced() && am.#field_ident.is_not_set() {
                let tenant_id = sea_orm_ext::try_get_tenant_id()?;
                let v = <#field_type as sea_orm::sea_query::ValueType>::try_from(tenant_id)
                    .map_err(|e| sea_orm::DbErr::Type(e.to_string()))?;
                am.#field_ident = #set_code;
            }
        }
    } else {
        TokenStream::new()
    };

    quote! {
        #[automatically_derived]
        impl Entity {
            pub async fn update_many_with_fill<C>(
                models: Vec<ActiveModel>,
                db: &C,
            ) -> Result<sea_orm::UpdateResult, sea_orm::DbErr>
            where
                C: sea_orm::ConnectionTrait,
            {
                let mut total_rows = 0u64;
                for mut am in models {
                    #tenant_block
                    #fill_block
                    sea_orm::ActiveModelTrait::update(am, db).await?;
                    total_rows += 1;
                }
                Ok(sea_orm::UpdateResult { rows_affected: total_rows })
            }

            pub async fn update_many_with_fill_returning<C>(
                models: Vec<ActiveModel>,
                db: &C,
            ) -> Result<Vec<Model>, sea_orm::DbErr>
            where
                C: sea_orm::ConnectionTrait,
            {
                let mut results = Vec::with_capacity(models.len());
                for mut am in models {
                    #tenant_block
                    #fill_block
                    let model = sea_orm::ActiveModelTrait::update(am, db).await?;
                    results.push(model);
                }
                Ok(results)
            }
        }
    }
}

/// 生成批量软删除方法
///
/// 使用 `Entity::update_many()` + 主键 `is_in` 过滤，单条 SQL UPDATE 完成批量软删除。
fn expand_batch_delete_method(soft_delete: &Option<SoftDeleteFieldInfo>, tenant_field: &Option<TenantFieldInfo>, primary_key: &Option<PrimaryKeyInfo>) -> TokenStream {
    let Some(sd) = soft_delete else {
        return TokenStream::new();
    };

    let sd_column_ident = &sd.column_ident;
    let del_value = &sd.del_value;
    let sd_field_type = &sd.field_type;

    let tenant_filter_code = if let Some(t) = tenant_field {
        let t_column_ident = &t.column_ident;
        quote! {
            if sea_orm_ext::is_tenant_enforced() && !sea_orm_ext::is_table_tenant_ignored(<Entity as sea_orm::EntityName>::table_name(&Entity::default()).as_ref()) {
                let tenant_id = sea_orm_ext::require_tenant_id();
                query = query.filter(Column::#t_column_ident.eq(tenant_id));
            }
        }
    } else {
        TokenStream::new()
    };

    let pk_extract_and_filter = if let Some(pk) = primary_key {
        let pk_field = &pk.field_ident;
        let pk_type = &pk.field_type;
        let pk_column_name = pk.field_ident.to_string().to_upper_camel_case();
        let pk_column_ident = format_ident!("{}", pk_column_name);
        quote! {
            let pk_values: Vec<#pk_type> = models.iter().filter_map(|am| {
                match &am.#pk_field {
                    sea_orm::ActiveValue::Set(v) | sea_orm::ActiveValue::Unchanged(v) => Some(v.clone()),
                    _ => None,
                }
            }).collect();
            if pk_values.is_empty() {
                return Ok(sea_orm::UpdateResult { rows_affected: 0 });
            }
            query = query.filter(Column::#pk_column_ident.is_in(pk_values));
        }
    } else {
        quote! {
            let _ = models;
            return Ok(sea_orm::UpdateResult { rows_affected: 0 });
        }
    };

    quote! {
        #[automatically_derived]
        impl Entity {
            pub async fn delete_many_soft<C>(
                models: Vec<ActiveModel>,
                db: &C,
            ) -> Result<sea_orm::UpdateResult, sea_orm::DbErr>
            where
                C: sea_orm::ConnectionTrait,
            {
                if models.is_empty() {
                    return Ok(sea_orm::UpdateResult { rows_affected: 0 });
                }
                let mut query = Entity::update_many()
                    .col_expr(Column::#sd_column_ident, sea_query::Expr::value(#del_value as #sd_field_type));
                #tenant_filter_code
                #pk_extract_and_filter
                query.exec(db).await
            }
        }
    }
}