// ==========================
// Dummy sqlite 模块：用于模拟 sqlite 库
// ==========================
pub mod sqlite {
    #[derive(Debug)]
    pub struct OpenFlags;

    impl OpenFlags {
        pub fn new() -> Self {
            OpenFlags
        }
    }

    #[derive(Debug)]
    pub struct Connection;

    /// 模拟打开数据库（忽略 flags 参数）
    pub fn open(path: &str) -> Result<Connection, &'static str> {
        println!("Dummy sqlite::open called with path: {}", path);
        Ok(Connection)
    }

    impl Connection {
        pub fn execute(&self, sql: &str) -> Result<(), &'static str> {
            println!("Dummy execute: {}", sql);
            Ok(())
        }

        pub fn prepare(&self, sql: &str) -> Result<Statement, &'static str> {
            println!("Dummy prepare: {}", sql);
            Ok(Statement::new())
        }
    }

    #[derive(Debug)]
    pub struct Statement {
        called: bool,
    }

    impl Statement {
        pub fn new() -> Self {
            Statement { called: false }
        }
        pub fn next(&mut self) -> Result<Option<Row>, &'static str> {
            if !self.called {
                self.called = true;
                Ok(Some(Row))
            } else {
                Ok(None)
            }
        }
    }

    #[derive(Debug)]
    pub struct Row;

    // ---------- 为 Row 添加辅助 trait ----------
    pub trait FromDummyRow: Sized {
        fn from_dummy(s: &str) -> Result<Self, &'static str>;
    }

    impl FromDummyRow for i64 {
        fn from_dummy(s: &str) -> Result<Self, &'static str> {
            s.parse::<i64>().map_err(|_| "Failed to parse i64")
        }
    }

    impl FromDummyRow for String {
        fn from_dummy(s: &str) -> Result<Self, &'static str> {
            Ok(s.to_string())
        }
    }

    impl Row {
        pub fn get<T: FromDummyRow>(&self, idx: usize) -> Result<T, &'static str> {
            let s = match idx {
                0 => "1",
                1 => "Alice",
                _ => return Err("Index out of bounds"),
            };
            T::from_dummy(s)
        }
    }
}

// ==========================
// Dummy jammdb 模块：用于模拟 jammdb 库（键值存储）
// ==========================
pub mod jammdb {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Debug)]
    pub struct DB {
        pub data: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    }

    impl DB {
        pub fn tx(&self, _writable: bool) -> Result<Transaction, &'static str> {
            Ok(Transaction {
                data: self.data.clone(),
            })
        }
    }

    #[derive(Debug)]
    pub struct Transaction {
        pub data: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    }

    impl Transaction {
        pub fn get(&self, key: &str) -> Option<Vec<u8>> {
            self.data.lock().unwrap().get(key).cloned()
        }
        pub fn put(&mut self, key: &str, value: &[u8]) -> Result<(), &'static str> {
            self.data.lock().unwrap().insert(key.to_string(), value.to_vec());
            Ok(())
        }
    }

    pub struct OpenOptions;

    impl OpenOptions {
        pub fn new() -> Self {
            OpenOptions
        }
        pub fn open(&self, _path: &str) -> Result<DB, &'static str> {
            Ok(DB {
                data: Arc::new(Mutex::new(HashMap::new())),
            })
        }
    }
}

// ==========================
// 引入 sqlite-vfs（由本地 fork 提供）
// ==========================
use async_trait::async_trait;
use jammdb::{DB, OpenOptions as JammdbOpenOptions, Transaction};
use sqlite_vfs::{
    register_vfs, Vfs, VfsFile, OpenOptions as VfsOpenOptions, Result as VfsResult,
};
use std::sync::{Arc, Mutex};
use tokio::task;

// ---------- 定义 open_with_flags_and_vfs ----------
// 此函数用于模拟“通过 VFS 打开 SQLite”，这里只是调用 dummy 的 sqlite::open
fn open_with_flags_and_vfs(path: &str, _flags: sqlite::OpenFlags, vfs: &str) -> sqlite::Connection {
    println!("Opening SQLite DB with VFS: {}", vfs);
    sqlite::open(path).expect("failed to open sqlite")
}

// ---------- 异步 JammDB VFS 实现 ----------
pub struct AsyncJammDbVfs {
    db: DB,
}

