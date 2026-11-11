// SPDX-FileCopyrightText: 2026 Gundu Labs
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

pub(super) fn check_systemd(report: &mut Report) {
    if !Path::new("/run/systemd/system").exists() {
        report.warning(
            "systemd",
            "systemd 未运行，无法检查 gazed 服务状态",
            "在常规安装环境中，请以 systemd 启动系统后运行 `systemctl status gazed`。",
        );
        return;
    }

    match command_output("systemctl", &["is-active", "gazed"]) {
        Ok((true, state)) if state == "active" => report.pass("Service", "gazed 正在运行"),
        Ok((_, state)) => report.error(
            "Service",
            format!("gazed 状态为 {state}"),
            "运行 `sudo systemctl enable --now gazed`；若失败，用 `journalctl -u gazed -n 100 --no-pager` 查看原因。",
        ),
        Err(err) => report.error(
            "Service",
            format!("无法查询 gazed：{err}"),
            "运行 `systemctl status gazed`。",
        ),
    }

    match command_output("systemctl", &["is-enabled", "gazed"]) {
        Ok((true, state)) if state == "enabled" => {
            report.pass("开机自启", "gazed 已设置为开机自启");
        }
        Ok((_, state)) => report.warning(
            "开机自启",
            format!("gazed 状态为 {state}"),
            "运行 `sudo systemctl enable gazed`，以便重启后认证仍可用。",
        ),
        Err(err) => report.warning(
            "开机自启",
            format!("无法查询 gazed 的自启状态：{err}"),
            "运行 `systemctl is-enabled gazed`。",
        ),
    }
}

pub(super) fn check_config(report: &mut Report) -> Option<Config> {
    let path = Path::new(CONFIG_PATH);
    if !path.exists() {
        report.error(
            "配置",
            format!("{CONFIG_PATH} 不存在"),
            "请重装 Gaze 或恢复软件包自带的配置文件。",
        );
        return None;
    }

    check_config_permissions(report, path);

    let config = match Config::load_from(CONFIG_PATH) {
        Ok(config) => {
            let unknown = fs::read_to_string(CONFIG_PATH)
                .map(|contents| unknown_config_keys(&contents))
                .unwrap_or_default();
            if unknown.is_empty() {
                report.pass("配置", format!("{CONFIG_PATH} 解析成功"));
            } else {
                report.warning(
                    "配置",
                    format!(
                        "{CONFIG_PATH} 可解析，但 Gaze 不会读取：{}",
                        unknown.join(", ")
                    ),
                    "请删除或修正这些键；它们不会生效，拼写错误会导致设置被静默忽略。",
                );
            }
            config
        }
        Err(err)
            if err
                .downcast_ref::<std::io::Error>()
                .is_some_and(|err| err.kind() == std::io::ErrorKind::PermissionDenied) =>
        {
            report.pass(
                "配置",
                format!(
                    "此处无法读取 {CONFIG_PATH}；数值已通过 gazed 检查，但未检查文件中的未知键"
                ),
            );
            return None;
        }
        Err(err) => {
            report.error(
                "配置",
                format!("无法加载 {CONFIG_PATH}：{err}"),
                "请检查文件并修正 TOML 语法，然后运行 `sudo systemctl restart gazed`。",
            );
            return None;
        }
    };

    for check in config_findings(&config) {
        report.checks.push(check);
    }

    Some(config)
}

pub(super) fn check_config_permissions(report: &mut Report, path: &Path) {
    match fs::metadata(path) {
        Ok(metadata) => {
            let mode = metadata.mode() & 0o777;
            if metadata.uid() != 0 {
                report.error(
                    "配置文件归属",
                    format!("{CONFIG_PATH} 的所有者为 UID {}", metadata.uid()),
                    format!("请运行 `sudo chown root:root {CONFIG_PATH}`。"),
                );
            } else if mode & 0o022 != 0 {
                report.error(
                    "配置文件权限",
                    format!("{CONFIG_PATH} 的权限包含写位：{mode:o}"),
                    format!("请运行 `sudo chmod 0644 {CONFIG_PATH}`。"),
                );
            } else {
                report.pass(
                    "配置文件权限",
                    format!("属于 root，且同组与其他用户不可写（{mode:o}）"),
                );
            }
        }
        Err(err) => report.error(
            "配置文件权限",
            format!("无法检查 {CONFIG_PATH}：{err}"),
            format!("请运行 `sudo stat {CONFIG_PATH}`。"),
        ),
    }
}

