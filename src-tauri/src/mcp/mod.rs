//! MCP server —— 把 notemd 的检索面供给 agent。
//!
//! 进程拓扑见 `docs/superpowers/specs/2026-08-19-notemd-mcp-server-design.md`:
//! agent --stdio--> `notemd mcp` 外壳 --UDS/管道--> GUI 主程序。
//! 外壳与 server 是同一个二进制,于是工具 schema 是同一个编译期常量,
//! 两边不可能对不上 —— 不靠约定,靠编译。

pub mod dispatch;
pub mod gate;
pub mod protocol;
pub mod roots;
pub mod server;
pub mod shim;
pub mod tools;
