fn main() {
    // fluent-dark：std-widgets（輸入框、下拉選單）用深色外觀，和自訂元件一致。
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent-dark".into())
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles);
    slint_build::compile_with_config("ui/main.slint", config).expect("Slint 編譯失敗");

    // Windows：把圖示與版本資訊嵌進 cantiva.exe。
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico")
            .set("ProductName", "Cantiva")
            .set("FileDescription", "Cantiva 本機語音工作室");
        if let Err(e) = res.compile() {
            println!("cargo:warning=無法嵌入 Windows 圖示：{e}");
        }
    }
    println!("cargo:rerun-if-changed=assets/icon.ico");
}
