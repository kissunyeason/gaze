// SPDX-FileCopyrightText: 2026 Gundu Labs
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use gaze_core::acceleration::{NpuDevice, discover_npus, provider_runtime};
use std::path::Path;

fn register_fix(library: &Path) -> String {
    format!(
        "Link the vendor's libonnxruntime.so to {} and list its SDK library directories in {}; then restart gazed.",
        library.display(),
        library.with_file_name("library-path").display()
    )
}

// OpenVINO can also drive an Intel GPU or CPU, so only NPU devices need /sys/class/accel.
fn active_provider<'a>(config: Option<&'a Config>, devices: &[NpuDevice]) -> Option<&'a str> {
    let inference = &config?.inference;
    match inference.execution_provider.as_str() {
        "cpu" => None,
        "auto" => devices.first().map(|device| device.provider),
        provider => Some(provider),
    }
}

pub(super) fn check_acceleration(report: &mut Report, config: Option<&Config>) {
    let devices = discover_npus();
    let wants_npu = config.is_some_and(|config| {
        config.inference.execution_provider != "cpu" && config.inference.device == "npu"
    });
    if devices.is_empty() && wants_npu {
        report.warning(
            "NPU 硬件",
            "/sys/class/accel 中没有绑定受支持的 NPU 驱动",
            "请安装厂商的 NPU 内核驱动与固件；仅有 Intel 或 AMD CPU 并不代表存在 NPU。",
        );
    }
    for device in &devices {
        report.pass(
            "NPU 硬件",
            format!(
                "{} 使用 {}（{}）",
                device.node.display(),
                device.driver,
                device.provider
            ),
        );
        if !device.node.exists() {
            report.warning(
                "NPU 设备",
                format!("缺少 {}", device.node.display()),
                "请检查 NPU 内核驱动、固件以及 /dev/accel 的权限。",
            );
        }
    }

    let Some(config) = config else {
        return;
    };
    if let Some(provider) = active_provider(Some(config), &devices) {
        let library = provider_runtime(provider);
        if library.is_file() {
            report.pass(
                "加速器运行时",
                format!(
                    "{} 已注册；可用 `gaze doctor --benchmark` 检查模型会话",
                    library.display()
                ),
            );
        } else {
            report.warning(
                "加速器运行时",
                format!("已配置 {provider}，但尚未注册"),
                register_fix(&library),
            );
        }
        return;
    }

    let mut providers: Vec<&str> = devices.iter().map(|device| device.provider).collect();
    providers.sort_unstable();
    providers.dedup();
    for provider in providers {
        let library = provider_runtime(provider);
        if library.is_file() {
            report.off(
                "NPU 加速",
                format!("{provider} 运行时已注册；当前配置为 CPU"),
                "在 `gaze config` 中选择 auto/npu，然后重启 gazed。",
            );
        } else {
            report.off(
                "NPU 加速",
                format!("an NPU is present but no {provider} runtime is registered"),
                format!(
                    "{} 然后在 `gaze config` 中选择 auto/npu。",
                    register_fix(&library)
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaze_core::config::Config;
    use std::path::PathBuf;

    fn config_with_provider(provider: &str) -> Config {
        let mut config = Config::default();
        config.inference.execution_provider = provider.to_string();
        config
    }

    fn device(provider: &'static str) -> NpuDevice {
        NpuDevice {
            node: PathBuf::from("/dev/accel/accel0"),
            driver: "intel_vpu".to_string(),
            provider,
        }
    }

    #[test]
    fn cpu_never_selects_a_provider_even_with_npus_present() {
        let config = config_with_provider("cpu");
        let devices = vec![device("openvino")];
        assert_eq!(active_provider(Some(&config), &devices), None);
    }

    #[test]
    fn auto_selects_the_first_discovered_npu() {
        let config = config_with_provider("auto");
        let devices = vec![device("openvino"), device("vitis")];
        assert_eq!(active_provider(Some(&config), &devices), Some("openvino"));
    }

    #[test]
    fn auto_with_no_npu_selects_nothing() {
        let config = config_with_provider("auto");
        assert_eq!(active_provider(Some(&config), &[]), None);
    }

    #[test]
    fn explicit_provider_is_used_verbatim() {
        let config = config_with_provider("openvino");
        assert_eq!(active_provider(Some(&config), &[]), Some("openvino"));
        let config = config_with_provider("vitis");
        let devices = vec![device("openvino")];
        assert_eq!(active_provider(Some(&config), &devices), Some("vitis"));
    }

    #[test]
    fn missing_config_selects_nothing() {
        let devices = vec![device("openvino")];
        assert_eq!(active_provider(None, &devices), None);
    }

    #[test]
    fn register_fix_names_the_library_and_its_config_file() {
        let library = Path::new("/usr/lib/gaze/runtimes/openvino/libonnxruntime.so");
        let fix = register_fix(library);
        assert!(
            fix.contains("/usr/lib/gaze/runtimes/openvino/libonnxruntime.so"),
            "{fix}"
        );
        assert!(
            fix.contains("/usr/lib/gaze/runtimes/openvino/library-path"),
            "{fix}"
        );
        assert!(fix.contains("restart gazed"), "{fix}");
    }
}
