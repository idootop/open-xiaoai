//! 小爱开放平台客户端库
//!
//! 这个库提供了与小爱开放平台交互的客户端实现，
//! 包括服务发现、连接管理、音频处理等功能。
//! 它被设计为模块化的结构，便于扩展和维护。
//!
//! # 主要模块
//!
//! * `base` - 基础类型和常量定义
//! * `services` - 核心服务实现，包括服务发现、连接管理、音频处理等
//! * `utils` - 通用工具函数和辅助功能
//!
//! # 使用示例
//!
//! ```rust
//! use client_rust::services::discovery;
//! use client_rust::services::connect;
//!
//! async fn example() {
//!     // 创建服务发现实例
//!     let discovery_service = discovery::default_discovery().await;
//!     
//!     // 发现服务器
//!     let server_addr = discovery_service.discover().await.expect("Failed to discover server");
//!     
//!     // 连接到服务器
//!     // ...
//! }
//! ```

/// 基础类型和常量定义模块
pub mod base;

/// 核心服务实现模块
/// 
/// 包含以下子模块：
/// * `discovery` - 服务发现功能
/// * `connect` - 连接管理
/// * `audio` - 音频处理
/// * `monitor` - 系统监控
pub mod services;

/// 通用工具函数和辅助功能模块
/// 
/// 包含以下子模块：
/// * `event` - 事件处理
/// * `rand` - 随机数生成
/// * `shell` - Shell命令执行
/// * `task` - 异步任务管理
pub mod utils;
