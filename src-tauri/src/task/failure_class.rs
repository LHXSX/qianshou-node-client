//! 失败分类 (failure classification)
//!
//! 从子进程 exit_code + stderr + 错误信息,推断任务失败的**根因类别**。
//! 输出有两个用途:
//!   1. 上报后端 (调度器据此区分对待:环境问题不该把节点判为"不胜任",
//!      硬件不足才该降级) — 见 platform_v8/engine/capability_feedback.py
//!   2. 本地自愈决策 (env_missing_pkg → pip install · env_broken_venv → 重建 tier)
//!      — 见 self_heal.rs
//!
//! 设计原则: 宁可保守 (Unknown / ScriptError) 也不要误判成"可自愈"后乱装东西。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureClass {
    /// 缺 Python 包 (ModuleNotFoundError / ImportError) → 可自愈: pip install
    EnvMissingPkg,
    /// venv 损坏 / Python 解释器起不来 → 可自愈: 重建对应 tier
    EnvBrokenVenv,
    /// 缺系统工具 (ffmpeg / blender 等) → 可自愈: 装 portable 包
    EnvMissingTool,
    /// 内存不足 / OOM / 被 Killed → 不自愈,硬件不够,降级
    ResourceOom,
    /// 磁盘满 → 先清缓存,清不出来再降级
    ResourceDiskFull,
    /// 脚本自身 bug (Traceback 非 import) → 不自愈 (平台侧脚本问题)
    ScriptError,
    /// 网络/下载失败 (code_url 拉不到 / 连接问题) → 退避重试
    NetworkError,
    /// 任务超时 → 不自愈 (任务太大或机器太慢)
    Timeout,
    /// 无法归类
    Unknown,
}

impl FailureClass {
    /// 上报后端用的稳定字符串 (对应 we_shards.failure_class)
    pub fn as_str(&self) -> &'static str {
        match self {
            FailureClass::EnvMissingPkg => "env_missing_pkg",
            FailureClass::EnvBrokenVenv => "env_broken_venv",
            FailureClass::EnvMissingTool => "env_missing_tool",
            FailureClass::ResourceOom => "resource_oom",
            FailureClass::ResourceDiskFull => "resource_disk_full",
            FailureClass::ScriptError => "script_error",
            FailureClass::NetworkError => "network_error",
            FailureClass::Timeout => "timeout",
            FailureClass::Unknown => "unknown",
        }
    }

    /// 从上报字符串反解 (供 v8_ws 拿到 result.failure_class 后判断是否自愈)
    pub fn parse(s: &str) -> FailureClass {
        match s {
            "env_missing_pkg" => FailureClass::EnvMissingPkg,
            "env_broken_venv" => FailureClass::EnvBrokenVenv,
            "env_missing_tool" => FailureClass::EnvMissingTool,
            "resource_oom" => FailureClass::ResourceOom,
            "resource_disk_full" => FailureClass::ResourceDiskFull,
            "script_error" => FailureClass::ScriptError,
            "network_error" => FailureClass::NetworkError,
            "timeout" => FailureClass::Timeout,
            _ => FailureClass::Unknown,
        }
    }

    /// 是否值得本地自愈 (其余的要么平台侧问题,要么硬件不够,自愈无意义)
    pub fn is_self_healable(&self) -> bool {
        matches!(
            self,
            FailureClass::EnvMissingPkg
                | FailureClass::EnvBrokenVenv
                | FailureClass::EnvMissingTool
                | FailureClass::ResourceDiskFull
        )
    }
}

#[derive(Debug, Clone)]
pub struct Classification {
    pub class: FailureClass,
    /// 缺失的依赖名 (pip 包名 · 已做 import→pip 映射) · 仅 EnvMissingPkg/EnvMissingTool 有值
    pub missing_dep: Option<String>,
    /// 人类可读的简短说明 (拼进上报)
    pub detail: String,
}

