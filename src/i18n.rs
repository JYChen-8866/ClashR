//! Minimal i18n — static locale with compile-time key lookup.
//!
//! Usage: `t("home.proxy_mode")` returns the localized string for the
//! current locale. Locale is stored in Preferences and applied at startup.

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::RwLock;

static LOCALE: Lazy<RwLock<&'static str>> = Lazy::new(|| RwLock::new("en"));

pub fn set_locale(locale: &str) {
    let l = match locale {
        "zh" | "zh-CN" | "zh-Hans" => "zh",
        _ => "en",
    };
    *LOCALE.write().unwrap() = l;
}

pub fn current_locale() -> &'static str {
    *LOCALE.read().unwrap()
}

pub fn t(key: &str) -> &'static str {
    let locale = current_locale();
    let map = match locale {
        "zh" => &*ZH,
        _ => &*EN,
    };
    map.get(key).copied().unwrap_or_else(|| {
        // Fallback to English if key missing in current locale
        EN.get(key).copied().unwrap_or("???")
    })
}

pub fn available_locales() -> &'static [(&'static str, &'static str)] {
    &[("en", "English"), ("zh", "中文")]
}

static EN: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut m = HashMap::new();
    // Sidebar
    m.insert("nav.home", "Home");
    m.insert("nav.proxies", "Proxies");
    m.insert("nav.profiles", "Profiles");
    m.insert("nav.connections", "Connections");
    m.insert("nav.rules", "Rules");
    m.insert("nav.logs", "Logs");
    m.insert("nav.settings", "Settings");
    // Home
    m.insert("home.proxy_mode", "Proxy Mode");
    m.insert("home.network", "Network");
    m.insert("home.net_off", "Off");
    m.insert("home.net_sysproxy", "System Proxy");
    m.insert("home.net_tun", "TUN");
    m.insert("home.traffic", "Traffic");
    m.insert("home.upload_speed", "Upload");
    m.insert("home.download_speed", "Download");
    m.insert("home.active_conn", "Active");
    m.insert("home.upload_total", "Up Total");
    m.insert("home.download_total", "Down Total");
    m.insert("home.memory", "Memory");
    m.insert("home.site_test", "Site Test");
    m.insert("home.ip_info", "IP Info");
    m.insert("home.collecting", "Collecting traffic…");
    m.insert("home.global_node", "GLOBAL Node");
    // Proxies
    m.insert("proxies.test", "Test");
    m.insert("proxies.refresh", "Refresh");
    // Settings
    m.insert("settings.title", "Settings");
    m.insert("settings.appearance", "Appearance");
    m.insert("settings.theme", "Theme");
    m.insert("settings.language", "Language");
    m.insert("settings.system_proxy", "System Proxy");
    m.insert("settings.helper_service", "Helper Service");
    m.insert("settings.clash_core", "Clash Core");
    m.insert("settings.restart_core", "Restart Core");
    m.insert("settings.stop_core", "Stop Core");
    m.insert("settings.install_helper", "Install Helper");
    m.insert("settings.uninstall_helper", "Uninstall Helper");
    m.insert("settings.svc_running", "Installed and running");
    m.insert("settings.svc_stopped", "Installed but not running");
    m.insert("settings.svc_none", "Not installed");
    // Home mode buttons
    m.insert("home.mode_rule", "Rule");
    m.insert("home.mode_global", "Global");
    m.insert("home.mode_direct", "Direct");
    m.insert("home.ip_info", "IP Info");
    m.insert("home.test", "Test");
    m.insert("home.testing", "Testing");
    m.insert("home.query_failed", "Query failed");
    // Layout
    m.insert("nav.navigation", "Navigation");
    m.insert("status.running", "Running");
    m.insert("status.stopped", "Stopped");
    m.insert("status.starting", "Starting");
    m.insert("status.stopping", "Stopping");
    m.insert("status.failed", "Failed");
    // Connections
    m.insert("conn.active", "active");
    m.insert("conn.close_all", "Close All");
    m.insert("conn.host", "Host");
    m.insert("conn.network", "Net");
    m.insert("conn.process", "Process");
    m.insert("conn.rule", "Rule");
    m.insert("conn.chains", "Chain");
    m
});

static ZH: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("nav.home", "主页");
    m.insert("nav.proxies", "代理");
    m.insert("nav.profiles", "订阅");
    m.insert("nav.connections", "连接");
    m.insert("nav.rules", "规则");
    m.insert("nav.logs", "日志");
    m.insert("nav.settings", "设置");
    m.insert("home.proxy_mode", "代理模式");
    m.insert("home.network", "网络设置");
    m.insert("home.net_off", "关闭");
    m.insert("home.net_sysproxy", "系统代理");
    m.insert("home.net_tun", "TUN");
    m.insert("home.traffic", "流量统计");
    m.insert("home.upload_speed", "上传速度");
    m.insert("home.download_speed", "下载速度");
    m.insert("home.active_conn", "活跃连接");
    m.insert("home.upload_total", "上传量");
    m.insert("home.download_total", "下载量");
    m.insert("home.memory", "内核占用");
    m.insert("home.site_test", "网站测速");
    m.insert("home.ip_info", "IP 信息");
    m.insert("home.collecting", "正在收集流量数据…");
    m.insert("home.global_node", "GLOBAL 节点");
    m.insert("proxies.test", "测速");
    m.insert("proxies.refresh", "刷新");
    m.insert("settings.title", "设置");
    m.insert("settings.appearance", "外观");
    m.insert("settings.theme", "主题");
    m.insert("settings.language", "语言");
    m.insert("settings.system_proxy", "系统代理");
    m.insert("settings.helper_service", "辅助服务");
    m.insert("settings.clash_core", "内核管理");
    m.insert("settings.restart_core", "重启内核");
    m.insert("settings.stop_core", "停止内核");
    m.insert("settings.install_helper", "安装服务");
    m.insert("settings.uninstall_helper", "卸载服务");
    m.insert("settings.svc_running", "已安装，运行中");
    m.insert("settings.svc_stopped", "已安装，未运行");
    m.insert("settings.svc_none", "未安装");
    m.insert("home.mode_rule", "规则");
    m.insert("home.mode_global", "全局");
    m.insert("home.mode_direct", "直连");
    m.insert("home.ip_info", "IP 信息");
    m.insert("home.test", "测试");
    m.insert("home.testing", "测试中");
    m.insert("home.query_failed", "查询失败");
    m.insert("nav.navigation", "导航");
    m.insert("status.running", "运行中");
    m.insert("status.stopped", "已停止");
    m.insert("status.starting", "启动中");
    m.insert("status.stopping", "停止中");
    m.insert("status.failed", "失败");
    m.insert("conn.active", "活跃");
    m.insert("conn.close_all", "全部关闭");
    m.insert("conn.host", "目标");
    m.insert("conn.network", "网络");
    m.insert("conn.process", "进程");
    m.insert("conn.rule", "规则");
    m.insert("conn.chains", "链路");
    m
});
