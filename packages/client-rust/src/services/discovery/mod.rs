// 导入子模块
pub mod udp;    // UDP服务发现实现模块

// 导入所需的外部依赖
use crate::base::AppError;  // 应用错误类型
use std::net::SocketAddr;   // 网络套接字地址类型

/// 服务发现接口
/// 
/// 该特性定义了服务发现的核心功能，任何实现服务发现的结构体
/// 都需要实现这个特性。这允许在不同的服务发现实现之间进行切换，
/// 同时保持API的一致性。
pub trait DiscoveryService {
    /// 发现可用服务端
    /// 
    /// 该方法尝试在网络中发现可用的服务端，并返回其网络地址。
    /// 返回一个异步Future，最终解析为包含服务端地址的Result，
    /// 或者在发现失败时返回错误。
    /// 
    /// # 返回值
    /// 
    /// * `Result<SocketAddr, AppError>` - 成功时返回服务端的套接字地址，失败时返回错误
    fn discover(&self) -> impl std::future::Future<Output = Result<SocketAddr, AppError>> + Send;
}

/// 创建默认发现服务实例
/// 
/// 这是一个工厂函数，用于创建默认的服务发现实现。
/// 当前默认使用UDP广播方式进行服务发现。
/// 
/// # 返回值
/// 
/// * 返回实现了`DiscoveryService`特性的实例
/// 
/// # Panics
/// 
/// 如果无法创建服务发现实例，此函数将会panic
pub async fn default_discovery(secret: &str) -> impl DiscoveryService {
    udp::UdpDiscoveryService::new(secret).await.expect("Failed to create discovery service")
}