/// 从 exit_code + stderr + error_msg 推断失败分类。
///
/// 判定顺序很重要 (从"最明确"到"最模糊"):
///   超时 → 网络 → OOM → 磁盘满 → 缺包 → 坏 venv → 缺工具 → 脚本 bug → Unknown
pub fn classify(exit_code: Option<i32>, stderr: &str, error_msg: &str) -> Classification {
    let hay = format!("{}\n{}", error_msg, stderr);
    let low = hay.to_lowercase();

    let mk = |class: FailureClass, dep: Option<String>, detail: &str| Classification {
        class,
        missing_dep: dep,
        detail: detail.to_string(),
    };

    // 1. 超时 (executor 的超时错误信息 / Python 侧 TimeoutError)
    if low.contains("超时") || low.contains("timeouterror") || low.contains("timed out") {
        return mk(FailureClass::Timeout, None, "任务执行超时");
    }

    // 2. 网络 / 下载 (code_url 拉不到 · DNS · 连接)
    if low.contains("下载脚本")
        || low.contains("脚本内容为空")
        || low.contains("connection refused")
        || low.contains("failed to connect")
        || low.contains("temporary failure in name resolution")
        || low.contains("dns")
        || low.contains("http 5")
        || low.contains("http 404")
        || low.contains("connectionerror")
        || low.contains("max retries exceeded")
    {
        return mk(FailureClass::NetworkError, None, "网络/脚本下载失败");
    }

    // 3. OOM / 被杀 (exit -9 = SIGKILL · MemoryError · OOM)
    if exit_code == Some(-9)
        || exit_code == Some(137) // 128+9 · 有些壳层这样回
        || low.contains("memoryerror")
        || low.contains("out of memory")
        || low.contains("oomkilled")
        || low.contains("cannot allocate memory")
        || low.contains("killed")
    {
        return mk(FailureClass::ResourceOom, None, "内存不足/进程被杀(OOM)");
    }

    // 4. 磁盘满
    if low.contains("no space left")
        || low.contains("disk full")
        || low.contains("errno 28")
        || low.contains("磁盘空间不足")
    {
        return mk(FailureClass::ResourceDiskFull, None, "磁盘空间不足");
    }

    // 5. 缺 Python 包 (ModuleNotFoundError: No module named 'X')
    if let Some(import_name) = extract_missing_module(&hay) {
        let pip = import_to_pip(&import_name);
        return mk(
            FailureClass::EnvMissingPkg,
            Some(pip.clone()),
            &format!("缺 Python 包: import {} → pip {}", import_name, pip),
        );
    }
    if low.contains("importerror") || low.contains("dll load failed") {
        // ImportError(含 Win 上的 "DLL load failed") · 拿不到具体包名,但确属环境
        return mk(FailureClass::EnvMissingPkg, None, "Python 包导入失败(ImportError)");
    }

    // 6. 坏 venv / 解释器起不来 (executor.rs 的明确错误信息)
    if low.contains("无可用 python")
        || low.contains("候选全部启动失败")
        || low.contains("script 模式缺少 code_url")
        || (low.contains("venv") && low.contains("not found"))
    {
        return mk(FailureClass::EnvBrokenVenv, None, "venv/Python 解释器不可用");
    }

    // 7. 缺系统工具
    for (kw, tool) in [
        ("ffmpeg", "ffmpeg"),
        ("ffprobe", "ffmpeg"),
        ("blender", "blender"),
        ("tesseract", "tesseract"),
    ] {
        if low.contains(kw)
            && (low.contains("not found")
                || low.contains("未安装")
                || low.contains("no such file")
                || low.contains("command not found")
                || low.contains("is not recognized"))
        {
            return mk(
                FailureClass::EnvMissingTool,
                Some(tool.to_string()),
                &format!("缺系统工具: {}", tool),
            );
        }
    }

    // 8. 脚本自身 bug: 有 Python Traceback 但不是 import/资源问题
    if low.contains("traceback (most recent call last)") {
        return mk(FailureClass::ScriptError, None, "脚本运行抛异常(Traceback)");
    }

    // 9. 退出码 1 但没抓到更具体的信号 → 多半脚本逻辑/参数问题
    if exit_code == Some(1) {
        return mk(FailureClass::ScriptError, None, "脚本非零退出(exit 1)");
    }

    mk(FailureClass::Unknown, None, "未能归类的失败")
}

