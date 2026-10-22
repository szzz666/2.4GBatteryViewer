fn main() {
    // 换了图标/配置必须让 build script 重新跑一遍，否则 tauri-codegen 会继续用
    // OUT_DIR 里缓存的旧图标（exe 资源图标和窗口图标都会停在旧的 Tauri 默认图标上）
    println!("cargo:rerun-if-changed=icons");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    tauri_build::build()
}