impl AsyncJammDbVfs {
    /// 异步创建一个 JammDB 数据库实例
    pub async fn new(path: &str) -> Self {
        let path = path.to_string();
        let db = task::spawn_blocking(move || {
            JammdbOpenOptions::new()
                .open(&path)
                .expect("Failed to open JammDB")
        })
        .await
        .expect("Task failed");
        Self { db }
    }
}

impl Vfs for AsyncJammDbVfs {
    type File = AsyncJammDbFile;

    fn open(&self, name: &str, _options: VfsOpenOptions) -> VfsResult<Self::File> {
        let txn = self.db.tx(true).expect("Failed to open transaction");
        Ok(AsyncJammDbFile {
            txn: Arc::new(Mutex::new(txn)),
            key: name.to_string(),
        })
    }
}

// ---------- 异步 JammDB 文件封装 ----------
pub struct AsyncJammDbFile {
    txn: Arc<Mutex<Transaction>>,
    key: String,
}

#[async_trait]
impl VfsFile for AsyncJammDbFile {
    async fn read_at(&self, _offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        let key = self.key.clone();
        let txn = self.txn.clone();
        // 在 spawn_blocking 中获取数据（不直接捕获 buf）
        let data_opt: Option<Vec<u8>> = task::spawn_blocking(move || {
            let txn = txn.lock().expect("Failed to lock transaction");
            txn.get(&key)
        })
        .await
        .expect("Task failed");
        if let Some(data) = data_opt {
            let len = data.len().min(buf.len());
            buf[..len].copy_from_slice(&data[..len]);
            Ok(len)
        } else {
            Ok(0)
        }
    }

    async fn write_at(&self, _offset: u64, buf: &[u8]) -> VfsResult<usize> {
        let key = self.key.clone();
        let data = buf.to_vec();
        let txn = self.txn.clone();
        task::spawn_blocking(move || {
            let mut txn = txn.lock().expect("Failed to lock transaction");
            txn.put(&key, &data).expect("Failed to write data");
            Ok(data.len())
        })
        .await
        .expect("Task failed")
    }
}

// ---------- 使用 SQLite + 异步 JammDB VFS ----------
#[tokio::main]
async fn main() {
    println!("Starting Async JammDB VFS demo...");

    // 1. 创建异步 VFS 实例（数据存储在 jammdb.db 中）
    let vfs = AsyncJammDbVfs::new("jammdb.db").await;
    // 2. 注册异步 VFS，名称为 "jammdb_async"
    register_vfs("jammdb_async", vfs, true).expect("Failed to register VFS");

    // 3. 使用异步 VFS 打开 SQLite 数据库（此处使用内存数据库作为示例）
    let conn = task::spawn_blocking(|| {
        open_with_flags_and_vfs(":memory:", sqlite::OpenFlags::new(), "jammdb_async")
    })
    .await
    .expect("Task failed");

    // 将 Connection 包装到 Arc<Mutex> 中以实现共享所有权
    let conn = Arc::new(Mutex::new(conn));

    // 4. 执行 SQL：创建表并插入数据
    let conn_clone = conn.clone();
    task::spawn_blocking(move || {
        let conn = conn_clone.lock().unwrap();
        conn.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
            .expect("Failed to create table");
        conn.execute("INSERT INTO users (name) VALUES ('Alice')")
            .expect("Failed to insert data");
    })
    .await
    .expect("Task failed");

    // 5. 准备查询语句
    let stmt = {
        let conn = conn.lock().unwrap();
        conn.prepare("SELECT * FROM users")
            .expect("Failed to prepare statement")
    };

    // 将 Statement 包装到 Arc<Mutex> 中
    let stmt = Arc::new(Mutex::new(stmt));

    println!("Query results:");
    loop {
        // 在闭包中获取下一行
        let stmt_clone = stmt.clone();
        let row_opt = task::spawn_blocking(move || {
            let mut stmt = stmt_clone.lock().unwrap();
            stmt.next()
        })
        .await
        .expect("Task failed");

        match row_opt {
            Ok(Some(row)) => {
                let id: i64 = row.get(0).expect("Failed to read id");
                let name: String = row.get(1).expect("Failed to read name");
                println!("User {}: {}", id, name);
            }
            Ok(None) => break,
            Err(e) => panic!("Error: {}", e),
        }
    }
}