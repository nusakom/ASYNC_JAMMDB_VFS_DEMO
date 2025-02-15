use async_trait::async_trait;
use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub struct VfsError(pub String);

impl fmt::Display for VfsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VfsError: {}", self.0)
    }
}

impl Error for VfsError {}

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

/// VFS 打开选项（示例中不做具体处理）
#[derive(Clone)]
pub struct OpenOptions;

impl OpenOptions {
    pub fn new() -> Self {
        OpenOptions
    }
}

/// 定义 Vfs trait，要求实现 open 方法
pub trait Vfs: Send + Sync {
    type File: VfsFile;
    fn open(&self, name: &str, options: OpenOptions) -> Result<Self::File>;
}

/// 定义 VfsFile trait，要求异步读写方法
#[async_trait]
pub trait VfsFile: Send + Sync {
    async fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize>;
    async fn write_at(&self, offset: u64, buf: &[u8]) -> Result<usize>;
}

/// 注册 VFS 实现；实际环境中应将该 VFS 与 SQLite 关联；此处仅打印注册信息
pub fn register_vfs<V: Vfs + 'static>(name: &str, _vfs: V, make_default: bool) -> Result<()> {
    println!("Registered VFS: {} (default: {})", name, make_default);
    Ok(())
}
