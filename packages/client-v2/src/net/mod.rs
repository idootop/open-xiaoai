//! # Net 模块
//!
//! 网络通信相关模块：
//! - `command` - RPC 命令类型系统
//! - `discovery` - 服务发现
//! - `event` - 实时事件系统
//! - `network` - 底层网络连接
//! - `protocol` - 通信协议定义
//! - `rpc` - RPC 调用管理
//! - `sync` - 时间同步工具
//! - `jitter_buffer` - 抖动缓冲区实现

pub mod command;
pub mod discovery;
pub mod event;
pub mod jitter_buffer;
pub mod network;
pub mod protocol;
pub mod rpc;
pub mod sync;
