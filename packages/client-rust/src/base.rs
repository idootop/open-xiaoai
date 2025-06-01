//! 基础类型和常量定义
//!
//! 该模块定义了整个应用程序中使用的基础类型和常量，
//! 包括错误处理类型和版本信息。

/// 应用程序错误类型
/// 
/// 使用动态错误特性对象（trait object）作为统一的错误类型，
/// 这允许在整个应用程序中使用一致的错误处理方式，同时支持
/// 不同类型的错误。通过Box智能指针进行堆分配，使错误可以
/// 在不同的上下文中传递。
/// 
/// # 示例
/// 
/// ```rust
/// use crate::base::AppError;
/// 
/// fn example_function() -> Result<(), AppError> {
///     // 可以返回任何实现了std::error::Error特性的错误
///     if something_went_wrong {
///         return Err("Something went wrong".into());
///     }
///     Ok(())
/// }
/// ```
pub type AppError = Box<dyn std::error::Error>;

/// 应用程序版本号
/// 
/// 从Cargo.toml文件中的package.version字段获取，
/// 在编译时由env!宏注入。这确保了版本号与包定义保持同步。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
