//! Runtime manager · 客户端隔离 Python 运行时
//!
//! 设计 (2026-05-20):
//!   - 后端 /api/v8/runtime/manifest 下发 mirrors + tiers (动态)
//!   - 客户端按 tier 在 ~/.qianshou/runtime/venvs/<tier> 创建 venv
//!   - 依赖从公共镜像装 (阿里/清华/中科大/PyPI · 自动 fallback)
//!   - 安装后自检 (smoke_test) → 写 installed.json
//!   - executor.rs 任务执行时使用对应 venv 的 python
//!   - WS hello 时读 installed.json 上报真实 runtime_tiers/software
//!
//! 用户角度: 工具页一个按钮"一键安装运行环境" → 客户端自动 venv + pip + 自检
//!
//! 不再依赖系统 pip/brew/PATH

pub mod manifest;
pub mod installer;
pub mod detector;
pub mod paths;
pub mod uv;
pub mod skills_fetcher;
pub mod commands;
pub mod bootstrap_bundled;
pub mod garbage_collect;
pub mod auto_install_tiers;
