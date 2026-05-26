// 防止 release 模式打开多余的命令行窗口（仅 windows 生效）
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    qianshou_client_lib::run()
}