/// 从 stderr 里抓 "No module named 'X'" 的 X (取顶层包名 · X.Y → X)
fn extract_missing_module(text: &str) -> Option<String> {
    // 匹配 No module named 'xxx' 或 "No module named xxx"
    let marker = "no module named";
    let low = text.to_lowercase();
    let pos = low.find(marker)?;
    let rest = &text[pos + marker.len()..];
    // 跳过空格和引号
    let rest = rest.trim_start();
    let rest = rest.trim_start_matches(['\'', '"']);
    // 取到下一个引号/空白/换行
    let end = rest
        .find(|c: char| c == '\'' || c == '"' || c.is_whitespace())
        .unwrap_or(rest.len());
    let name = &rest[..end];
    if name.is_empty() {
        return None;
    }
    // 顶层包名 (a.b.c → a)
    let top = name.split('.').next().unwrap_or(name);
    Some(top.to_string())
}

/// import 名 → pip 包名 (常见不一致的映射 · 其余默认同名)
fn import_to_pip(import_name: &str) -> String {
    match import_name {
        "PIL" => "pillow",
        "fitz" => "pymupdf",
        "cv2" => "opencv-python-headless",
        "sklearn" => "scikit-learn",
        "bs4" => "beautifulsoup4",
        "yaml" => "pyyaml",
        "skimage" => "scikit-image",
        "OpenSSL" => "pyopenssl",
        "dateutil" => "python-dateutil",
        "faster_whisper" => "faster-whisper",
        other => other,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_missing_pkg_with_mapping() {
        let c = classify(
            Some(1),
            "Traceback (most recent call last):\n  ...\nModuleNotFoundError: No module named 'PIL'",
            "exit code 1",
        );
        assert_eq!(c.class, FailureClass::EnvMissingPkg);
        assert_eq!(c.missing_dep.as_deref(), Some("pillow"));
    }

    #[test]
    fn test_missing_pkg_submodule_takes_top() {
        let c = classify(Some(1), "ModuleNotFoundError: No module named 'torch.nn'", "");
        assert_eq!(c.class, FailureClass::EnvMissingPkg);
        assert_eq!(c.missing_dep.as_deref(), Some("torch"));
    }

    #[test]
    fn test_oom() {
        let c = classify(Some(-9), "", "exit code -9");
        assert_eq!(c.class, FailureClass::ResourceOom);
        assert!(!c.class.is_self_healable());
    }

    #[test]
    fn test_oom_memoryerror() {
        let c = classify(Some(1), "MemoryError", "exit code 1");
        assert_eq!(c.class, FailureClass::ResourceOom);
    }

    #[test]
    fn test_disk_full() {
        let c = classify(Some(1), "OSError: [Errno 28] No space left on device", "");
        assert_eq!(c.class, FailureClass::ResourceDiskFull);
        assert!(c.class.is_self_healable());
    }

    #[test]
    fn test_timeout() {
        let c = classify(None, "", "任务执行超时（300秒）");
        assert_eq!(c.class, FailureClass::Timeout);
    }

    #[test]
    fn test_network() {
        let c = classify(None, "", "下载脚本 HTTP 404: https://x/y.py");
        assert_eq!(c.class, FailureClass::NetworkError);
    }

    #[test]
    fn test_broken_venv() {
        let c = classify(
            None,
            "",
            "节点无可用 Python:5 个候选全部启动失败 (最后: python3 (No such file))",
        );
        assert_eq!(c.class, FailureClass::EnvBrokenVenv);
    }

    #[test]
    fn test_missing_tool() {
        let c = classify(Some(1), "ffmpeg: command not found", "exit code 1");
        assert_eq!(c.class, FailureClass::EnvMissingTool);
        assert_eq!(c.missing_dep.as_deref(), Some("ffmpeg"));
    }

    #[test]
    fn test_script_error() {
        let c = classify(
            Some(1),
            "Traceback (most recent call last):\n  File ...\nValueError: bad input",
            "exit code 1",
        );
        assert_eq!(c.class, FailureClass::ScriptError);
    }

    #[test]
    fn test_unknown() {
        let c = classify(Some(2), "weird", "exit code 2");
        assert_eq!(c.class, FailureClass::Unknown);
    }
}
