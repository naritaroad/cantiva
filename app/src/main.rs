// 正式版不要跳出主控台視窗；--headless 模式由 cantiva-mcp 以隱藏方式啟動。
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod headless;
mod gui;
mod runtime;

fn main() -> anyhow::Result<()> {
    let headless = std::env::args().any(|a| a == "--headless");
    let rt = runtime::Runtime::start(headless)?;
    if headless {
        return headless::run(rt);
    }
    gui::run(rt)
}