pub(super) fn config_findings(config: &Config) -> Vec<Check> {
    let mut findings = Vec::new();
    let mut error = |message: String, fix: &'static str| {
        findings.push(Check {
            level: Level::Error,
            name: "配置项取值",
            message,
            fix: Some(fix.to_string()),
        });
    };

    for err in config.security.validation_errors() {
        let fix = match err.field {
            SecurityField::Level | SecurityField::ModelQuality => {
                "请用 `gaze config` 选择受支持的安全等级。"
            }
            SecurityField::Threshold => {
                "请在 /etc/gaze/config.toml 中设置有效的 RGB 与红外自定义阈值。"
            }
            SecurityField::HybridPolicy => "请使用 default、or、fallback_on_dark 或 and。",
        };
        error(err.message, fix);
    }
    if let Err(err) = config.enrollment.validate() {
        error(
            err.to_string(),
            "请将 enrollment.min_face_size_ratio 设为 0.10 到 0.75 之间的值。",
        );
    }
    if let Err(err) = config.cameras.validate() {
        error(
            err.to_string(),
            "cameras.parallel_capture 请使用 never、auto 或 always。",
        );
    }
    if let Err(err) = config.inference.validate() {
        let fix = "请在 [inference] 中使用 cpu/cpu、auto/npu、openvino/cpu、openvino/gpu、openvino/npu 或 vitis/npu。";
        error(err.to_string(), fix);
    }

    let rgb = &config.cameras.rgb;
    let ir = config.cameras.ir.trim();
    if rgb.is_empty() && ir.is_empty() {
        error(
            "cameras.rgb 与 cameras.ir 均为空".to_string(),
            "请将 cameras.rgb 设为 \\\"primary\\\" 或配置红外摄像头。",
        );
    }
    for rgb in rgb.iter() {
        if let Some(index) = rgb.strip_prefix("/dev/video") {
            if index.is_empty() || !index.chars().all(|c| c.is_ascii_digit()) {
                error(
                    format!("RGB 摄像头节点无效：{rgb:?}"),
                    "请使用 /dev/video<编号>、usb:VVVV:PPPP、libcamera:<名称>、\\\"primary\\\" 或 GStreamer 来源。",
                );
            }
        } else if rgb.starts_with("usb:") && gaze_vision::camera::parse_usb_spec(rgb).is_none() {
            error(
                format!("RGB 的 USB 描述无效：{rgb:?}"),
                "请使用十六进制的 usb:VVVV:PPPP，例如 usb:046d:085e。",
            );
        } else if rgb.starts_with("libcamera:")
            && gaze_vision::camera::parse_libcamera_spec(rgb).is_none()
        {
            error(
                format!("RGB 的 libcamera 描述无效：{rgb:?}"),
                "请使用 libcamera:<摄像头名称>，名称可通过 `cam --list` 获取。",
            );
        }
    }

    if config.liveness.enabled {
        if !config.liveness.threshold.is_finite()
            || !(MIN_LIVENESS_THRESHOLD..=MAX_LIVENESS_THRESHOLD)
                .contains(&config.liveness.threshold)
        {
            error(
                format!(
                    "liveness.threshold 必须介于 {MIN_LIVENESS_THRESHOLD} 与 {MAX_LIVENESS_THRESHOLD} 之间，当前为 {}",
                    config.liveness.threshold
                ),
                "请将 liveness.threshold 设为 0.10 到 1.0 之间的值。",
            );
        }
        if !config.liveness.max_seconds.is_finite()
            || !(MIN_LIVENESS_MAX_SECONDS..=MAX_LIVENESS_MAX_SECONDS)
                .contains(&config.liveness.max_seconds)
        {
            error(
                format!(
                    "liveness.max_seconds 必须介于 {MIN_LIVENESS_MAX_SECONDS} 与 {MAX_LIVENESS_MAX_SECONDS} 之间，当前为 {}",
                    config.liveness.max_seconds
                ),
                "请将 liveness.max_seconds 设为 0.2 到 30.0 之间的值（默认 2.0）。",
            );
        }
        if config.liveness.ir_model
            && (!config.liveness.ir_threshold.is_finite()
                || !(MIN_LIVENESS_THRESHOLD..=MAX_LIVENESS_THRESHOLD)
                    .contains(&config.liveness.ir_threshold))
        {
            error(
                format!(
                    "liveness.ir_threshold 必须介于 {MIN_LIVENESS_THRESHOLD} 与 {MAX_LIVENESS_THRESHOLD} 之间，当前为 {}",
                    config.liveness.ir_threshold
                ),
                "请将 liveness.ir_threshold 设为 0.10 到 1.0 之间的值（默认 0.5）。",
            );
        }
    }

    if findings.is_empty() {
        findings.push(Check {
            level: Level::Pass,
            name: "配置项取值",
            message: "摄像头、安全、录入、推理与活体检测配置均有效".to_string(),
            fix: None,
        });
    }

    if config.cameras.emitter_enabled && ir.is_empty() {
        findings.push(Check {
            level: Level::Warning,
            name: "红外补光",
            message: "cameras.emitter_enabled 为 true，但 cameras.ir 为空".to_string(),
            fix: Some("请配置 cameras.ir，或关闭 emitter_enabled。".to_string()),
        });
    }
    if config.cameras.parallel_capture() == "always" && !ir.is_empty() {
        findings.push(Check {
            level: Level::Warning,
            name: "并行采集",
            message: "cameras.parallel_capture is \"always\", which streams RGB and IR at once \
                      without checking that the camera supports it"
                .to_string(),
            fix: Some(
                "If hybrid auth starts failing with \"IR camera stream stopped unexpectedly\", \
                 use \"auto\" or \"never\"."
                    .to_string(),
            ),
        });
    }
    if !config.liveness.enabled {
        findings.push(Check {
            level: Level::Warning,
            name: "活体检测",
            message: "活体防欺骗已关闭".to_string(),
            fix: Some("除非你明确接受照片或屏幕欺骗的风险，否则请启用 [liveness]。".to_string()),
        });
    }
    if config.liveness.ir_model && !config.liveness.enabled {
        findings.push(Check {
            level: Level::Warning,
            name: "红外防欺骗模型",
            message: "liveness.ir_model 为 true，但活体检测已关闭，因此不会生效".to_string(),
            fix: Some("请启用 [liveness] 或移除 ir_model。".to_string()),
        });
    } else if config.liveness.ir_model && ir.is_empty() {
        findings.push(Check {
            level: Level::Warning,
            name: "红外防欺骗模型",
            message: "liveness.ir_model 为 true，但 cameras.ir 为空，因此不会生效".to_string(),
            fix: Some("请配置 cameras.ir 或移除 ir_model。".to_string()),
        });
    }
    if config.enrollment.max_templates == 0 {
        findings.push(Check {
            level: Level::Warning,
            name: "录入上限",
            message: "max_templates 为 0，模板淘汰会被禁用".to_string(),
            fix: Some("请将 enrollment.max_templates 设为正数（默认 2）。".into()),
        });
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_default_config_has_no_errors() {
        let findings = config_findings(&Config::default());
        assert!(!findings.iter().any(|check| check.level == Level::Error));
    }

    #[test]
    fn ir_model_settings_are_checked() {
        let mut config = Config::default();
        config.liveness.ir_model = true;
        config.liveness.ir_threshold = 1.5;
        let findings = config_findings(&config);
        assert!(
            findings.iter().any(|check| check.level == Level::Error
                && check.message.contains("liveness.ir_threshold"))
        );
        assert!(
            findings.iter().any(|check| check.name == "红外防欺骗模型"
                && check.message.contains("cameras.ir"))
        );

        config.liveness.ir_threshold = 0.5;
        config.cameras.ir = "/dev/video2".to_string();
        let findings = config_findings(&config);
        assert!(!findings.iter().any(|check| check.name == "红外防欺骗模型"));
        assert!(!findings.iter().any(|check| check.level == Level::Error));
    }

    #[test]
    fn config_checks_invalid_thresholds_and_camera_sources() {
        let mut config = Config::default();
        config.security.level = "custom".to_string();
        config.security.detector = "standard".to_string();
        config.security.recognizer = "standard".to_string();
        config.security.rgb_threshold = 1.5;
        config.security.ir_threshold = -0.1;
        config.security.hybrid_policy = "sometimes".to_string();
        config.cameras.rgb = "/dev/videoX".into();
        config.enrollment.min_face_size_ratio = 0.05;
        config.liveness.threshold = f64::NAN;
        config.liveness.max_seconds = 0.0;

        let findings = config_findings(&config);
        let messages = findings
            .iter()
            .map(|check| check.message.as_str())
            .collect::<Vec<_>>();
        assert!(
            messages
                .iter()
                .any(|message| message.contains("security.rgb_threshold"))
        );
        assert!(
            messages
                .iter()
                .any(|message| message.contains("security.ir_threshold"))
        );
        assert!(
            messages
                .iter()
                .any(|message| message.contains("hybrid_policy"))
        );
        assert!(
            messages
                .iter()
                .any(|message| message.contains("/dev/videoX"))
        );
        assert!(
            messages
                .iter()
                .any(|message| message.contains("enrollment.min_face_size_ratio"))
        );
        assert!(
            messages
                .iter()
                .any(|message| message.contains("liveness.threshold"))
        );
        assert!(
            messages
                .iter()
                .any(|message| message.contains("max_seconds"))
        );
    }

    #[test]
    fn config_checks_the_parallel_capture_mode() {
        let mut config = Config::default();
        config.cameras.parallel_capture = "sometimes".to_string();
        assert!(
            config_findings(&config)
                .iter()
                .any(|check| check.level == Level::Error
                    && check.message.contains("cameras.parallel_capture"))
        );

        let mut forced = Config::default();
        forced.cameras.ir = "/dev/video2".to_string();
        forced.cameras.parallel_capture = "always".to_string();
        assert!(config_findings(&forced).iter().any(
            |check| check.level == Level::Warning && check.message.contains("parallel_capture")
        ));

        let mut detected = Config::default();
        detected.cameras.ir = "/dev/video2".to_string();
        detected.cameras.parallel_capture = "auto".to_string();
        assert!(
            !config_findings(&detected)
                .iter()
                .any(|check| check.message.contains("parallel_capture"))
        );
    }

    #[test]
    fn every_invalid_security_field_is_reported_exactly_once() {
        let mut config = Config::default();
        config.security.level = "custom".to_string();
        config.security.detector = "standard".to_string();
        config.security.recognizer = "standard".to_string();
        config.security.rgb_threshold = 1.5;
        config.security.ir_threshold = -0.1;
        config.security.hybrid_policy = "sometimes".to_string();

        let findings = config_findings(&config);
        let count = |needle: &str| {
            findings
                .iter()
                .filter(|check| check.message.contains(needle))
                .count()
        };
        assert_eq!(count("security.rgb_threshold"), 1);
        assert_eq!(count("security.ir_threshold"), 1);
        assert_eq!(count("security.hybrid_policy"), 1);
    }

    #[test]
    fn invalid_security_fields_get_their_own_fix() {
        let mut config = Config::default();
        config.security.level = "custom".to_string();
        config.security.detector = "standard".to_string();
        config.security.recognizer = "standard".to_string();
        config.security.rgb_threshold = 1.5;
        config.security.hybrid_policy = "sometimes".to_string();

        let fix_for = |needle: &str| {
            config_findings(&config)
                .into_iter()
                .find(|check| check.message.contains(needle))
                .and_then(|check| check.fix)
                .unwrap_or_default()
        };
        assert!(fix_for("security.rgb_threshold").contains("/etc/gaze/config.toml"));
        assert!(fix_for("security.hybrid_policy").contains("fallback_on_dark"));
    }
}
